use super::*;
use crate::store::history_pins::MessagePinUpdate;

fn expiry(timestamp: i64, duration: Option<u32>, kind: Option<wa::message_context_info::MessageAddonExpiryType>) -> Result<Option<i64>> {
    use wa::message_context_info::MessageAddonExpiryType;
    anyhow::ensure!(kind.is_none() || kind == Some(MessageAddonExpiryType::STATIC), "message pin expiry type is unsupported");
    duration.map(|duration| timestamp.checked_add(i64::from(duration) * 1000)
        .ok_or_else(|| anyhow::anyhow!("message pin expiry overflow"))).transpose()
}

fn target(key: Option<&wa::MessageKey>) -> Option<(String, Option<String>)> {
    let key = key?;
    let id = key.id.as_ref().filter(|id| !id.is_empty())?.clone();
    let remote = match key.remote_jid.as_deref() {
        Some(remote) => Some(remote.parse::<Jid>().ok()?.to_non_ad().to_string()),
        None => None,
    };
    Some((id, remote))
}

pub(super) fn history_message_pin(web: &wa::WebMessageInfo) -> Result<Option<MessagePinUpdate>> {
    use wa::pin_in_chat::Type;
    let Some(pin) = web.pin_in_chat.as_option() else { return Ok(None) };
    let pinned = match pin.r#type {
        Some(Type::PIN_FOR_ALL) => true,
        Some(Type::UNPIN_FOR_ALL) => false,
        _ => return Ok(None),
    };
    let Some((target, remote)) = target(pin.key.as_option()) else { return Ok(None) };
    let Some(timestamp) = pin.server_timestamp_ms.or(pin.sender_timestamp_ms).filter(|timestamp| *timestamp > 0) else { return Ok(None) };
    let expires_at = if pinned {
        let context = pin.message_add_on_context_info.as_option();
        expiry(timestamp, context.and_then(|context| context.message_add_on_duration_in_secs),
            context.and_then(|context| context.message_add_on_expiry_type))?
    } else { None };
    Ok(Some(MessagePinUpdate { target, remote, pinned, timestamp, expires_at }))
}

pub(super) fn live_message_pin(message: &wa::Message, fallback_timestamp: i64) -> Result<Option<MessagePinUpdate>> {
    use wa::message::pin_in_chat_message::Type;
    let Some(pin) = message.pin_in_chat_message.as_option() else { return Ok(None) };
    let pinned = match pin.r#type {
        Some(Type::PIN_FOR_ALL) => true,
        Some(Type::UNPIN_FOR_ALL) => false,
        _ => return Ok(None),
    };
    let Some((target, remote)) = target(pin.key.as_option()) else { return Ok(None) };
    let timestamp = pin.sender_timestamp_ms.unwrap_or(fallback_timestamp);
    anyhow::ensure!(timestamp > 0, "message pin timestamp is missing");
    let expires_at = if pinned {
        let context = message.message_context_info.as_option();
        expiry(timestamp, context.and_then(|context| context.message_add_on_duration_in_secs),
            context.and_then(|context| context.message_add_on_expiry_type))?
    } else { None };
    Ok(Some(MessagePinUpdate { target, remote, pinned, timestamp, expires_at }))
}

impl Inbound {
    pub(super) async fn apply_history_pin(&self, store: &StoreWorker, chat: &str, web: &wa::WebMessageInfo) {
        match history_message_pin(web) {
            Ok(Some(pin)) => {
                let chat = chat.to_owned();
                let event_chat = chat.clone();
                if store.run(move |store| store.apply_message_pin_update(&chat, &pin, true)).await.observed() == Some(true) {
                    let _ = self.events.send(ServiceEvent::Marks { chat: event_chat });
                }
            }
            Ok(None) => {}
            Err(error) => log::warn!("history message pin skipped: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web() -> wa::WebMessageInfo {
        wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey { id: Some("action-id".into()), ..Default::default() }),
            pin_in_chat: MessageField::some(wa::PinInChat {
                r#type: Some(wa::pin_in_chat::Type::PIN_FOR_ALL),
                key: MessageField::some(wa::MessageKey { id: Some("target-id".into()), remote_jid: Some("1@g.us".into()), ..Default::default() }),
                sender_timestamp_ms: Some(1000), server_timestamp_ms: Some(2000),
                message_add_on_context_info: MessageField::some(wa::MessageAddOnContextInfo {
                    message_add_on_duration_in_secs: Some(10),
                    message_add_on_expiry_type: Some(wa::message_context_info::MessageAddonExpiryType::STATIC),
                }), ..Default::default()
            }), ..Default::default()
        }
    }

    #[test]
    fn history_message_pin_reads_explicit_target_time_and_duration() {
        let pin = history_message_pin(&web()).unwrap().unwrap();
        assert_eq!(pin.target, "target-id");
        assert_eq!(pin.timestamp, 2000);
        assert_eq!(pin.expires_at, Some(12000));
        assert_eq!(pin.remote.as_deref(), Some("1@g.us"));
    }

    #[test]
    fn history_message_pin_absence_or_missing_target_never_clears_pins() {
        assert!(history_message_pin(&wa::WebMessageInfo::default()).unwrap().is_none());
        let mut web = web();
        web.pin_in_chat.as_option_mut().unwrap().key = MessageField::none();
        assert!(history_message_pin(&web).unwrap().is_none());
        web.pin_in_chat.as_option_mut().unwrap().key = MessageField::some(wa::MessageKey { id: Some("target-id".into()), ..Default::default() });
        web.pin_in_chat.as_option_mut().unwrap().server_timestamp_ms = None;
        assert_eq!(history_message_pin(&web).unwrap().unwrap().timestamp, 1000);
    }

    #[test]
    fn history_message_pin_rejects_unknown_parent_expiry_and_overflow() {
        let mut web = web();
        web.pin_in_chat.as_option_mut().unwrap().message_add_on_context_info.as_option_mut().unwrap().message_add_on_expiry_type =
            Some(wa::message_context_info::MessageAddonExpiryType::DEPENDENT_ON_PARENT);
        assert!(history_message_pin(&web).is_err());
        assert!(expiry(i64::MAX, Some(1), None).is_err());
        web.pin_in_chat.as_option_mut().unwrap().r#type = Some(wa::pin_in_chat::Type::UNPIN_FOR_ALL);
        assert!(!history_message_pin(&web).unwrap().unwrap().pinned);
    }
}
