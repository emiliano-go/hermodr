use super::*;
use whatsapp_rust::wacore::types::{events::DecryptedPayload, message::MessageSource};

fn info(id: &str) -> Arc<MessageInfo> {
    Arc::new(MessageInfo {
        id: id.into(),
        source: MessageSource {
            chat: "1@g.us".parse().unwrap(),
            sender: "100@s.whatsapp.net".parse().unwrap(),
            is_group: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn target(id: &str) -> wa::MessageKey {
    wa::MessageKey {
        id: Some(id.into()),
        remote_jid: Some("1@g.us".into()),
        participant: Some("100@s.whatsapp.net".into()),
        ..Default::default()
    }
}

fn envelope(id: &str, kind: wa::message::secret_encrypted_message::SecretEncType) -> wa::Message {
    wa::Message {
        secret_encrypted_message: MessageField::some(wa::message::SecretEncryptedMessage {
            target_message_key: MessageField::some(target(id)),
            secret_enc_type: Some(kind),
            enc_payload: Some(vec![7; 32]),
            enc_iv: Some(vec![0; 12]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn poll_body() -> wa::Message {
    wa::Message {
        poll_creation_message_v3: MessageField::some(wa::message::PollCreationMessage {
            name: Some("changed".into()),
            options: vec![
                wa::message::poll_creation_message::Option {
                    option_name: Some("one".into()),
                    ..Default::default()
                },
                wa::message::poll_creation_message::Option {
                    option_name: Some("two".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn inbound(info: Arc<MessageInfo>, message: wa::Message) -> InboundMessage {
    InboundMessage::builder().info(info).message(Arc::new(message)).build()
}

fn capture(edits: &SecretEdits, info: Arc<MessageInfo>, message: &wa::Message) {
    let raw = DecryptedPayload::builder()
        .info(info)
        .enc_index(0)
        .enc_type("msg")
        .payload(message.encode_to_vec().into())
        .build();
    edits.handler().handle_event(Arc::new(Event::DecryptedPayload(raw)));
}

#[test]
fn exact_raw_info_survives_but_same_id_retry_cannot_borrow_target() {
    use wa::message::secret_encrypted_message::SecretEncType;
    let original = info("edit");
    let retry = info("edit");
    let mut captures = Captures::default();
    let now = Instant::now();
    captures.record(&original, &envelope("parent", SecretEncType::POLL_EDIT).encode_to_vec(), now);
    assert!(
        matches!(captures.candidate(&original, now), Some(Candidate::Envelope { target, kind: SecretEncKind::PollEdit }) if target.id.as_deref() == Some("parent"))
    );
    assert!(captures.candidate(&retry, now).is_none());
    captures.record(&retry, &poll_body().encode_to_vec(), now);
    assert!(matches!(captures.candidate(&retry, now), Some(Candidate::Plain)));
    assert!(matches!(captures.candidate(&original, now), Some(Candidate::Envelope { .. })));
}

#[test]
fn divergent_enc_payloads_fail_closed_and_duplicates_keep_provenance() {
    use wa::message::secret_encrypted_message::SecretEncType;
    let info = info("edit");
    let mut captures = Captures::default();
    let now = Instant::now();
    let first = envelope("first", SecretEncType::POLL_EDIT).encode_to_vec();
    captures.record(&info, &first, now);
    captures.record(&info, &first, now);
    assert!(matches!(captures.candidate(&info, now), Some(Candidate::Envelope { .. })));
    captures.record(&info, &envelope("second", SecretEncType::POLL_EDIT).encode_to_vec(), now);
    assert!(matches!(captures.candidate(&info, now), Some(Candidate::Rejected)));
    captures.record(&info, &first, now);
    assert!(matches!(captures.candidate(&info, now), Some(Candidate::Rejected)));
}

#[test]
fn cache_is_bounded_and_live_expiry_degrades_missing_provenance() {
    let now = Instant::now();
    let mut captures = Captures::default();
    let held: Vec<_> = (0..=CAPACITY).map(|index| info(&index.to_string())).collect();
    for info in &held {
        captures.record(info, &poll_body().encode_to_vec(), now);
    }
    assert_eq!(captures.entries.len(), CAPACITY);
    assert!(captures.degraded);
    assert_eq!(
        held[..CAPACITY]
            .iter()
            .filter(|info| captures.candidate(info, now).is_none())
            .count(),
        1
    );
    assert!(matches!(captures.candidate(&held[CAPACITY], now), Some(Candidate::Plain)));
    let mut captures = Captures::default();
    captures.record(&held[0], &poll_body().encode_to_vec(), now);
    assert!(captures.candidate(&held[0], now + MAX_AGE).is_none());
    assert!(captures.degraded);
    let mut captures = Captures::default();
    {
        let gone = info("gone");
        captures.record(&gone, &poll_body().encode_to_vec(), now);
    }
    captures.prune(now + MAX_AGE);
    assert!(captures.entries.is_empty());
    assert!(!captures.degraded);
}

#[test]
fn hashes_are_explicit_and_wrong_kind_or_mixed_body_is_rejected() {
    let hash = whatsapp_rust::wacore::poll::compute_option_hash("old label");
    let raw: String = hash.iter().map(|byte| format!("{byte:02X}")).collect();
    let option = wa::message::poll_creation_message::Option {
        option_name: Some("new label".into()),
        option_hash: Some(raw),
        ..Default::default()
    };
    assert_eq!(poll_option(&option).unwrap().hash, hash);
    use whatsapp_rust::wacore::poll::{decrypt_poll_vote_with_secret, encrypt_poll_vote_with_secret, PollVoteCiphertext};
    let (payload, iv) =
        encrypt_poll_vote_with_secret(&[hash.to_vec()], &[9; 32], "parent", "100@s.whatsapp.net", "200@s.whatsapp.net").unwrap();
    let cipher = || PollVoteCiphertext {
        enc_payload: &payload,
        enc_iv: &iv,
    };
    assert_eq!(
        decrypt_poll_vote_with_secret(cipher(), &[9; 32], "parent", "100@s.whatsapp.net", "200@s.whatsapp.net").unwrap(),
        vec![hash.to_vec()]
    );
    assert!(decrypt_poll_vote_with_secret(cipher(), &[9; 32], "other", "100@s.whatsapp.net", "200@s.whatsapp.net").is_err());
    assert!(decrypt_poll_vote_with_secret(cipher(), &[9; 32], "parent", "100@s.whatsapp.net", "300@s.whatsapp.net").is_err());
    let mut invalid = option;
    invalid.option_hash = Some("z".repeat(64));
    assert!(poll_option(&invalid).is_err());
    invalid.option_hash = Some("é".repeat(32));
    assert!(poll_option(&invalid).is_err());
    assert!(edit_body(&poll_body(), SecretEncKind::EventEdit).unwrap().is_none());
    let mut mixed = poll_body();
    mixed.event_message = MessageField::some(wa::message::EventMessage::default());
    assert!(edit_body(&mixed, SecretEncKind::PollEdit).unwrap().is_none());
}

#[tokio::test]
async fn failed_open_unmatched_raw_and_conflicting_fanout_never_create_poll_rows() {
    use wa::message::secret_encrypted_message::SecretEncType;
    let edits = SecretEdits::default();
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let raw_info = info("edit");
    let outer = envelope("parent", SecretEncType::POLL_EDIT);
    capture(&edits, raw_info.clone(), &outer);
    assert!(matches!(
        edits.apply(&store, &inbound(raw_info.clone(), outer), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
    assert!(matches!(
        edits
            .apply(&store, &inbound(info("edit"), poll_body()), "1@g.us", &[])
            .await
            .unwrap(),
        Outcome::Drop
    ));
    capture(&edits, raw_info.clone(), &envelope("other", SecretEncType::POLL_EDIT));
    assert!(matches!(
        edits.apply(&store, &inbound(raw_info, poll_body()), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn uncorrelated_controls_and_plain_mixed_add_fail_closed() {
    let edits = SecretEdits::default();
    edits.0.lock().unwrap().degraded = true;
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let legacy = wa::Message {
        protocol_message: MessageField::some(wa::message::ProtocolMessage {
            r#type: Some(wa::message::protocol_message::Type::MESSAGE_EDIT),
            key: MessageField::some(target("parent")),
            edited_message: MessageField::some(poll_body()),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        edits.apply(&store, &inbound(info("legacy"), legacy), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
    let add = wa::Message {
        poll_add_option_message: MessageField::some(wa::message::PollAddOptionMessage {
            poll_creation_message_key: MessageField::some(target("parent")),
            add_option: MessageField::some(wa::message::poll_creation_message::Option {
                option_name: Some("three".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        edits
            .apply(&store, &inbound(info("add"), add.clone()), "1@g.us", &[])
            .await
            .unwrap(),
        Outcome::Drop
    ));
    let mut mixed = add;
    mixed.event_message = MessageField::some(wa::message::EventMessage {
        name: Some("event".into()),
        ..Default::default()
    });
    let mixed_info = info("mixed");
    capture(&edits, mixed_info.clone(), &mixed);
    assert!(matches!(
        edits.apply(&store, &inbound(mixed_info, mixed), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
}

#[tokio::test]
async fn decrypted_edit_uses_captured_target_and_rejects_cross_chat_or_inner_target_swap() {
    use wa::message::secret_encrypted_message::SecretEncType;
    let edits = SecretEdits::default();
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    store
        .run(|store| {
            store.insert_message(&StoredMessage {
                header: MessageHeader {
                    chat: "1@g.us".into(),
                    id: "parent".into(),
                    sender: "100@s.whatsapp.net".into(),
                    ..Default::default()
                },
                text: "original".into(),
                media: Media {
                    kind: Some("poll".into()),
                    ..Default::default()
                },
                ..Default::default()
            })?;
            store.save_poll(
                "1@g.us",
                "parent",
                "100@s.whatsapp.net",
                "original",
                &["one".into(), "two".into()],
                false,
                Some(&[9; 32]),
            )
        })
        .await
        .unwrap();
    let valid = info("edit");
    capture(&edits, valid.clone(), &envelope("parent", SecretEncType::POLL_EDIT));
    assert!(matches!(
        edits.apply(&store, &inbound(valid, poll_body()), "1@g.us", &[]).await.unwrap(),
        Outcome::Applied { id } if id == "parent"
    ));
    let row = store.message("1@g.us", "parent").await.unwrap();
    assert_eq!(row.text, "changed");
    assert_eq!(store.count().await.unwrap(), 1);
    let cross = info("cross");
    let mut outer = envelope("parent", SecretEncType::POLL_EDIT);
    outer
        .secret_encrypted_message
        .as_option_mut()
        .unwrap()
        .target_message_key
        .as_option_mut()
        .unwrap()
        .remote_jid = Some("2@g.us".into());
    capture(&edits, cross.clone(), &outer);
    assert!(matches!(
        edits.apply(&store, &inbound(cross, poll_body()), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
    let swapped = info("add");
    capture(&edits, swapped.clone(), &envelope("parent", SecretEncType::POLL_ADD_OPTION));
    let body = wa::Message {
        poll_add_option_message: MessageField::some(wa::message::PollAddOptionMessage {
            poll_creation_message_key: MessageField::some(target("other")),
            add_option: MessageField::some(wa::message::poll_creation_message::Option {
                option_name: Some("three".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        edits.apply(&store, &inbound(swapped, body), "1@g.us", &[]).await.unwrap(),
        Outcome::Drop
    ));
}
