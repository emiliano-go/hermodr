use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::Mutex};
use postal_core::database_crypto::{DatabaseKey, needs_key, prepare_database};
use tauri::{AppHandle, Manager, State};
use crate::{account_store::{account_base, current_sessions, history_base, AccountsFile}, AppState, UiSettings};

pub(crate) struct DatabaseEncryption {
    enabled_at_start: bool,
    accounts: Mutex<BTreeMap<String, Result<Option<DatabaseKey>, String>>>,
}

impl DatabaseEncryption {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled_at_start: enabled, accounts: Mutex::new(BTreeMap::new()) }
    }

    pub(crate) fn key(&self, account: &str) -> Result<Option<DatabaseKey>, String> {
        self.accounts.lock().unwrap().get(account).cloned().unwrap_or(Ok(None))
    }

    pub(crate) fn new_account_key(&self, account: &str, directory: &Path) -> Result<Option<DatabaseKey>, String> {
        if let Some(key) = self.accounts.lock().unwrap().get(account).cloned() { return key; }
        let key = if self.enabled_at_start { crate::database_keys::load_or_create(account, true, directory).map(Some) } else { Ok(None) };
        self.accounts.lock().unwrap().insert(account.to_owned(), key.clone());
        key
    }
}

fn files(base: &Path, history: &Path, previous: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<_> = ["messages.db", "scheduled.db", "favorites.db", "aliases.db"]
        .into_iter().map(|name| base.join(name)).collect();
    files.push(history.join("messages.db"));
    if let Some(previous) = previous { files.push(previous.join("messages.db")); }
    for path in current_sessions(base) {
        if path.parent() != Some(base) { return Err("Session pointer escapes its account folder.".into()); }
        files.push(path);
    }
    if base.exists() {
        for entry in std::fs::read_dir(base).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            if let Some(name) = name.to_str().and_then(session_database_name) { files.push(base.join(name)); }
        }
    }
    files.sort(); files.dedup();
    Ok(files)
}

fn session_database_name(name: &str) -> Option<&str> {
    let database = name.strip_suffix(".postal-encryption")
        .or_else(|| name.find(".db.cipher-").map(|end| &name[..end + 3])).unwrap_or(name);
    (database.starts_with("session") && database.ends_with(".db")).then_some(database)
}

fn prepare_files(paths: &[PathBuf], requested: bool,
    load: impl FnOnce(bool) -> Result<DatabaseKey, String>) -> Result<Option<DatabaseKey>, String> {
    let encrypted = paths.iter().try_fold(false, |found, path| needs_key(path).map(|value| found || value))
        .map_err(|error| error.to_string())?;
    if !requested && !encrypted { return Ok(None); }
    let key = load(!encrypted)?;
    for path in paths { prepare_database(path, &key).map_err(|error| error.to_string())?; }
    Ok(Some(key))
}

pub(crate) fn initialize(app: &AppHandle, settings: &UiSettings, accounts: &AccountsFile) {
    let encryption = DatabaseEncryption::new(settings.encrypt_databases);
    let key_directory = account_base(app, "default");
    for account in &accounts.accounts {
        let base = account_base(app, &account.id);
        let previous = std::fs::read_to_string(base.join(crate::account_store::HISTORY_POINTER)).ok()
            .filter(|value| !value.trim().is_empty()).map(|value| PathBuf::from(value.trim()));
        let key = files(&base, &history_base(app, settings, &account.id), previous.as_deref())
            .and_then(|paths| prepare_files(&paths, settings.encrypt_databases,
                |may_create| crate::database_keys::load_or_create(&account.id, may_create, &key_directory)));
        if key.is_err() { log::warn!("protected storage for an account could not be prepared"); }
        encryption.accounts.lock().unwrap().insert(account.id.clone(), key);
    }
    app.manage(encryption);
}

pub(crate) fn command_failure(error: String) -> crate::command_error::CommandError {
    match error.as_str() {
        "error.database_key_invalid_account" | "error.database_key_missing" | "error.database_key_corrupt"
        | "error.database_key_unavailable" | "error.database_key_save_failed" | "error.database_key_random_unavailable" =>
            crate::command_error::CommandError::code(&error),
        _ => crate::command_error::CommandError::code("error.database_prepare_failed").with_diagnostic(error),
    }
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct DatabaseEncryptionStatus {
    active_account: Option<String>,
    requested: bool,
    enabled_at_start: bool,
    active_account_encrypted: Option<bool>,
    restart_required: bool,
    error: Option<postal_core::message_ref::MessageRef>,
    diagnostic: Option<String>,
}

#[tauri::command]
pub(crate) fn database_encryption_status(app: AppHandle, state: State<'_, AppState>) -> DatabaseEncryptionStatus {
    let encryption = app.state::<DatabaseEncryption>();
    let requested = state.settings.lock().unwrap().encrypt_databases;
    let account = state.accounts.lock().unwrap().active.clone();
    let key = account.as_deref().map(|account| encryption.key(account));
    let failure = key.as_ref().and_then(|key| key.as_ref().err()).cloned();
    let error = failure.as_ref().map(|failure| postal_core::message_ref::MessageRef::new(
        if failure.starts_with("error.database_key_") { failure.as_str() } else { "error.database_prepare_failed" }));
    let diagnostic = failure.filter(|failure| !failure.starts_with("error.database_key_"));
    DatabaseEncryptionStatus { active_account: account, requested, enabled_at_start: encryption.enabled_at_start,
        active_account_encrypted: key.as_ref().and_then(|key| key.as_ref().ok().map(Option::is_some)),
        restart_required: requested != encryption.enabled_at_start, error, diagnostic }
}

#[cfg(test)]
#[path = "database_encryption_tests.rs"]
mod tests;
