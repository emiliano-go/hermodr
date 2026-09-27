use super::*;
use std::path::Path;

#[tokio::test]
async fn incoming_media_captions_keep_wire_mentions_through_storage() {
    let caption = "Look @12345\nsecond line";
    let context = wa::ContextInfo { mentioned_jid: vec!["12345@lid".into()], ..Default::default() };
    let image = wa::Message {
        image_message: MessageField::some(wa::message::ImageMessage {
            caption: Some(caption.into()), context_info: MessageField::some(context.clone()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let video = wa::Message {
        video_message: MessageField::some(wa::message::VideoMessage {
            caption: Some(caption.into()), context_info: MessageField::some(context.clone()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let document = wa::Message {
        document_message: MessageField::some(wa::message::DocumentMessage {
            caption: Some(caption.into()), context_info: MessageField::some(context),
            ..Default::default()
        }),
        ..Default::default()
    };
    let wrapped = wa::Message {
        ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(image.clone()), ..Default::default()
        }),
        ..Default::default()
    };
    let store = MessageStore::open(Path::new(":memory:"), Retention::unlimited()).unwrap();
    let once = wa::Message {
        view_once_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(image.clone()), ..Default::default()
        }), ..Default::default()
    };
    for (index, wire) in [image, video, document, wrapped].into_iter().enumerate() {
        let header = MessageHeader { chat: "group@g.us".into(), id: index.to_string(), sender: "other@lid".into(), timestamp: 1, from_me: false };
        let mut stored = stored_message(&wire, header, None, None, false).await.unwrap();
        assert_eq!(stored.text, caption);
        assert!(stored.media.locator.is_some());
        stored.local.mentioned = mentions_me(&wire, &["12345@lid".into()]);
        assert!(stored.local.mentioned);
        store.insert_message(&stored).unwrap();
        let saved = store.message("group@g.us", &index.to_string()).unwrap();
        assert_eq!(saved.text, caption);
        assert!(saved.local.mentioned);
    }
    let empty = wa::Message { image_message: MessageField::some(Default::default()), ..Default::default() };
    let stored = stored_message(&empty, MessageHeader::default(), None, None, false).await.unwrap();
    assert_eq!(stored.text, "[image]");
    let stored = stored_message(&once, MessageHeader::default(), None, None, false).await.unwrap();
    assert_eq!(stored.text, "[image]");
    assert_eq!(stored.media.kind.as_deref(), Some("view_once"));
    assert!(stored.media.thumb.is_none());
}

#[tokio::test]
async fn group_changes_invalidate_fetched_metadata_and_overviews() {
    use whatsapp_rust::wacore::{
        stanza::groups::GroupNotificationAction,
        types::events::GroupUpdate,
    };
    let (events, mut received) = broadcast::channel(32);
    let inbound = Inbound {
        store: Arc::new(MessageStore::open(Path::new(":memory:"), Retention::unlimited()).unwrap()),
        events,
        connected: Arc::default(),
        client_for_events: Arc::default(),
        media_dir: None,
        group_cache: Arc::default(),
        groups_cache: Arc::default(),
        older_waits: Arc::default(),
        downloads: Arc::new(tokio::sync::Semaphore::new(1)),
        sync_progress: Arc::default(),
        auto_download_default: false,
        keep_archived: Arc::default(),
    };
    for action in [
        GroupNotificationAction::Subject {
            subject: "New subject".into(), subject_owner: None, subject_owner_pn: None,
            subject_owner_username: None, subject_time: None,
        },
        GroupNotificationAction::Description { id: "description".into(), description: Some("New description".into()) },
        GroupNotificationAction::Add { participants: vec![], reason: None },
    ] {
        inbound.group_cache.lock().unwrap().insert("1@g.us".into(), GroupInfo {
            subject: Some("Old subject".into()), description: Some("Old description".into()),
            ..Default::default()
        });
        inbound.group_cache.lock().unwrap().insert("other@g.us".into(), GroupInfo::default());
        *inbound.groups_cache.lock().unwrap() = Some(vec![]);
        let update = GroupUpdate::builder()
            .group_jid("1@g.us".parse().unwrap())
            .timestamp("2026-09-27T00:00:00Z".parse().unwrap())
            .is_lid_addressing_mode(false)
            .action(Box::new(action))
            .build();
        inbound.handle(&Event::GroupUpdate(update)).await;
        assert!(!inbound.group_cache.lock().unwrap().contains_key("1@g.us"));
        assert!(inbound.group_cache.lock().unwrap().contains_key("other@g.us"));
        assert!(inbound.groups_cache.lock().unwrap().is_none());
        assert_eq!(inbound.store.name_for("1@g.us").unwrap().as_deref(), Some("New subject"));
        let mut announced = false;
        while let Ok(event) = received.try_recv() {
            if matches!(event, ServiceEvent::GroupChanged { chat } if chat == "1@g.us") {
                announced = true;
            }
        }
        assert!(announced);
    }
}

#[test]
fn learned_caller_address_forms_reach_saved_names() {
    let store = MessageStore::open(Path::new(":memory:"), Retention::unlimited()).unwrap();
    let lid: Jid = "123:4@lid".parse().unwrap();
    let pn: Jid = "59897504482:5@s.whatsapp.net".parse().unwrap();
    remember_lid_pn(&store, &lid, Some(&pn));
    store.set_saved_name("59897504482@s.whatsapp.net", "Ada").unwrap();
    let forms = contact_forms(&store, "123@lid");
    assert_eq!(forms, ["123@lid", "59897504482@s.whatsapp.net"]);
    assert_eq!(forms.iter().find_map(|jid| store.name_for(jid).unwrap()), Some("Ada".into()));
}

/// A contact the core has mapped to a phone number answers to both forms,
/// so an alias added from either place is found from the other.
#[test]
fn an_alias_reaches_a_contact_through_both_of_its_address_forms() {
    let store = MessageStore::open(Path::new(":memory:"), Retention::default()).unwrap();
    store.set_lid_pn("12345", "59891954564").unwrap();
    assert_eq!(
        contact_forms(&store, "12345@lid"),
        ["12345@lid", "59891954564@s.whatsapp.net"]
    );
    assert_eq!(
        contact_forms(&store, "59891954564@s.whatsapp.net"),
        ["59891954564@s.whatsapp.net", "12345@lid"]
    );
}

/// A contact the core has not mapped has no twin, and still gets an alias.
#[test]
fn an_unmapped_contact_has_only_the_form_it_was_given() {
    let store = MessageStore::open(Path::new(":memory:"), Retention::default()).unwrap();
    assert_eq!(
        contact_forms(&store, "59891954564@s.whatsapp.net"),
        ["59891954564@s.whatsapp.net"]
    );
}

#[test]
fn link_metadata_is_read_like_discord() {
    let html = r##"<html><head><title>Fallback &amp; title</title>
        <meta content='Darel on X' property="og:title">
        <meta name=twitter:description content="He said &quot;wild&quot; &#8212; 12 replies">
        <META PROPERTY="og:site_name" CONTENT="FixupX" />
        <meta name="theme-color" content="#1DA1F2"></head></html>"##;
    let meta = meta_tags(html);
    assert_eq!(meta.get("og:title").map(String::as_str), Some("Darel on X"));
    assert_eq!(meta.get("twitter:description").map(String::as_str), Some("He said \"wild\" — 12 replies"));
    assert_eq!(meta.get("og:site_name").map(String::as_str), Some("FixupX"));
    assert_eq!(meta.get("theme-color").map(String::as_str), Some("#1DA1F2"));
    assert_eq!(html_title(html).as_deref(), Some("Fallback & title"));
    for private in ["127.0.0.1", "192.168.1.10", "10.0.0.2", "100.64.0.1", "169.254.1.1", "::1", "fd00::1", "::ffff:10.0.0.1"] {
        assert!(!is_public_ip(private.parse().unwrap()), "{private}");
    }
    assert!(is_public_ip("1.1.1.1".parse().unwrap()));
    assert!(is_public_ip("2606:4700::1111".parse().unwrap()));
}

#[test]
fn vacuum_waits_for_a_large_freelist_and_a_week() {
    let week = 7 * 86_400;
    assert!(should_vacuum(5_000, 10_000, week));
    assert!(!should_vacuum(5_000, 10_000, week - 1));
    assert!(!should_vacuum(900, 1_000, week));
    assert!(!should_vacuum(1_500, 100_000, week));
}

#[test]
fn adaptive_settle_scales_and_is_bounded() {
    assert_eq!(adaptive_settle(0), std::time::Duration::from_millis(300));
    assert_eq!(adaptive_settle(1_000), std::time::Duration::from_millis(300));
    assert_eq!(adaptive_settle(5_000), std::time::Duration::from_millis(500));
    assert_eq!(adaptive_settle(50_000), std::time::Duration::from_millis(2_000));
    assert_eq!(adaptive_settle(1_000_000), std::time::Duration::from_millis(2_000));
}

#[test]
fn readiness_needs_a_finished_drain_and_quiescence() {
    let secs = std::time::Duration::from_secs;
    let base = SyncProgress { pending: 100, ..Default::default() };
    // Still draining: not ready, however long it has been under the cap.
    assert!(!sync_ready(&base, secs(3)));
    // Drain done and nothing new for long enough: ready.
    let done = SyncProgress { offline_done: true, ..base };
    assert!(sync_ready(&done, secs(3)));
    // The hard cap always lets the UI go.
    assert!(sync_ready(&base, secs(61)));
    // Nothing announced: treated as no backlog after the grace period.
    let empty = SyncProgress::default();
    assert!(sync_ready(&empty, secs(3)));
    assert!(!sync_ready(&empty, secs(1)));
}

#[test]
fn an_older_request_is_completed_once_by_its_session() {
    // Regression guard: "load older" used to end only on its UI timeout when
    // the phone answered with nothing older, and reported a failure that
    // never happened. The answer is matched by request session instead.
    let now = std::time::Instant::now();
    let mut waits = OlderWaits::default();
    waits.remember(now, "3EB0AAA", "chat@s");
    waits.remember(now, "3EB0BBB", "other@s");
    assert_eq!(waits.resolve("3EB0AAA").as_deref(), Some("chat@s"));
    // A second answer for the same request completes nothing.
    assert_eq!(waits.resolve("3EB0AAA"), None);
    // An answer to a request nobody waits on (a quote's recall) is ignored.
    assert_eq!(waits.resolve("3EB0CCC"), None);
    assert_eq!(waits.resolve("3EB0BBB").as_deref(), Some("other@s"));
}

#[test]
fn unanswered_older_requests_are_forgotten() {
    let now = std::time::Instant::now();
    let mut waits = OlderWaits::default();
    waits.remember(now, "3EB0AAA", "chat@s");
    // A phone that never answers must not leave requests behind forever.
    waits.remember(now + OLDER_WAIT + Duration::from_secs(1), "3EB0BBB", "other@s");
    assert_eq!(waits.resolve("3EB0AAA"), None);
    assert_eq!(waits.resolve("3EB0BBB").as_deref(), Some("other@s"));
}

#[test]
fn events_serialize_for_the_ui() {
    // Regression guard: these are emitted with `app.emit`, which fails
    // silently for a shape serde cannot represent.
    for event in [
        ServiceEvent::QrCode { code: "2@abc".into() },
        ServiceEvent::Connected,
        ServiceEvent::Disconnected,
        ServiceEvent::RetentionApplied { removed: 3 },
        ServiceEvent::NamesUpdated { count: 2 },
        ServiceEvent::Syncing { pending: 5, applied: 2 },
        ServiceEvent::InitialSyncComplete { messages: 42, chats: 7 },
        ServiceEvent::Synced,
    ] {
        let json = serde_json::to_string(&event).expect("event must serialize");
        assert!(json.contains("\"kind\""), "missing tag: {json}");
    }

    let message = ServiceEvent::Message {
        message: Box::new(StoredMessage {
            header: MessageHeader {
                chat: "a@s".into(),
                id: "1".into(),
                sender: "b@s".into(),
                timestamp: 0,
                from_me: false,
            },
            text: "hi".into(),
            media: Media { locator: Some(vec![1]), ..Default::default() },
            ..Default::default()
        }),
    };
    let json = serde_json::to_string(&message).expect("message event must serialize");
    assert!(json.contains("\"message\""), "missing payload: {json}");
    // The UI reads one flat object keyed by column names.
    for key in ["\"chat\":\"a@s\"", "\"media_kind\"", "\"media_duration\"", "\"reply_to_id\"", "\"preview_url\"", "\"status\""] {
        assert!(json.contains(key), "missing {key}: {json}");
    }
    for key in ["\"header\"", "\"media\"", "\"locator\"", "\"media_ref\""] {
        assert!(!json.contains(key), "unexpected {key}: {json}");
    }

    // Burst hints must serialize with no payload beyond routing fields.
    let hint = ServiceEvent::hint(
        &StoredMessage {
            header: MessageHeader {
                chat: "a@s".into(),
                id: "1".into(),
                sender: "b@s".into(),
                timestamp: 0,
                from_me: false,
            },
            text: "hi".into(),
            ..Default::default()
        },
        true,
    );
    let json = serde_json::to_string(&hint).expect("hint event must serialize");
    assert!(json.contains("\"kind\":\"messageHint\""), "missing tag: {json}");
    for key in ["\"chat\":\"a@s\"", "\"id\":\"1\"", "\"sender\":\"b@s\"", "\"fresh\":true"] {
        assert!(json.contains(key), "missing {key}: {json}");
    }
    assert!(!json.contains("\"text\""), "hint must not carry a payload: {json}");
    let full_len = serde_json::to_string(&message).unwrap().len();
    eprintln!("event bytes: full={full_len} hint={} ratio={:.1}x", json.len(), full_len as f64 / json.len() as f64);
}

#[test]
fn default_config_targets_its_data_dir() {
    let c = ServiceConfig::under("/tmp/example");
    assert_eq!(c.session_path, Path::new("/tmp/example").join("session.db"));
    assert_eq!(c.messages_path, Path::new("/tmp/example").join("messages.db"));
    assert_eq!(c.retention, Retention::default());
    assert!(!c.accept_full_history);
}

#[test]
fn default_retention_is_bounded() {
    // The whole point of the rewrite: the default must not be unbounded.
    let r = Retention::default();
    assert!(r.max_age_hours.is_some());
    assert!(r.max_messages_per_chat.is_some());
}

#[test]
fn secret_horizon_tracks_the_message_window() {
    // Keys must not outlive the messages they belong to, or the session
    // database grows far beyond the history we actually keep.
    let config = cache_config_for(&Retention {
        max_age_hours: Some(24),
        max_messages_per_chat: None,
    });
    let day = Duration::from_secs(24 * 3600);
    assert!(config.msg_secret_retention.text < day * 2);
    assert!(config.msg_secret_retention.poll_event < day * 2);
}

#[test]
fn secret_horizon_has_a_floor() {
    // An edit can arrive shortly after its parent, so the horizon must not
    // collapse to zero for a very short retention window.
    let config = cache_config_for(&Retention {
        max_age_hours: Some(0),
        max_messages_per_chat: None,
    });
    assert!(config.msg_secret_retention.text >= Duration::from_secs(3600));
}

#[test]
fn unlimited_retention_falls_back_to_the_library_default() {
    let config = cache_config_for(&Retention::unlimited());
    assert_eq!(
        config.msg_secret_retention.text,
        Duration::from_secs(30 * 86_400)
    );
}

#[test]
fn only_stubs_that_mean_something_become_system_rows() {
    use wa::web_message_info::StubType;
    assert_eq!(system_kind(StubType::E2E_IDENTITY_CHANGED).as_deref(), Some("E2E_IDENTITY_CHANGED"));
    assert_eq!(system_kind(StubType::GROUP_PARTICIPANT_ADD).as_deref(), Some("GROUP_PARTICIPANT_ADD"));
    for silent in [
        StubType::GENERIC_NOTIFICATION,
        StubType::REVOKE,
        StubType::PAYMENT_CIPHERTEXT,
        StubType::VERIFIED_HIGH,
        StubType::NON_VERIFIED_TRANSITION,
    ] {
        assert_eq!(system_kind(silent), None, "{silent:?}");
    }
}

#[test]
fn a_reupload_repoints_the_media_and_drops_its_old_url() {
    let mut message = wa::Message {
        image_message: MessageField::some(wa::message::ImageMessage {
            url: Some("https://mmg.whatsapp.net/old".into()),
            direct_path: Some("/v/old".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    set_direct_path(&mut message, "/v/new");
    let image = message.image_message.as_option().unwrap();
    assert_eq!(image.direct_path.as_deref(), Some("/v/new"));
    assert_eq!(image.url, None);
}

#[test]
fn a_quoted_view_once_stub_keeps_its_kind_and_carries_no_media() {
    let quoted = empty_view_once(Some("video"));
    let inner = quoted.view_once_message_v2.as_option().unwrap().message.as_option().unwrap();
    let video = inner.video_message.as_option().unwrap();
    assert_eq!(video.view_once, Some(true));
    assert_eq!(video.direct_path, None);
    assert!(empty_view_once(None).view_once_message_v2.as_option().unwrap().message.as_option().unwrap().image_message.is_set());
}

#[test]
fn view_once_media_is_nested_in_the_v2_container() {
    for message in [
        wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage::default()),
            ..Default::default()
        },
        wa::Message {
            video_message: MessageField::some(wa::message::VideoMessage::default()),
            ..Default::default()
        },
        wa::Message {
            audio_message: MessageField::some(wa::message::AudioMessage::default()),
            ..Default::default()
        },
    ] {
        let wrapped = wrap_view_once(message);
        let outer = wrapped.view_once_message_v2.as_option().expect("v2 wrapper");
        let inner = outer.message.as_option().expect("wrapped message");
        assert!(
            inner.image_message.is_set() || inner.video_message.is_set() || inner.audio_message.is_set()
        );
    }
}

/// A state change addressed by LID resolves to the phone-number key the chat
/// and its messages are stored under, so archive/pin/mute land on the right row.
#[tokio::test]
async fn a_lid_state_change_lands_on_the_phone_number_row() {
    let store = MessageStore::open(Path::new(":memory:"), Retention::default()).unwrap();
    store.set_lid_pn("12345", "59891954564").unwrap();

    let lid: Jid = "12345@lid".parse().unwrap();
    assert_eq!(
        resolve_chat(None, &store, &lid).await,
        "59891954564@s.whatsapp.net"
    );

    // A number is already canonical, and a group passes through untouched.
    let pn: Jid = "59891954564@s.whatsapp.net".parse().unwrap();
    assert_eq!(resolve_chat(None, &store, &pn).await, "59891954564@s.whatsapp.net");
    let group: Jid = "120363000000000042@g.us".parse().unwrap();
    assert_eq!(resolve_chat(None, &store, &group).await, "120363000000000042@g.us");
}
