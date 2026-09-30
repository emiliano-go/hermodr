use super::{WhatsAppService, Result, Serialize, Jid};
use whatsapp_rust::wacore::iq::{devices::RemoveCompanionDeviceSpec, usync::{DeviceListSpec, DeviceListResponse}};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct LinkedDevice {
    pub jid: String,
    pub device_id: u16,
    pub is_current: bool,
    pub can_unlink: bool,
}

impl WhatsAppService {
    pub async fn linked_devices(&self) -> Result<Vec<LinkedDevice>> {
        anyhow::ensure!(self.is_connected(), "not connected yet");
        let identities = self.device_identities()?;
        let response = self.client.execute(DeviceListSpec::new(vec![identities[0].to_non_ad()],
            self.client.generate_message_id())
            .require_complete_response()).await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        device_rows(response, &identities)
    }

    pub async fn unlink_device(&self, address: &str, authorize: impl FnOnce() -> bool) -> Result<()> {
        let target = unlink_target(address, &self.device_identities()?)?;
        let devices = self.linked_devices().await?;
        authorize_unlink(&target, &devices, authorize)?;
        anyhow::ensure!(self.is_connected(), "not connected yet");
        self.client.execute(RemoveCompanionDeviceSpec::new(&target)).await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    fn device_identities(&self) -> Result<Vec<Jid>> {
        let identities = [self.client.pn(), self.client.lid()].into_iter().flatten()
            .filter(|jid| !jid.user.is_empty() && jid.device > 0 && (jid.is_pn() || jid.is_lid()))
            .collect::<Vec<_>>();
        anyhow::ensure!(!identities.is_empty(), "linked device identity is unavailable");
        Ok(identities)
    }
}

fn unlink_target(address: &str, identities: &[Jid]) -> Result<Jid> {
    let target: Jid = address.parse()?;
    anyhow::ensure!(target.device > 0 && target.agent == 0 && target.integrator == 0
        && identities.iter().any(|own| own.to_non_ad() == target.to_non_ad()),
        "device does not belong to this account");
    anyhow::ensure!(!identities.iter().any(|own| own.device == target.device),
        "use account logout to disconnect this device");
    Ok(target)
}

fn authorize_unlink(target: &Jid, devices: &[LinkedDevice], authorize: impl FnOnce() -> bool) -> Result<()> {
    anyhow::ensure!(devices.iter().any(|device| device.can_unlink && device.jid == target.to_string()),
        "device is no longer linked to this account");
    anyhow::ensure!(authorize(), "account changed before operation");
    Ok(())
}

fn device_rows(response: DeviceListResponse, identities: &[Jid]) -> Result<Vec<LinkedDevice>> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for list in response.device_lists {
        anyhow::ensure!(identities.iter().any(|own| own.to_non_ad() == list.user.to_non_ad()),
            "device list belongs to another account");
        for device in list.devices.into_iter().filter(|device| device.device > 0) {
            let jid = list.user.with_device_hosting(device.device, device.is_hosted);
            if !seen.insert((jid.device, device.is_hosted)) { continue; }
            let is_current = identities.iter().any(|own| own == &jid);
            rows.push(LinkedDevice { can_unlink: unlink_target(&jid.to_string(), identities).is_ok(),
                jid: jid.to_string(), device_id: jid.device, is_current });
        }
    }
    anyhow::ensure!(!rows.is_empty(), "linked device list is unavailable");
    rows.sort_by_key(|device| (!device.is_current, device.device_id));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::wacore::{iq::spec::IqSpec, usync::{UserDeviceList, UsyncDevice}};
    use whatsapp_rust::wacore_binary::{builder::NodeBuilder, NodeContent};

    fn identities() -> Vec<Jid> {
        ["100:2@s.whatsapp.net", "200:2@lid"].into_iter().map(|jid| jid.parse().unwrap()).collect()
    }

    #[test]
    fn unlink_requires_another_device_of_the_same_account() {
        let own = identities();
        for address in ["100@s.whatsapp.net", "100:0@s.whatsapp.net", "100:2@s.whatsapp.net",
            "200:2@lid", "300:3@s.whatsapp.net", "100:3@g.us", "100:3@hosted", "not a jid"] {
            assert!(unlink_target(address, &own).is_err(), "{address}");
        }
        for address in ["100:3@s.whatsapp.net", "200:3@lid"] {
            let target = unlink_target(address, &own).unwrap();
            let query = RemoveCompanionDeviceSpec::new(&target).build_iq();
            let Some(NodeContent::Nodes(children)) = query.content else { panic!("missing unlink payload"); };
            assert_eq!(query.namespace, "md");
            assert_eq!(children[0].tag, "remove-companion-device");
            assert_eq!(children[0].attrs().optional_string("jid").unwrap().as_ref(), address);
        }
    }

    #[test]
    fn device_rows_preserve_identity_and_do_not_invent_metadata() {
        let list = |user: &str| UserDeviceList { user: user.parse().unwrap(), devices: vec![
            UsyncDevice::new(0, None), UsyncDevice::new(3, None), UsyncDevice::new(2, None),
            UsyncDevice::new(3, None), UsyncDevice::new(4, None).with_hosting(true)],
            phash: None, key_index_bytes: None };
        let response = |user| DeviceListResponse { device_lists: vec![list(user)], lid_mappings: vec![] };
        let rows = device_rows(response("100@s.whatsapp.net"), &identities()).unwrap();
        assert_eq!(rows.iter().map(|row| row.device_id).collect::<Vec<_>>(), [2, 3, 4]);
        assert!(rows[0].is_current && !rows[0].can_unlink);
        assert_eq!(rows[1].jid, "100:3@s.whatsapp.net");
        assert!(rows[1].can_unlink);
        assert!(!rows[2].can_unlink);
        assert!(device_rows(response("300@s.whatsapp.net"), &identities()).is_err());
        assert!(device_rows(DeviceListResponse { device_lists: vec![], lid_mappings: vec![] }, &identities()).is_err());
        let mut aliases = response("100@s.whatsapp.net");
        aliases.device_lists.push(list("200@lid"));
        assert_eq!(device_rows(aliases, &identities()).unwrap().len(), 3);
        let json = serde_json::to_value(&rows[1]).unwrap();
        assert!(json.get("name").is_none() && json.get("last_active").is_none());
    }

    #[test]
    fn complete_list_rejects_missing_and_empty_account_results() {
        let query = DeviceListSpec::new(vec!["100@s.whatsapp.net".parse().unwrap()], "synthetic").require_complete_response();
        let empty = NodeBuilder::new("iq").children([NodeBuilder::new("usync")
            .children([NodeBuilder::new("list").build()]).build()]).build();
        assert!(query.parse_response(&empty.as_node_ref()).is_err());
        let user = NodeBuilder::new("user").attr("jid", "100@s.whatsapp.net")
            .children([NodeBuilder::new("devices").children([NodeBuilder::new("device-list").build()]).build()]).build();
        let empty_devices = NodeBuilder::new("iq").children([NodeBuilder::new("usync")
            .children([NodeBuilder::new("list").children([user]).build()]).build()]).build();
        assert!(query.parse_response(&empty_devices.as_node_ref()).is_err());
    }

    #[test]
    fn fresh_membership_and_account_binding_are_required_before_unlink() {
        let target = unlink_target("100:3@s.whatsapp.net", &identities()).unwrap();
        let row = LinkedDevice { jid: target.to_string(), device_id: 3, is_current: false, can_unlink: true };
        let called = std::cell::Cell::new(false);
        assert!(authorize_unlink(&target, &[], || { called.set(true); true }).is_err());
        assert!(!called.get());
        assert!(authorize_unlink(&target, std::slice::from_ref(&row), || false).is_err());
        assert!(authorize_unlink(&target, std::slice::from_ref(&row), || true).is_ok());
        assert!(authorize_unlink(&target, &[LinkedDevice { can_unlink: false, ..row }], || true).is_err());
    }
}
