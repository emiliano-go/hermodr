use super::*;
use crate::message_ref::MessageRef;
use whatsapp_rust::UsernameLookup;
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UsernameLookupResult {
    Found {
        jid: String,
        username: Option<String>,
    },
    NotFound,
    KeyRequired {
        username: Option<String>,
    },
}

fn found(jid: &Jid, username: Option<&str>) -> Result<UsernameLookupResult> {
    anyhow::ensure!(
        (jid.is_pn() || jid.is_lid()) && !jid.user.is_empty(),
        MessageRef::new("error.username_result_address_invalid")
    );
    Ok(UsernameLookupResult::Found {
        jid: jid.to_non_ad().to_string(),
        username: username.map(str::to_owned),
    })
}

fn map_lookup(lookup: UsernameLookup) -> Result<UsernameLookupResult> {
    match lookup {
        UsernameLookup::Found(user) => found(&user.jid, user.username.as_deref()),
        UsernameLookup::NotFound => Ok(UsernameLookupResult::NotFound),
        UsernameLookup::KeyRequired { username } => Ok(UsernameLookupResult::KeyRequired {
            username: username.map(|name| name.to_string()),
        }),
        _ => anyhow::bail!(MessageRef::new("error.username_result_unsupported")),
    }
}

impl WhatsAppService {
    pub async fn lookup_username(
        &self,
        username: &str,
        username_key: Option<&str>,
        authorize: impl Fn() -> bool,
    ) -> Result<UsernameLookupResult> {
        anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
        anyhow::ensure!(self.is_connected(), MessageRef::new("error.not_connected"));
        let lookup = self
            .client
            .contacts()
            .find_by_username(username.trim(), username_key)
            .await?;
        let result = map_lookup(lookup)?;
        anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
        let mut changed = false;
        if let UsernameLookupResult::Found { jid, .. } = &result {
            changed = import_sdk_contact_mapping(
                &self.client,
                &self.store,
                &jid.parse::<Jid>()?,
                &authorize,
            )
            .await?;
        }
        anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
        let (result, username_changed) = self
            .store
            .run(move |store| persist_lookup(store, result))
            .await?;
        changed |= username_changed;
        anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
        if changed {
            let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 });
        }
        Ok(result)
    }
}

pub(super) async fn import_sdk_contact_mapping(
    client: &Client,
    store: &StoreWorker,
    jid: &Jid,
    authorize: impl Fn() -> bool,
) -> Result<bool> {
    anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
    let Some(entry) = client.get_lid_pn_entry(jid).await? else {
        return Ok(false);
    };
    anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
    let lid = user_part(&entry.lid);
    let pn = user_part(&entry.phone_number);
    let changed = store
        .lid_pn(&lid)
        .await?
        .as_ref()
        .is_none_or(|(_, old)| old != &pn);
    anyhow::ensure!(authorize(), MessageRef::new("error.account_changed"));
    store.set_lid_pn(&lid, &pn).await?;
    Ok(changed)
}

fn persist_lookup(
    store: &MessageStore,
    mut result: UsernameLookupResult,
) -> Result<(UsernameLookupResult, bool)> {
    let mut changed = false;
    if let UsernameLookupResult::Found { jid, username } = &mut result {
        if let Some(username) = username {
            changed = store.set_username(jid, username)?;
        }
        *jid = store.canonical_chat(jid)?.into_owned();
    }
    Ok((result, changed))
}

pub(super) async fn record_incoming_call(
    store: &StoreWorker,
    client: Option<&Client>,
    call: &IncomingCall,
) -> Result<bool> {
    if !matches!(call.action, CallAction::Offer { .. }) {
        return Ok(false);
    }
    let source = call.from.to_non_ad();
    anyhow::ensure!(
        (source.is_pn() || source.is_lid()) && !source.user.is_empty(),
        "Incoming call has an invalid contact address."
    );
    let mut changed = false;
    if let Some(client) = client {
        changed = import_sdk_contact_mapping(client, store, &source, || true).await?;
    }
    if let Some(username) = &call.caller_username {
        changed |= store.set_username(&source.to_string(), username).await?;
    }
    Ok(changed)
}

#[cfg(test)]
#[path = "usernames_tests.rs"]
mod tests;
