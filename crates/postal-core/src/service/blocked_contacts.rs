use super::{WhatsAppService, Result, Serialize, Jid, contact_forms, other_form};
use crate::store::contact_identity::ContactIdentity;
use whatsapp_rust::wacore::iq::blocklist::UpdateBlocklistSpec;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct BlockedContact {
    pub jid: String,
    pub jids: Vec<String>,
    pub identity: ContactIdentity,
}

impl WhatsAppService {
    pub async fn blocked_contacts(&self) -> Result<Vec<BlockedContact>> {
        anyhow::ensure!(self.is_connected(), "not connected yet");
        let entries = self.client.blocking().get_blocklist().await?;
        let mut contacts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for entry in entries {
            let bare = block_target(&entry.jid.to_string())?;
            let jid = bare.to_string();
            if !seen.insert(jid.clone()) { continue; }
            other_form(&self.client, &self.store, &bare).await;
            let jids = contact_forms(&self.store, &jid).await;
            let key = jid.clone();
            let identity = self.store.run(move |store| store.contact_identity(&key)).await?;
            contacts.push(BlockedContact { jid, jids, identity });
        }
        anyhow::ensure!(self.is_connected(), "not connected yet");
        Ok(contacts)
    }

    pub async fn set_contact_blocked(&self, address: &str, blocked: bool, authorize: impl FnOnce() -> bool) -> Result<()> {
        let target = block_target(address)?;
        anyhow::ensure!(!self.is_self_jid(&target), "cannot block your own account");
        anyhow::ensure!(self.is_connected(), "not connected yet");
        update_blocklist(&target, blocked, async {
            Ok(if blocked || target.is_pn() {
                self.client.get_lid_pn_entry(&target).await?
                    .map(|entry| (Jid::lid(&*entry.lid), Jid::pn(&*entry.phone_number)))
            } else { None })
        }, || authorize() && self.is_connected(), |request| async move {
            self.client.execute(request).await.map_err(|error| anyhow::anyhow!(error.to_string()))
        }).await
    }
}

async fn update_blocklist<M, S, F>(target: &Jid, blocked: bool, mapping: M, authorize: impl FnOnce() -> bool, send: S) -> Result<()>
where
    M: std::future::Future<Output = Result<Option<(Jid, Jid)>>>,
    S: FnOnce(UpdateBlocklistSpec) -> F,
    F: std::future::Future<Output = Result<()>>,
{
    let request = block_request(target, blocked, mapping.await?)?;
    anyhow::ensure!(authorize(), "account changed or disconnected before operation");
    send(request).await
}

fn block_target(address: &str) -> Result<Jid> {
    let jid: Jid = address.parse()?;
    anyhow::ensure!((jid.is_pn() || jid.is_lid()) && jid.agent == 0 && jid.integrator == 0
        && !jid.user.is_empty() && jid.user.chars().all(|c| c.is_ascii_digit()),
        "blocklist target must be a contact");
    Ok(jid.to_non_ad())
}

fn block_request(target: &Jid, blocked: bool, mapping: Option<(Jid, Jid)>) -> Result<UpdateBlocklistSpec> {
    if !blocked && target.is_lid() { return Ok(UpdateBlocklistSpec::unblock(target)); }
    let (lid, pn) = mapping.ok_or_else(|| anyhow::anyhow!("no LID/phone mapping for this contact"))?;
    anyhow::ensure!(lid.is_lid() && pn.is_pn() && (*target == lid || *target == pn),
        "blocklist mapping belongs to another contact");
    Ok(if blocked { UpdateBlocklistSpec::block_with_pn(&lid, &pn) } else { UpdateBlocklistSpec::unblock(&lid) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::{wacore::iq::spec::IqSpec, wacore_binary::NodeContent};

    #[test]
    fn blocking_accepts_only_contact_addresses_and_normalizes_devices() {
        assert_eq!(block_target("59899000000:3@s.whatsapp.net").unwrap().to_string(), "59899000000@s.whatsapp.net");
        assert_eq!(block_target("77@lid").unwrap().to_string(), "77@lid");
        for address in ["status@broadcast", "123@g.us", "123@newsletter", "@lid", "name@lid", "59899000000"] {
            assert!(block_target(address).is_err(), "{address}");
        }
    }

    #[test]
    fn request_uses_both_address_forms_and_unblocks_unmapped_lids() {
        let lid = Jid::lid("77");
        let pn = Jid::pn("59899000000");
        for target in [&lid, &pn] {
            let request = block_request(target, true, Some((lid.clone(), pn.clone()))).unwrap().build_iq();
            let Some(NodeContent::Nodes(items)) = request.content else { panic!("missing blocklist item") };
            assert_eq!(items[0].attrs.get("jid").map(|value| value.to_string()), Some(lid.to_string()));
            assert_eq!(items[0].attrs.get("pn_jid").map(|value| value.to_string()), Some(pn.to_string()));
            assert_eq!(items[0].attrs.get("action").map(|value| value.to_string()).as_deref(), Some("block"));
        }
        assert!(block_request(&lid, false, None).is_ok());
        assert!(block_request(&pn, true, None).is_err());
        assert!(block_request(&pn, false, None).is_err());
        assert!(block_request(&Jid::lid("88"), true, Some((lid, pn))).is_err());
    }

    #[tokio::test]
    async fn mapping_await_rechecks_account_and_failed_ack_propagates() {
        use std::cell::Cell;
        let current = Cell::new(true);
        let writes = Cell::new(0);
        let lid = Jid::lid("77");
        let pn = Jid::pn("59899000000");
        let result = update_blocklist(&pn, true, async {
            tokio::task::yield_now().await;
            current.set(false);
            Ok(Some((lid.clone(), pn.clone())))
        }, || current.get(), |_| async { writes.set(writes.get() + 1); Ok(()) }).await;
        assert!(result.unwrap_err().to_string().contains("account changed"));
        assert_eq!(writes.get(), 0);
        current.set(true);
        let result = update_blocklist(&lid, false, async { Ok(None) }, || current.get(), |_| async {
            writes.set(writes.get() + 1);
            anyhow::bail!("synthetic server rejection")
        }).await;
        assert_eq!(result.unwrap_err().to_string(), "synthetic server rejection");
        assert_eq!(writes.get(), 1);
    }
}
