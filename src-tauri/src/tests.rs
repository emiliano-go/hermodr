use crate::{account_store::{is_stale_session, Account, AccountsFile, DEFAULT_ACCOUNT_LABEL}, migration::move_dir};

#[test]
fn stale_sessions_spare_the_active_wal() {
    assert!(!is_stale_session("session.db", "session.db"));
    assert!(!is_stale_session("session.db-wal", "session.db"));
    assert!(!is_stale_session("session.db-shm", "session.db"));
    assert!(is_stale_session("session.db-wal", "session-1.db"));
    assert!(is_stale_session("session-1.db", "session-2.db"));
    assert!(is_stale_session("session-1.db-shm", "session-2.db"));
    assert!(!is_stale_session("session-2.db-wal", "session-2.db"));
    assert!(!is_stale_session("session.dbx", "session-2.db"));
    assert!(!is_stale_session("messages.db", "session.db"));
}

fn label_of<'a>(file: &'a AccountsFile, id: &str) -> &'a str {
    &file
        .accounts
        .iter()
        .find(|a| a.id == id)
        .expect("account")
        .label
}

#[test]
fn an_untouched_account_takes_its_profile_name_once() {
    let mut file = AccountsFile {
        accounts: vec![
            Account {
                id: "default".into(),
                label: DEFAULT_ACCOUNT_LABEL.into(),
                jid: None,
            },
            Account {
                id: "acct-1".into(),
                label: "Work phone".into(),
                jid: None,
            },
        ],
        active: Some("default".into()),
    };
    assert!(file.seed_label("default", "Ada Lovelace"));
    assert_eq!(label_of(&file, "default"), "Ada Lovelace");
    // Seeding again is a no-op: the label is the account's own now.
    assert!(!file.seed_label("default", "Ada Byron"));
    assert_eq!(label_of(&file, "default"), "Ada Lovelace");
}

#[test]
fn a_name_the_user_chose_beats_the_profile_name() {
    let mut file = AccountsFile {
        accounts: vec![Account {
            id: "acct-1".into(),
            label: "Work phone".into(),
            jid: None,
        }],
        active: Some("acct-1".into()),
    };
    assert!(!file.seed_label("acct-1", "Grace Hopper"));
    assert_eq!(label_of(&file, "acct-1"), "Work phone");
}

#[test]
fn an_unknown_profile_name_leaves_the_label_alone() {
    let mut file = AccountsFile {
        accounts: vec![Account {
            id: "default".into(),
            label: DEFAULT_ACCOUNT_LABEL.into(),
            jid: None,
        }],
        active: Some("default".into()),
    };
    assert!(!file.seed_label("default", ""));
    assert!(!file.seed_label("default", "   "));
    assert_eq!(label_of(&file, "default"), DEFAULT_ACCOUNT_LABEL);
    assert!(!file.seed_label("nope", "Ada Lovelace"));
}

#[test]
fn an_empty_label_is_seeded_like_the_default_one() {
    let mut file = AccountsFile {
        accounts: vec![Account {
            id: "default".into(),
            label: String::new(),
            jid: None,
        }],
        active: Some("default".into()),
    };
    assert!(file.seed_label("default", "  Ada Lovelace  "));
    assert_eq!(label_of(&file, "default"), "Ada Lovelace");
}

#[test]
fn migration_adopts_the_old_directory_once() {
    let root = std::env::temp_dir().join(format!("postal-migrate-{}", std::process::id()));
    let from = root.join("old");
    let to = root.join("new");
    std::fs::create_dir_all(from.join("accounts")).unwrap();
    std::fs::write(from.join("session.db"), b"session").unwrap();
    std::fs::write(from.join("accounts").join("a.db"), b"account").unwrap();

    move_dir(&from, &to);
    assert!(!from.exists());
    assert_eq!(std::fs::read(to.join("session.db")).unwrap(), b"session");
    assert_eq!(std::fs::read(to.join("accounts").join("a.db")).unwrap(), b"account");

    // A second run must not touch a new directory that already exists.
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("stale"), b"stale").unwrap();
    move_dir(&from, &to);
    assert!(from.join("stale").exists());
    assert!(!to.join("stale").exists());

    std::fs::remove_dir_all(root).unwrap();
}
