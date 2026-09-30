use super::*;
use crate::store::{EditRevision, PollEdit, PollOption, SecretEdit};
use whatsapp_rust::wacore::poll::compute_option_hash;

#[test]
fn secret_edits_archive_preserves_hashes_revisions_and_rejects_forged_shareability() {
    let root = std::env::temp_dir().join(format!(
        "postal-secret-edit-archive-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store
        .insert_message(&StoredMessage {
            header: MessageHeader {
                chat: "1@g.us".into(),
                id: "poll".into(),
                sender: "100@s.whatsapp.net".into(),
                timestamp: 10,
                ..Default::default()
            },
            text: "Question".into(),
            media: Media {
                kind: Some("poll".into()),
                ..Default::default()
            },
            history_shareable: true,
            ..Default::default()
        })
        .unwrap();
    let options = vec![
        PollOption {
            name: "Yes".into(),
            hash: compute_option_hash("Yes"),
        },
        PollOption {
            name: "No".into(),
            hash: compute_option_hash("No"),
        },
    ];
    store
        .save_poll(
            "1@g.us",
            "poll",
            "100@s.whatsapp.net",
            "Question",
            &["Yes".into(), "No".into()],
            false,
            Some(&[7; 32]),
        )
        .unwrap();
    store.remember_poll_options("1@g.us", "poll", &options, true).unwrap();
    store
        .set_poll_vote("1@g.us", "poll", "200@s.whatsapp.net", &["Yes".into()])
        .unwrap();
    let renamed = vec![
        PollOption {
            name: "Absolutely".into(),
            hash: options[0].hash,
        },
        options[1].clone(),
    ];
    let editor = vec!["100@s.whatsapp.net".into()];
    assert!(store
        .apply_secret_edit(
            "1@g.us",
            "poll",
            &editor,
            &editor,
            &SecretEdit::Poll(PollEdit {
                name: Some("Changed question".into()),
                options: renamed,
                selectable: None,
            }),
            &EditRevision {
                timestamp_ms: 20_000,
                message_id: "edit".into()
            }
        )
        .unwrap());
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    let backup = root.join("backup");
    store.export_backup(&backup, &media, &[]).unwrap();
    let forged = Connection::open(backup.join("messages.db")).unwrap();
    forged.execute("UPDATE messages SET history_shareable = 1", []).unwrap();
    drop(forged);
    let account = root.join("restored");
    let restored_media = root.join("restored-media");
    restore_backup(&backup, &account, &restored_media).unwrap();
    let restored = MessageStore::open(&account.join("messages.db")).unwrap();
    assert!(!restored.message("1@g.us", "poll").unwrap().history_shareable);
    assert_eq!(
        restored.poll_option_hashes("1@g.us", "poll", &["Absolutely".into()]).unwrap(),
        vec![options[0].hash.to_vec()]
    );
    assert_eq!(restored.poll_secret("1@g.us", "poll").unwrap().unwrap().secret, vec![7; 32]);
    assert_eq!(restored.marks("1@g.us").unwrap().polls[0].votes[0].options, ["Absolutely"]);
    let restored_revision: (i64, String) = restored
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT timestamp_ms, message_id FROM secret_edit_revisions WHERE chat = '1@g.us' AND id = 'poll'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(restored_revision, (20_000, "edit".into()));
    let conversation = root.join("conversation");
    store.export_conversation("1@g.us", &conversation, &media).unwrap();
    let json: serde_json::Value = read_json(&conversation.join("conversation.json"), 1024 * 1024).unwrap();
    assert_eq!(json["version"], 2);
    assert_eq!(json["pages"][0]["poll_metadata"][0]["options"][0]["name"], "Absolutely");
    assert_eq!(
        json["pages"][0]["poll_metadata"][0]["options"][0]["hash"],
        serde_json::json!(options[0].hash)
    );
    assert_eq!(json["pages"][0]["poll_metadata"][0]["allow_add_option"], true);
    assert_eq!(json["pages"][0]["edit_revisions"][0]["timestamp_ms"], 20_000);
    assert_eq!(store.clear_chat("1@g.us").unwrap(), 1);
    assert_eq!(restored.clear_history().unwrap(), 1);
    for database in [&store, &restored] {
        for table in ["poll_option_hashes", "secret_edit_revisions"] {
            assert_eq!(
                database
                    .conn
                    .lock()
                    .unwrap()
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
    drop(restored);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}
