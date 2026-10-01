use super::*;
use crate::store::SystemNotice;

pub(super) fn scheduled_call_notice(message: &wa::Message) -> Option<SystemNotice> {
    let decoded = decoded_message(message);
    if decoded.spoiler || decoded.view_once { return None; }
    let base = decoded.message;
    let creation = base.scheduled_call_creation_message.as_option();
    let edit = base.scheduled_call_edit_message.as_option();
    if creation.is_some() && edit.is_some() { return None; }
    if let Some(call) = creation {
        use wa::message::scheduled_call_creation_message::CallType;
        let kind = match call.call_type {
            Some(CallType::VOICE) => "voice",
            Some(CallType::VIDEO) => "video",
            _ => "",
        };
        return Some(SystemNotice { kind: Some("SCHEDULED_CALL_CREATED".into()), params: vec![
            call.title.clone().unwrap_or_default(),
            call.scheduled_timestamp_ms.filter(|at| *at > 0).map(|at| (at / 1000).to_string()).unwrap_or_default(),
            kind.into(),
        ] });
    }
    let edit = edit?;
    if edit.edit_type != Some(wa::message::scheduled_call_edit_message::EditType::CANCEL) { return None; }
    let target = edit.key.as_option()?.id.as_deref().filter(|id| !id.trim().is_empty())?;
    Some(SystemNotice { kind: Some("SCHEDULED_CALL_CANCEL".into()), params: vec![target.into()] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creation() -> wa::Message {
        wa::Message { scheduled_call_creation_message: MessageField::some(wa::message::ScheduledCallCreationMessage {
            title: Some("Synthetic call @ home".into()), scheduled_timestamp_ms: Some(2_000_000_000_123),
            call_type: Some(wa::message::scheduled_call_creation_message::CallType::VIDEO),
        }), ..Default::default() }
    }

    #[tokio::test]
    async fn scheduled_call_creation_keeps_wire_units_and_message_identity() {
        let wrapped = wa::Message { ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(creation()), ..Default::default()
        }), ..Default::default() };
        let header = MessageHeader { chat: "100@g.us".into(), id: "call-creation".into(), sender: "200@lid".into(), timestamp: 1000, from_me: true };
        let row = stored_message(&wrapped, header.clone(), None, None, false).await.unwrap();
        assert_eq!(row.header, header);
        assert_eq!(row.system.kind.as_deref(), Some("SCHEDULED_CALL_CREATED"));
        assert_eq!(row.system.params, vec!["Synthetic call @ home", "2000000000", "video"]);
        assert!(row.local.read);
        assert!(row.media.kind.is_none());
    }

    #[test]
    fn scheduled_call_edits_require_cancel_and_a_nonempty_target() {
        use wa::message::scheduled_call_edit_message::EditType;
        let mut message = wa::Message { scheduled_call_edit_message: MessageField::some(wa::message::ScheduledCallEditMessage {
            key: MessageField::some(wa::MessageKey { id: Some("original-call".into()), ..Default::default() }),
            edit_type: Some(EditType::CANCEL),
        }), ..Default::default() };
        let notice = scheduled_call_notice(&message).unwrap();
        assert_eq!(notice.kind.as_deref(), Some("SCHEDULED_CALL_CANCEL"));
        assert_eq!(notice.params, vec!["original-call"]);
        message.scheduled_call_edit_message = MessageField::some(wa::message::ScheduledCallEditMessage { edit_type: Some(EditType::UNKNOWN), ..Default::default() });
        assert!(scheduled_call_notice(&message).is_none());
        message.scheduled_call_edit_message = MessageField::some(wa::message::ScheduledCallEditMessage {
            key: MessageField::some(wa::MessageKey { id: Some(" ".into()), ..Default::default() }), edit_type: Some(EditType::CANCEL),
        });
        assert!(scheduled_call_notice(&message).is_none());
    }

    #[test]
    fn scheduled_call_decoder_preserves_unknown_and_hidden_payloads() {
        let mut message = creation();
        message.scheduled_call_creation_message = MessageField::some(wa::message::ScheduledCallCreationMessage {
            title: Some("Synthetic call @ home".into()), scheduled_timestamp_ms: Some(-1000), call_type: None,
        });
        assert_eq!(scheduled_call_notice(&message).unwrap().params, vec!["Synthetic call @ home", "", ""]);
        message.scheduled_call_edit_message = MessageField::some(wa::message::ScheduledCallEditMessage::default());
        assert!(scheduled_call_notice(&message).is_none());
        assert!(scheduled_call_notice(&wa::Message::default()).is_none());
        let spoiler = wa::Message { spoiler_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(creation()), ..Default::default()
        }), ..Default::default() };
        assert!(scheduled_call_notice(&spoiler).is_none());
    }

    #[test]
    fn outgoing_event_revision_captures_transport_seconds_before_waiting_for_ack() {
        let source = include_str!("polls.rs").split("pub async fn edit_event(").nth(1).unwrap()
            .split("pub async fn respond_event(").next().unwrap();
        let send = source.find(".edit_message_encrypted(").unwrap();
        let capture = source.find("let timestamp_ms = unix_now() * 1000;").unwrap();
        assert!(capture < send, "ACK completion must not assign the authored revision");
        let after_send = &source[send..];
        assert!(!after_send.contains("now_millis()") && !after_send.contains("unix_now()"));
        assert!(after_send.contains("EditRevision { timestamp_ms, message_id: result.message_id }"));
    }
}
