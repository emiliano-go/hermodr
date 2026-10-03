use super::*;

const MEMBER: &str = "598999001@s.whatsapp.net";
const LID: &str = "77@lid";
const OWN: &str = "598999002@s.whatsapp.net";
const GROUP: &str = "120363000000000001@g.us";

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store.set_lid_pn("77", "598999001").unwrap();
    store
}

fn message(chat: &str, id: &str, sender: &str, timestamp: i64) -> StoredMessage {
    StoredMessage {
        header: MessageHeader {
            chat: chat.into(),
            id: id.into(),
            sender: sender.into(),
            timestamp,
            from_me: sender == OWN,
        },
        text: "synthetic".into(),
        ..Default::default()
    }
}

fn roster() -> Vec<MemberRosterEntry> {
    vec![
        MemberRosterEntry {
            jid: MEMBER.into(),
            admin: false,
            owner: false,
            label: Some("Helper".into()),
        },
        MemberRosterEntry {
            jid: OWN.into(),
            admin: true,
            owner: false,
            label: None,
        },
    ]
}

#[test]
fn local_profile_counts_aliases_scope_and_observed_mentions_without_guessing_text() {
    let store = store();
    store
        .set_contact_state(MEMBER, Some("Saved"), true, 1)
        .unwrap();
    store.set_push_name(LID, "Push").unwrap();
    store.set_username(LID, "member").unwrap();
    let mut image = message(GROUP, "one", MEMBER, 10);
    image.media.kind = Some("image".into());
    store.insert_message(&image).unwrap();
    let mut text = message(GROUP, "two", LID, 20);
    text.text = "@598999001 is text, not mention evidence".into();
    store.insert_message(&text).unwrap();
    store
        .insert_message(&message(MEMBER, "dm", MEMBER, 30))
        .unwrap();
    store
        .insert_message(&message(GROUP, "mention", OWN, 25))
        .unwrap();
    let mut join = message(GROUP, "join", OWN, 5);
    join.system.kind = Some("GROUP_PARTICIPANT_ADD".into());
    join.system.params = vec![LID.into()];
    store.insert_message(&join).unwrap();
    store.record_member_mentions(GROUP, "one", &[]).unwrap();
    store
        .record_member_mentions(GROUP, "mention", &[LID.into(), MEMBER.into()])
        .unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO reactions(chat,target,sender,emoji) VALUES(?1,'mention',?2,'👍')",
            params![GROUP, LID],
        )
        .unwrap();
    let local = store
        .member_profile_local(LID, Some(GROUP), OWN, 100)
        .unwrap();
    assert_eq!(
        (
            local.stats.total,
            local.stats.media_total,
            local.stats.first_at,
            local.stats.last_at
        ),
        (2, 1, Some(10), Some(20))
    );
    assert_eq!(
        (
            local.stats.reactions_sent,
            local.stats.times_mentioned,
            local.stats.mention_contexts_recorded
        ),
        (1, 1, 2)
    );
    assert_eq!(local.identity.saved_name.as_deref(), Some("Saved"));
    assert_eq!(local.identity.push_name.as_deref(), Some("Push"));
    assert_eq!(local.identity.username.as_deref(), Some("member"));
    assert_eq!(local.pn_jid.as_deref(), Some(MEMBER));
    assert_eq!(local.lid_jid.as_deref(), Some(LID));
    assert_eq!(local.join.unwrap().actor.as_deref(), Some(OWN));
    assert_eq!(
        store
            .member_profile_local(MEMBER, None, OWN, 100)
            .unwrap()
            .stats
            .total,
        3
    );
    assert!(store
        .member_profile_local("77:3@lid", None, OWN, 100)
        .is_err());
    let independent = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&independent.conn.lock().unwrap()).unwrap();
    assert_eq!(
        independent
            .member_profile_local(MEMBER, None, OWN, 100)
            .unwrap()
            .stats
            .total,
        0
    );
}

#[test]
fn notes_are_durable_local_and_alias_aware() {
    let path = std::env::temp_dir().join(format!(
        "postal-member-profile-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = MessageStore::open(&path).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        store.set_lid_pn("77", "598999001").unwrap();
        store
            .set_member_note(LID, Some(GROUP), "  Exact local note  ", 3, 100)
            .unwrap();
        store
            .set_member_note(MEMBER, None, "Global", 1, 101)
            .unwrap();
        assert!(store
            .set_member_note(MEMBER, None, "bad\0note", 0, 102)
            .is_err());
        assert!(store
            .set_member_note(MEMBER, None, "valid", 100001, 102)
            .is_err());
    }
    {
        let store = MessageStore::open(&path).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        let local = store
            .member_profile_local(MEMBER, Some(GROUP), OWN, 200)
            .unwrap();
        assert_eq!(
            (
                local.note.text.as_str(),
                local.note.warnings,
                local.note.updated_at
            ),
            ("  Exact local note  ", 3, Some(100))
        );
        assert_eq!(
            store
                .member_profile_local(LID, None, OWN, 200)
                .unwrap()
                .note
                .text,
            "Global"
        );
        assert!(store.chats().unwrap().is_empty());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn cached_rosters_preserve_order_roles_and_unknown_membership() {
    let store = store();
    store
        .record_group_profile_snapshot(GROUP, Some("Group"), &roster(), true, 100)
        .unwrap();
    store
        .record_member_change(GROUP, LID, "promote", None, 110)
        .unwrap();
    store
        .record_group_profile_snapshot(GROUP, Some("Old"), &roster(), false, 105)
        .unwrap();
    let local = store
        .member_profile_local(MEMBER, Some(GROUP), OWN, 111)
        .unwrap();
    let group = local.group.unwrap();
    assert_eq!(
        (
            group.present,
            group.admin,
            group.owner,
            group.label.as_deref()
        ),
        (Some(true), Some(true), Some(false), Some("Helper"))
    );
    assert_eq!(local.mutual_groups.len(), 1);
    store
        .record_member_change(GROUP, MEMBER, "remove", None, 120)
        .unwrap();
    store
        .record_member_change(GROUP, MEMBER, "add", None, 119)
        .unwrap();
    let local = store
        .member_profile_local(LID, Some(GROUP), OWN, 121)
        .unwrap();
    assert_eq!(local.group.unwrap().present, Some(false));
    assert!(local.mutual_groups.is_empty());
    store
        .record_member_change(GROUP, MEMBER, "add", None, 122)
        .unwrap();
    let group = store
        .member_profile_local(LID, Some(GROUP), OWN, 123)
        .unwrap()
        .group
        .unwrap();
    assert_eq!(
        (group.present, group.admin, group.owner),
        (Some(true), None, None)
    );
    let other = "120363000000000002@g.us";
    store
        .record_member_change(other, OWN, "add", None, 130)
        .unwrap();
    assert_eq!(
        store
            .member_profile_local(MEMBER, Some(other), OWN, 130)
            .unwrap()
            .group
            .unwrap()
            .present,
        None
    );
    store
        .record_member_change(other, MEMBER, "label", Some("Observed"), 131)
        .unwrap();
    let tag = store
        .member_profile_local(MEMBER, Some(other), OWN, 132)
        .unwrap()
        .group
        .unwrap();
    assert_eq!(
        (tag.present, tag.admin, tag.owner, tag.label.as_deref()),
        (Some(true), None, None, Some("Observed"))
    );
    assert!(store
        .record_member_change(GROUP, MEMBER, "invented", None, 1000)
        .is_err());
    assert_eq!(
        store
            .member_profile_local(MEMBER, Some(GROUP), OWN, 130)
            .unwrap()
            .group
            .unwrap()
            .observed_at,
        122
    );
}

#[test]
fn own_reactions_use_me_marker_and_live_signals_expire() {
    let store = store();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO reactions(chat,target,sender,emoji) VALUES(?1,'target','@me','👍')",
            [GROUP],
        )
        .unwrap();
    store
        .record_member_signal(MEMBER, None, Some(true), Some(90), None, 100)
        .unwrap();
    store
        .record_member_signal(LID, Some(GROUP), None, None, Some("typing"), 100)
        .unwrap();
    store
        .record_member_signal(LID, Some(GROUP), None, None, Some("paused"), 99)
        .unwrap();
    let signals = store
        .member_profile_local(LID, Some(GROUP), OWN, 110)
        .unwrap()
        .signals;
    assert_eq!(
        (signals.online, signals.last_seen, signals.typing.as_deref()),
        (Some(true), Some(90), Some("typing"))
    );
    let expired = store
        .member_profile_local(LID, Some(GROUP), OWN, 131)
        .unwrap()
        .signals;
    assert_eq!(
        (expired.online, expired.last_seen, expired.typing),
        (None, None, None)
    );
    assert_eq!(
        (expired.presence_at, expired.typing_at),
        (Some(100), Some(100))
    );
    assert_eq!(
        store
            .member_profile_local(OWN, Some(GROUP), OWN, 131)
            .unwrap()
            .stats
            .reactions_sent,
        1
    );
    assert_eq!(
        store
            .member_profile_local(MEMBER, Some(GROUP), OWN, 131)
            .unwrap()
            .stats
            .reactions_sent,
        0
    );
    assert!(store
        .record_member_signal(MEMBER, Some(GROUP), None, None, Some("guessed"), 140)
        .is_err());
}

#[test]
fn live_cache_is_alias_aware_monotonic_and_bounded() {
    let store = store();
    store.cache_member_live(MEMBER, "new", 100).unwrap();
    store.cache_member_live(MEMBER, "old", 99).unwrap();
    assert_eq!(
        store.member_live_cache(LID).unwrap(),
        Some(("new".into(), 100))
    );
    assert!(store
        .cache_member_live(MEMBER, &"x".repeat(65537), 101)
        .is_err());
}

#[test]
fn private_rows_never_contribute_stats_mentions_or_reactions() {
    let store = store();
    for (id, kind) in [
        ("once", "view_once"),
        ("unknown", "unknown"),
        ("spoiler", "image"),
        ("unavailable", "image"),
        ("revoked", "image"),
        ("deleted", "image"),
    ] {
        let mut private = message(GROUP, id, MEMBER, 10);
        private.media.kind = Some(kind.into());
        private.spoiler = id == "spoiler";
        private.local.revoked = id == "revoked";
        private.local.deleted = id == "deleted";
        if id == "unavailable" {
            private.system.kind = Some("UNAVAILABLE_MESSAGE".into());
        }
        store.insert_message(&private).unwrap();
        store
            .record_member_mentions(GROUP, id, &[MEMBER.into()])
            .unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO reactions(chat,target,sender,emoji) VALUES(?1,?2,?3,'👍')",
                params![GROUP, id, MEMBER],
            )
            .unwrap();
    }
    let stats = store
        .member_profile_local(MEMBER, Some(GROUP), OWN, 20)
        .unwrap()
        .stats;
    assert_eq!(
        (
            stats.total,
            stats.media_total,
            stats.reactions_sent,
            stats.times_mentioned,
            stats.mention_contexts_recorded
        ),
        (0, 0, 0, 0, 0)
    );
    let recorded: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM member_mention_contexts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(recorded, 0);
}

#[test]
fn alias_merge_and_context_cleanup_preserve_only_current_local_evidence() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
        .insert_message(&message(LID, "context", LID, 10))
        .unwrap();
    store
        .set_member_note(LID, Some(LID), "new", 2, 101)
        .unwrap();
    store
        .set_member_note(MEMBER, Some(MEMBER), "old", 1, 100)
        .unwrap();
    store
        .record_member_mentions(LID, "context", &[LID.into()])
        .unwrap();
    store
        .record_member_group_mentions(LID, "context", &[GROUP.into()])
        .unwrap();
    store
        .record_member_signal(LID, None, Some(true), None, None, 110)
        .unwrap();
    store
        .record_member_signal(MEMBER, None, Some(false), None, None, 100)
        .unwrap();
    store.cache_member_live(LID, "new", 101).unwrap();
    store.cache_member_live(MEMBER, "old", 100).unwrap();
    {
        let conn = store.conn.lock().unwrap();
        merge(&conn, LID, MEMBER).unwrap();
        merge(&conn, LID, MEMBER).unwrap();
    }
    store.set_lid_pn("77", "598999001").unwrap();
    let local = store
        .member_profile_local(MEMBER, Some(MEMBER), OWN, 111)
        .unwrap();
    assert_eq!((local.note.text.as_str(), local.note.warnings), ("new", 2));
    assert_eq!(local.signals.online, Some(true));
    assert_eq!(local.stats.times_mentioned, 1);
    assert_eq!(
        store.member_live_cache(MEMBER).unwrap(),
        Some(("new".into(), 101))
    );
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE messages SET deleted=1 WHERE chat=?1 AND id='context'",
            [MEMBER],
        )
        .unwrap();
    store
        .record_member_mentions(MEMBER, "context", &[MEMBER.into()])
        .unwrap();
    let local = store
        .member_profile_local(MEMBER, Some(MEMBER), OWN, 111)
        .unwrap();
    assert_eq!(
        (
            local.stats.total,
            local.stats.times_mentioned,
            local.stats.mention_contexts_recorded
        ),
        (0, 0, 0)
    );
    for table in [
        "member_mention_contexts",
        "member_mentions",
        "member_mention_groups",
    ] {
        let count: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
    store
        .insert_message(&message(GROUP, "delete", MEMBER, 10))
        .unwrap();
    store
        .record_member_mentions(GROUP, "delete", &[MEMBER.into()])
        .unwrap();
    store
        .record_member_group_mentions(GROUP, "delete", &[GROUP.into()])
        .unwrap();
    assert_eq!(
        store
            .member_profile_local(MEMBER, Some(GROUP), OWN, 111)
            .unwrap()
            .stats
            .group_mention_contexts_recorded,
        1
    );
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM messages WHERE chat=?1 AND id='delete'",
            [GROUP],
        )
        .unwrap();
    assert_eq!(
        store
            .member_profile_local(MEMBER, Some(GROUP), OWN, 111)
            .unwrap()
            .stats
            .group_mention_contexts_recorded,
        0
    );
}

#[test]
fn member_validation_retains_typed_errors_before_any_write() {
    for (result, code) in [
        (member_address("invalid"), "error.member_address"),
        (member_address("1@g.us"), "error.member_address"),
        (profile_scope(Some("invalid")), "error.member_scope"),
    ] {
        assert_eq!(result.unwrap_err().downcast_ref::<MessageRef>().unwrap().code, code);
    }
    let store = store();
    let error = store.set_member_note(MEMBER, Some(GROUP), "note", 100001, 1).unwrap_err();
    assert_eq!(error.downcast_ref::<MessageRef>().unwrap().code, "error.member_note_limit");
    assert_eq!(store.member_profile_local(MEMBER, Some(GROUP), OWN, 1).unwrap().note.warnings, 0);
}
