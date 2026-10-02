use crate::{account_store::{is_stale_session, Account, AccountsFile, DEFAULT_ACCOUNT_LABEL}, migration::move_dir};

fn keep(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn ipc_commands_match_build_and_capabilities() {
    use std::collections::BTreeSet;
    let build = include_str!("../build.rs").split("];").next().unwrap();
    let commands: BTreeSet<_> = build.split('"').skip(1).step_by(2).collect();
    let source = include_str!("lib.rs");
    let handlers: BTreeSet<_> = source.split("generate_handler![").nth(1).unwrap()
        .split(']').next().unwrap().split(',').map(str::trim).filter(|name| !name.is_empty())
        .map(|name| name.rsplit("::").next().unwrap()).collect();
    assert!(!commands.is_empty());
    assert_eq!(commands, handlers);
    let main: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
    let floating: serde_json::Value = serde_json::from_str(include_str!("../capabilities/floating.json")).unwrap();
    let permissions: Vec<_> = [&main, &floating].into_iter()
        .flat_map(|capability| capability["permissions"].as_array().unwrap()).collect();
    for command in commands {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(permissions.iter().any(|value| value.as_str() == Some(&permission)), "{permission}");
    }
}

#[test]
fn float_capability_only_exposes_bound_chat_commands() {
    let main: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
    let floating: serde_json::Value = serde_json::from_str(include_str!("../capabilities/floating.json")).unwrap();
    assert_eq!(main["windows"], serde_json::json!(["main"]));
    assert_eq!(floating["windows"], serde_json::json!(["postal-float-*"]));
    let actual: std::collections::BTreeSet<_> = floating["permissions"].as_array().unwrap()
        .iter().map(|value| value.as_str().unwrap()).collect();
    assert_eq!(actual, ["allow-float-context", "allow-float-subscribe", "allow-float-message-page",
        "allow-float-send-text", "allow-close-float-chat"].into_iter().collect());
    assert!(main["permissions"].as_array().unwrap().iter().any(|value| value == "allow-open-float-chat"));
}

#[test]
fn webview_policy_keeps_scripts_local_and_scopes_style_relaxation() {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let security = &config["app"]["security"];
    for name in ["csp", "devCsp"] {
        let policy = &security[name];
        assert_eq!(policy["script-src"], "'self'");
        assert_eq!(policy["object-src"], "'none'");
        assert_eq!(policy["frame-src"], "'none'");
        for directive in ["default-src", "script-src", "style-src", "img-src", "connect-src", "media-src", "font-src"] {
            let sources = policy[directive].as_str().unwrap();
            assert!(!sources.contains('*'), "{name}: {directive}");
            assert!(!sources.contains("unsafe-eval"));
            assert!(directive == "style-src" || !sources.contains("unsafe-inline"));
        }
        for directive in ["img-src", "media-src"] {
            let sources = policy[directive].as_str().unwrap();
            for source in ["asset:", "blob:", "data:"] { assert!(sources.contains(source)); }
        }
    }
    assert_eq!(security["dangerousDisableAssetCspModification"], serde_json::json!(["style-src"]));
}

#[test]
fn stale_sessions_spare_both_linked_modes() {
    assert!(!is_stale_session("session.db", &keep(&["session.db", "session-android.db"])));
    assert!(!is_stale_session("session.db-wal", &keep(&["session.db", "session-android.db"])));
    assert!(!is_stale_session(
        "session-android.db-shm",
        &keep(&["session.db", "session-android.db"])
    ));
    assert!(!is_stale_session(
        "session-android.db-wal",
        &keep(&["session.db", "session-android.db"])
    ));
    assert!(is_stale_session("session-1.db", &keep(&["session.db", "session-android.db"])));
    assert!(is_stale_session("session-1.db-shm", &keep(&["session.db"])));
    assert!(is_stale_session("session.db-wal", &keep(&["session-1.db"])));
    assert!(is_stale_session("session-1.db", &keep(&["session-2.db"])));
    assert!(!is_stale_session("session-2.db-wal", &keep(&["session-2.db"])));
    assert!(!is_stale_session("session.dbx", &keep(&["session-2.db"])));
    assert!(!is_stale_session("messages.db", &keep(&["session.db"])));
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
                once_paired: false,

            },
            Account {
                id: "acct-1".into(),
                label: "Work phone".into(),
                jid: None,
                once_paired: false,

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
            once_paired: false,

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
            once_paired: false,

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
            once_paired: false,

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

#[test]
fn cold_storage_keeps_each_account_in_its_own_folder() {
    use crate::account_store::history_base_for;
    use std::path::{Path, PathBuf};
    let base = Path::new("/data/account");
    assert_eq!(history_base_for(base, None, "default"), PathBuf::from("/data/account"));
    assert_eq!(history_base_for(base, Some("  "), "acct-1"), PathBuf::from("/data/account"));
    assert_eq!(history_base_for(base, Some("/cold"), "default"), PathBuf::from("/cold"));
    assert_eq!(history_base_for(base, Some("/cold"), "acct-1"), PathBuf::from("/cold/accounts/acct-1"));
}

#[test]
fn the_companion_forwards_only_store_changes() {
    use postal_core::ServiceEvent;
    let store_events = [
        ServiceEvent::Marks { chat: "a@s".into() },
        ServiceEvent::ChatStateChanged { chat: "a@s".into() },
        ServiceEvent::RetentionApplied { removed: 1 },
    ];
    for event in &store_events {
        assert!(crate::connection::instance_store_event(event), "{event:?}");
    }
    // Catch-up replay and connection noise stay out of the main UI's stream.
    let noise = [
        ServiceEvent::MessageHint {
            chat: "a@s".into(),
            id: "1".into(),
            sender: "b@s".into(),
            from_me: false,
            fresh: true,
            change: postal_core::HintChange::Arrival,
            status: None,
        },
        ServiceEvent::Message { message: Box::default() },
        ServiceEvent::Synced,
    ];
    for event in &noise {
        assert!(!crate::connection::instance_store_event(event), "{event:?}");
    }
}

#[test]
fn the_companion_sheet_skips_catch_up_noise() {
    use postal_core::ServiceEvent;
    for event in [
        ServiceEvent::QrCode { code: "x".into() },
        ServiceEvent::Connected,
        ServiceEvent::Disconnected,
        ServiceEvent::LoggedOut,
        ServiceEvent::Marks { chat: "a@s".into() },
    ] {
        assert!(crate::connection::instance_sheet_event(&event), "{event:?}");
    }
    for event in [
        ServiceEvent::MessageHint {
            chat: "a@s".into(),
            id: "1".into(),
            sender: "b@s".into(),
            from_me: false,
            fresh: true,
            change: postal_core::HintChange::Arrival,
            status: None,
        },
        ServiceEvent::Message { message: Box::default() },
        ServiceEvent::Syncing { pending: 9, applied: 1 },
    ] {
        assert!(!crate::connection::instance_sheet_event(&event), "{event:?}");
    }
}

#[test]
fn nvidia_modules_are_detected_for_the_renderer_workaround() {
    use crate::nvidia_module_loaded;
    assert!(nvidia_module_loaded("nvidia 123 0 - Live 0x0000\nnvidia_modeset 1 1 nvidia, Live 0x0"));
    assert!(nvidia_module_loaded("nvidia_drm 1 1 nvidia_modeset,nvidia, Live 0x0"));
    assert!(nvidia_module_loaded("nouveau 1 0 - Live 0x0"));
    assert!(!nvidia_module_loaded("amdgpu 1 0 - Live 0x0\ni915 0 0 - Live 0x0"));
    assert!(!nvidia_module_loaded(""));
}
