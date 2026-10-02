use super::*;

fn event() -> crate::store::Event {
    crate::store::Event {
        pinned: false,
        id: "event".into(),
        name: "Dinner".into(),
        description: None,
        start: Some(100),
        end: Some(200),
        location: None,
        link: None,
        canceled: false,
        extra_guests_allowed: Some(true),
        is_scheduled_call: None,
        has_reminder: None,
        reminder_offset_sec: None,
        invitation_id: None,
        invitation: false,
        can_respond: true,
        responses: Vec::new(),
    }
}

fn draft() -> crate::store::NewEvent {
    crate::store::NewEvent {
        name: "Dinner".into(),
        start: Some(100),
        end: Some(200),
        ..Default::default()
    }
}

#[test]
fn create_and_edit_validate_supported_fields_and_preserve_readonly_reminders() {
    assert!(validate_create(&draft()).is_ok());
    let mut existing = event();
    existing.has_reminder = Some(true);
    existing.reminder_offset_sec = Some(-120);
    let mut changed = draft();
    changed.has_reminder = Some(true);
    changed.reminder_offset_sec = Some(-120);
    assert!(validate_edit(&existing, &changed).is_ok());
    assert!(validate_create(&changed).is_err());
    changed.reminder_offset_sec = Some(120);
    assert!(validate_edit(&existing, &changed).is_err());
    changed = draft();
    changed.end = Some(99);
    assert!(validate_create(&changed).is_err());
    changed = draft();
    changed.invitation = true;
    assert!(validate_create(&changed).is_err());
    existing = event();
    existing.invitation = true;
    assert!(validate_edit(&existing, &draft()).is_err());
}

#[test]
fn event_projection_preserves_metadata_without_inventing_invitation_identity() {
    let mut original = draft();
    original.extra_guests_allowed = Some(true);
    original.is_scheduled_call = Some(false);
    original.has_reminder = Some(true);
    original.reminder_offset_sec = Some(-60);
    let decoded = super::super::polls::event_of(&event_content(&original)).unwrap();
    assert_eq!(decoded.extra_guests_allowed, Some(true));
    assert_eq!(decoded.is_scheduled_call, Some(false));
    assert_eq!(decoded.has_reminder, Some(true));
    assert_eq!(decoded.reminder_offset_sec, Some(-60));
    assert_eq!(decoded.end, Some(200));
    assert!(!decoded.invitation);
    let invite = wa::Message {
        event_invite_message: MessageField::some(wa::message::EventInviteMessage {
            event_title: Some("Invitation".into()),
            caption: Some("Come along".into()),
            start_time: Some(100),
            end_time: Some(200),
            call_link: Some("https://example.test/call".into()),
            is_canceled: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let decoded = super::super::polls::event_of(&invite).unwrap();
    assert!(decoded.invitation && decoded.invitation_id.is_none());
    assert_eq!(decoded.name, "Invitation");
    assert!(decoded.canceled);
    assert_eq!(decoded.end, Some(200));
    assert_eq!(decoded.description.as_deref(), Some("Come along"));
}

#[test]
fn response_validation_rejects_clear_private_invitation_canceled_and_invalid_guest_requests() {
    let mut current = event();
    for response in ["going", "maybe", "not_going"] {
        assert!(validate_response(&current, response, None).is_ok());
    }
    for response in ["clear", "", "unknown"] {
        assert!(validate_response(&current, response, None).is_err());
    }
    assert!(validate_response(&current, "going", Some(1)).is_ok());
    assert!(validate_response(&current, "going", Some(-1)).is_err());
    assert!(validate_response(&current, "maybe", Some(1)).is_ok());
    current.extra_guests_allowed = Some(false);
    assert!(validate_response(&current, "going", Some(1)).is_err());
    current = event();
    current.canceled = true;
    assert!(validate_response(&current, "going", None).is_err());
    current = event();
    current.invitation = true;
    assert!(validate_response(&current, "going", None).is_err());
    current = event();
    current.can_respond = false;
    assert!(validate_response(&current, "going", None).is_err());
}

#[test]
fn public_event_target_rejects_private_rows() {
    let mut public = StoredMessage::default();
    public.media.kind = Some("event".into());
    assert!(validate_public_target(&public).is_ok());
    for flag in 0..7 {
        let mut row = public.clone();
        match flag {
            0 => row.local.deleted = true,
            1 => row.local.revoked = true,
            2 => row.spoiler = true,
            3 => row.media.once_kind = Some("event".into()),
            4 => row.media.kind = Some("event_invite".into()),
            5 => row.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
            _ => row.media.kind = Some("view_once".into()),
        }
        assert!(validate_public_target(&row).is_err());
    }
}

fn cipher(response: wa::message::EventResponseMessage) -> wa::message::EncEventResponseMessage {
    let (payload, iv) = whatsapp_rust::wacore::event::encrypt_event_response_with_secret(
        &response,
        &[7; 32],
        "event",
        "15550000001@s.whatsapp.net",
        "15550000002@s.whatsapp.net",
    )
    .unwrap();
    wa::message::EncEventResponseMessage {
        event_creation_message_key: MessageField::some(wa::MessageKey {
            id: Some("event".into()),
            ..Default::default()
        }),
        enc_payload: Some(payload),
        enc_iv: Some(iv.to_vec()),
    }
}

#[test]
fn authenticated_response_preserves_authored_timestamp_guests_and_source_id() {
    let payload = cipher(wa::message::EventResponseMessage {
        response: Some(wa::message::event_response_message::EventResponseType::GOING),
        timestamp_ms: Some(1700000000000),
        extra_guest_count: Some(2),
    });
    let opened = decrypt_snapshot(
        "source",
        "event",
        &[7; 32],
        &[Jid::lid("111"), Jid::pn("15550000001")],
        &[Jid::lid("222"), Jid::pn("15550000002")],
        &payload,
    )
    .unwrap();
    assert_eq!(opened.response, "going");
    assert_eq!(opened.timestamp_ms, Some(1700000000000));
    assert_eq!(opened.extra_guest_count, Some(2));
    assert_eq!(opened.source_id, "source");
    assert!(decrypt_snapshot(
        "source",
        "other",
        &[7; 32],
        &[Jid::pn("15550000001")],
        &[Jid::pn("15550000002")],
        &payload
    )
    .is_err());
    assert!(decrypt_snapshot(
        "source",
        "event",
        &[7; 32],
        &[Jid::pn("15550000001")],
        &[Jid::pn("15550000003")],
        &payload
    )
    .is_err());
}

#[test]
fn received_unset_response_stays_unset_without_fabricated_timestamp_and_invalid_counts_fail() {
    let opened = response_snapshot("source", wa::message::EventResponseMessage::default()).unwrap();
    assert_eq!(opened.response, "");
    assert_eq!(opened.timestamp_ms, None);
    assert_eq!(opened.extra_guest_count, None);
    assert!(response_snapshot(
        "source",
        wa::message::EventResponseMessage {
            extra_guest_count: Some(-1),
            ..Default::default()
        }
    )
    .is_err());
    assert!(response_snapshot(
        "source",
        wa::message::EventResponseMessage {
            timestamp_ms: Some(0),
            ..Default::default()
        }
    )
    .is_err());
    assert!(response_snapshot("", wa::message::EventResponseMessage::default()).is_err());
}

#[tokio::test]
async fn namespace_candidates_require_existing_source_approved_pairs() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    assert_eq!(
        namespace_forms(&store, None, &Jid::lid("111"))
            .await
            .unwrap(),
        vec![Jid::lid("111")]
    );
    store.set_lid_pn("111", "15550000001").await.unwrap();
    assert_eq!(
        namespace_forms(&store, None, &Jid::lid("111"))
            .await
            .unwrap(),
        vec![Jid::lid("111"), Jid::pn("15550000001")]
    );
    assert!(namespace_forms(&store, None, &"123@g.us".parse().unwrap())
        .await
        .is_err());
}
