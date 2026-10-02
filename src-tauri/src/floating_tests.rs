use super::*;

#[test]
fn floating_targets_and_text_reject_cross_surface_identifiers_and_invalid_payloads() {
    for chat in ["1@s.whatsapp.net", "2@lid", "123-456@g.us", "3@newsletter", "4@broadcast"] { assert!(chat_target(chat).is_ok()); }
    for chat in ["", "../account", "1:2@s.whatsapp.net", "status@broadcast", "1@localhost", "@g.us", "a@lid", "1@g.us@other", "-@g.us", "1--2@g.us"] {
        assert!(chat_target(chat).is_err());
    }
    assert!(checked_text("hello\nworld").is_ok());
    for text in [" ".into(), "a\0b".into(), "x".repeat(65537)] { assert!(checked_text(&text).is_err()); }
    assert_eq!(title("\r\n", "1@lid"), "1@lid"); assert_eq!(title("Real\nName", "fallback"), "RealName");
    assert_eq!(title(&"x".repeat(200), "fallback").len(), 120);
}

fn entry(account: &str, chat: &str) -> Entry {
    Entry { binding: Arc::new(Binding { account_id: account.into(), chat: chat.into(), title: chat.into(), service: Weak::new(),
        sending: tokio::sync::Mutex::new(()) }), updates: None }
}

#[test]
fn duplicate_pair_focus_is_allowed_at_limit_without_mixing_account_or_chat() {
    let mut entries = HashMap::new();
    for index in 0..MAX_WINDOWS { entries.insert(format!("postal-float-{index}"), entry("a", &format!("{index}@lid"))); }
    assert_eq!(existing_window(&entries, "a", "0@lid").unwrap().as_deref(), Some("postal-float-0"));
    assert!(existing_window(&entries, "b", "0@lid").is_err());
    assert!(existing_window(&entries, "a", "other@lid").is_err());
    entries.remove("postal-float-7");
    assert!(existing_window(&entries, "b", "0@lid").unwrap().is_none());
}

#[test]
fn invalidation_removes_authority_for_all_windows_before_native_close_attempts() {
    let registry = FloatingChats::default();
    registry.entries.lock().unwrap().insert("postal-float-a".into(), entry("a", "1@lid"));
    registry.entries.lock().unwrap().insert("postal-float-b".into(), entry("a", "2@lid"));
    let mut labels = drain_bindings(&registry); labels.sort();
    assert_eq!(labels, ["postal-float-a", "postal-float-b"]);
    assert!(registry.entries.lock().unwrap().is_empty());
    assert!(drain_bindings(&registry).is_empty());
}

#[test]
fn service_binding_rejects_replacement_and_shutdown_even_with_same_account_identity() {
    let original = Arc::new(()); let other = Arc::new(()); let weak = Arc::downgrade(&original);
    assert!(same_service(&weak, &original)); assert!(!same_service(&weak, &other));
    drop(original); assert!(!same_service(&weak, &other));
}

#[test]
fn float_pages_remove_media_and_quote_authority_but_preserve_readable_rows() {
    let mut page = postal_core::store::MessagePage { has_more: true, messages: vec![postal_core::StoredMessage {
        text: "cached message".into(), media: postal_core::store::Media { path: Some("C:\\account\\file.mp4".into()),
            thumb: Some("/private/thumb.png".into()), locator: Some(vec![1]), ..Default::default() },
        quote: postal_core::store::Quote { path: Some("/private/quote.png".into()), thumb: Some("file:///private/quote.jpg".into()),
            text: Some("quoted text".into()), locator: Some(vec![2]), ..Default::default() },
        link: postal_core::store::LinkCard { thumb: Some("asset://private/avatar".into()), ..Default::default() }, ..Default::default()
    }] };
    strip_paths(&mut page); let message = &page.messages[0];
    assert_eq!(message.text, "cached message"); assert_eq!(message.quote.text.as_deref(), Some("quoted text"));
    assert!(page.has_more); assert!(message.media.path.is_none() && message.media.thumb.is_none() && message.media.locator.is_none());
    assert!(message.quote.path.is_none() && message.quote.thumb.is_none() && message.quote.locator.is_none());
    assert!(message.link.thumb.is_none());
}

#[test]
fn notifications_match_bound_chat_and_never_forward_other_chat_pairing_or_upload_payloads() {
    assert!(relevant(&ServiceEvent::Marks { chat: "1@lid".into() }, "1@lid"));
    assert!(!relevant(&ServiceEvent::Marks { chat: "2@lid".into() }, "1@lid"));
    assert!(relevant(&ServiceEvent::Disconnected, "1@lid"));
    assert!(relevant(&ServiceEvent::HistoryLoaded { chats: vec!["1@lid".into()] }, "1@lid"));
    assert!(!relevant(&ServiceEvent::HistoryLoaded { chats: vec!["2@lid".into()] }, "1@lid"));
    assert!(!relevant(&ServiceEvent::QrCode { code: "private-pairing".into() }, "1@lid"));
    assert!(!relevant(&ServiceEvent::UploadProgress { token: "other-upload".into(), sent: 1, total: 2 }, "1@lid"));
}
