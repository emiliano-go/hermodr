use super::*;

fn file(active: Option<&str>, ids: &[&str]) -> AccountsFile {
    AccountsFile { active: active.map(str::to_owned), accounts: ids.iter().map(|id| Account {
        id: (*id).into(), label: format!("Label {id}"), jid: Some(format!("{id}@synthetic")), once_paired: true,
    }).collect() }
}

#[test]
fn unknown_cleanup_id_rejects_without_mutating_registered_accounts() {
    let mut accounts = file(Some("active"), &["active", "inactive"]);
    let before = serde_json::to_value(&accounts).unwrap();
    let error = select_account_removal(&mut accounts, "unknown").unwrap_err();
    assert_eq!(error.message.code, "error.unknown_account");
    assert_eq!(serde_json::to_value(accounts).unwrap(), before);
}

#[test]
fn registered_path_escaping_cleanup_ids_reject_on_both_platforms_without_mutation() {
    for id in ["", ".", "..", "../outside", "..\\outside", "nested/account", "nested\\account", "/absolute", "\\absolute",
        "C:outside", "C:\\outside", "safe:stream", "nul\0suffix", "...", "trailing.", "trailing "] {
        let mut accounts = file(Some("active"), &["active", id]);
        let before = serde_json::to_value(&accounts).unwrap();
        assert_eq!(select_account_removal(&mut accounts, id).unwrap_err().message.code, "error.unknown_account");
        assert_eq!(serde_json::to_value(accounts).unwrap(), before, "{id:?}");
    }
}

#[test]
fn inactive_cleanup_preserves_active_selection_and_remaining_metadata() {
    let mut accounts = file(Some("active"), &["active", "inactive", "remaining"]);
    let active = serde_json::to_value(&accounts.accounts[0]).unwrap();
    let remaining = serde_json::to_value(&accounts.accounts[2]).unwrap();
    assert!(!select_account_removal(&mut accounts, "inactive").unwrap());
    assert_eq!(accounts.active.as_deref(), Some("active"));
    assert_eq!(accounts.accounts.len(), 2);
    assert_eq!(serde_json::to_value(&accounts.accounts[0]).unwrap(), active);
    assert_eq!(serde_json::to_value(&accounts.accounts[1]).unwrap(), remaining);
}

#[test]
fn active_cleanup_selects_next_registered_account_and_last_cleanup_clears_selection() {
    let mut accounts = file(Some("active"), &["active", "remaining"]);
    assert!(select_account_removal(&mut accounts, "active").unwrap());
    assert_eq!(accounts.active.as_deref(), Some("remaining"));
    assert!(select_account_removal(&mut accounts, "remaining").unwrap());
    assert!(accounts.accounts.is_empty());
    assert!(accounts.active.is_none());
}

#[test]
fn default_and_ordinary_registered_basenames_keep_existing_ids_and_labels() {
    for id in ["default", "acct-123", "synthetic account", "account.name", "part..name", ".hidden", "حساب"] {
        let mut accounts = file(Some(id), &[id, "remaining"]);
        let survivor = serde_json::to_value(&accounts.accounts[1]).unwrap();
        assert!(select_account_removal(&mut accounts, id).unwrap());
        assert_eq!(accounts.active.as_deref(), Some("remaining"));
        assert_eq!(serde_json::to_value(&accounts.accounts[0]).unwrap(), survivor);
    }
    let mut accounts = file(Some("active"), &["active", "default"]);
    assert!(!select_account_removal(&mut accounts, "default").unwrap());
    assert_eq!(accounts.active.as_deref(), Some("active"));
}
