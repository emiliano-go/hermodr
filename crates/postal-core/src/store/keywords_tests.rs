use super::*;

#[test]
fn keyword_limits_report_typed_codes_and_exact_scalar_bounds() {
    for (input, code, max, actual) in [
        (vec!["word".to_owned(); 51], "error.keyword_list_limit", 50, 51),
        (vec!["é".repeat(101)], "error.keyword_length_limit", 100, 101),
    ] {
        let error = terms(&input).unwrap_err();
        let message = error.downcast_ref::<MessageRef>().unwrap();
        assert_eq!(message.code, code);
        assert_eq!(serde_json::to_value(&message.params).unwrap(), serde_json::json!({ "max": max, "actual": actual }));
    }
}

fn row(chat: &str, id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        header: MessageHeader {
            chat: chat.into(),
            id: id.into(),
            sender: "synthetic@s.whatsapp.net".into(),
            timestamp: 1,
            from_me: false,
        },
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn keyword_counts_are_readonly_visible_unread_incoming_once_per_message() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store
        .insert_message(&row("visible@g.us", "ordinary", "NEEDLE needle"))
        .unwrap();
    let mut caption = row("visible@g.us", "caption", "A Needle caption");
    caption.media.kind = Some("image".into());
    store.insert_message(&caption).unwrap();
    let mutations: [fn(&mut StoredMessage); 11] = [
        |m: &mut StoredMessage| m.header.from_me = true,
        |m: &mut StoredMessage| m.local.read = true,
        |m: &mut StoredMessage| m.local.mentioned = true,
        |m: &mut StoredMessage| m.local.deleted = true,
        |m: &mut StoredMessage| m.local.revoked = true,
        |m: &mut StoredMessage| m.spoiler = true,
        |m: &mut StoredMessage| m.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
        |m: &mut StoredMessage| m.system.kind = Some("SYSTEM".into()),
        |m: &mut StoredMessage| m.media.kind = Some("view_once".into()),
        |m: &mut StoredMessage| m.media.kind = Some("unknown".into()),
        |m: &mut StoredMessage| m.media.once_kind = Some("image".into()),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut guarded = row("visible@g.us", &format!("excluded-{index}"), "NEEDLE");
        mutate(&mut guarded);
        store.insert_message(&guarded).unwrap();
    }
    store
        .insert_message(&row("visible@g.us", "hidden-word", "Needle SECRET"))
        .unwrap();
    store
        .insert_message(&row("hidden@g.us", "hidden-chat", "Needle"))
        .unwrap();
    let mut placeholder = row("visible@g.us", "placeholder", "\u{FEFF}[image]");
    placeholder.media.kind = Some("image".into());
    store.insert_message(&placeholder).unwrap();
    let mut quoted = row("visible@g.us", "quote-only", "Plain body");
    quoted.quote.text = Some("Needle".into());
    store.insert_message(&quoted).unwrap();
    store
        .insert_message(&row("visible@g.us", "once-table", "Needle"))
        .unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO view_once(chat, id) VALUES ('visible@g.us', 'once-table')",
            [],
        )
        .unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute("INSERT INTO hidden_chats(jid) VALUES ('hidden@g.us')", [])
        .unwrap();
    let before: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let counts = store
        .keyword_mentions(&["needle".into(), "image".into()], &["secret".into()])
        .unwrap();
    assert_eq!(
        counts,
        std::collections::HashMap::from([("visible@g.us".into(), 2)])
    );
    let navigation = store
        .keyword_matches(
            Some("visible@g.us"),
            false,
            &["needle".into(), "image".into()],
            &["secret".into()],
        )
        .unwrap();
    assert_eq!(
        navigation
            .iter()
            .map(|m| m.header.id.as_str())
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from(["ordinary", "caption", "excluded-1", "excluded-2"])
    );
    assert_eq!(
        store
            .keyword_matches(
                Some("visible@g.us"),
                true,
                &["needle".into(), "image".into()],
                &["secret".into()]
            )
            .unwrap()
            .len(),
        3
    );
    let after: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert!(
        !store
            .message("visible@g.us", "ordinary")
            .unwrap()
            .local
            .read
    );
    assert!(
        store
            .message("visible@g.us", "excluded-2")
            .unwrap()
            .local
            .mentioned
    );
}

#[test]
fn keyword_unicode_literals_bounds_and_empty_rules_use_no_database_query() {
    for (body, term, expected) in [
        ("CAFÉ", "café", true),
        ("ΟΣ", "ος", true),
        ("İ", "i\u{0307}", true),
        ("STRASSE", "straße", false),
        ("A.*B", "a.*b", true),
        ("AxB", "a.*b", false),
        ("concatenate", "cat", true),
    ] {
        assert_eq!(
            matches(&body.to_lowercase(), &terms(&[term.into()]).unwrap()),
            expected,
            "{body}/{term}"
        );
    }
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store
        .insert_message(&row("unicode@g.us", "unicode", "CAFÉ ΟΣ İ A.*B Straße"))
        .unwrap();
    store
        .insert_message(&row("other@g.us", "literal-miss", "AxB STRASSE"))
        .unwrap();
    assert_eq!(
        store
            .keyword_mentions(
                &[
                    "café".into(),
                    "ος".into(),
                    "i\u{0307}".into(),
                    "a.*b".into()
                ],
                &[]
            )
            .unwrap(),
        std::collections::HashMap::from([("unicode@g.us".into(), 1)])
    );
    assert_eq!(
        store.keyword_mentions(&["straße".into()], &[]).unwrap(),
        std::collections::HashMap::from([("unicode@g.us".into(), 1)])
    );
    assert_eq!(
        terms(&[
            "\u{FEFF}Needle\u{FEFF}".into(),
            "needle".into(),
            "\u{0085}needle\u{0085}".into()
        ])
        .unwrap(),
        ["needle", "\u{0085}needle\u{0085}"]
    );
    assert!(terms(&["🦀".repeat(100)]).is_ok());
    assert!(terms(&["🦀".repeat(101)]).is_err());
    assert!(terms(&vec!["same".into(); 51]).is_err());
    store
        .conn
        .lock()
        .unwrap()
        .execute_batch("DROP TABLE messages")
        .unwrap();
    assert!(store.keyword_mentions(&[], &[]).unwrap().is_empty());
    assert!(
        store
            .keyword_matches(None, false, &[], &[])
            .unwrap()
            .is_empty()
    );
    assert!(store.keyword_mentions(&["needle".into()], &[]).is_err());
}

#[test]
fn keyword_navigation_is_readonly_scoped_newest_bounded_and_keeps_real_mentions_for_dedup() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for index in 0..530 {
        let mut message = row(
            if index < 510 {
                "scope-a@g.us"
            } else {
                "scope-b@g.us"
            },
            &format!("{index:04}"),
            "Needle",
        );
        message.header.timestamp = index;
        message.local.mentioned = index % 3 == 0;
        message.local.read = index % 7 == 0;
        store.insert_message(&message).unwrap();
    }
    for (id, spoiler, text) in [
        ("spoiler", true, "Needle"),
        ("hidden-word", false, "Needle SECRET"),
    ] {
        let mut guarded = row("scope-a@g.us", id, text);
        guarded.spoiler = spoiler;
        guarded.header.timestamp = 1000;
        store.insert_message(&guarded).unwrap();
    }
    let before: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let highlight = ["needle".into()];
    let hide = ["secret".into()];
    let all = store
        .keyword_matches(None, false, &highlight, &hide)
        .unwrap();
    assert_eq!(all.len(), 500);
    assert_eq!(all[0].header.id, "0529");
    assert_eq!(all[499].header.id, "0030");
    assert_eq!(
        all.iter()
            .map(|m| (&m.header.chat, &m.header.id))
            .collect::<std::collections::HashSet<_>>()
            .len(),
        500
    );
    assert!(all.iter().any(|m| m.local.mentioned));
    assert!(all.iter().any(|m| m.local.read));
    let scoped = store
        .keyword_matches(Some("scope-b@g.us"), true, &highlight, &hide)
        .unwrap();
    assert!(
        scoped
            .iter()
            .all(|m| m.header.chat == "scope-b@g.us" && !m.local.read)
    );
    assert!(
        scoped
            .iter()
            .any(|m| m.header.id == "0528" && m.local.mentioned)
    );
    assert!(all.iter().all(|m| !m.spoiler && !m.text.contains("SECRET")));
    let after: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
}
