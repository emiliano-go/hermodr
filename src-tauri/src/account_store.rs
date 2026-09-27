use std::path::PathBuf;
use postal_core::ServiceConfig;
use tauri::{AppHandle, Manager};
use crate::{AppState, settings::UiSettings};

/// The label an account starts with, replaced by the profile name on its first
/// connect. Still holding it is how we tell an untouched account from one the
/// user named themselves, so it must stay in step with the three writes below.
pub(crate) const DEFAULT_ACCOUNT_LABEL: &str = "WhatsApp";

/// One signed-in account.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    /// Learned once the account connects, so its picture shows while inactive.
    #[serde(default)]
    pub jid: Option<String>,
}

/// The account list, persisted next to the app config.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct AccountsFile {
    pub(crate) accounts: Vec<Account>,
    pub(crate) active: Option<String>,
}

impl AccountsFile {
    /// Names an account after its own profile name, once the app knows one.
    ///
    /// Only an account still carrying the default label is written, so this
    /// runs at most once per account and a name the user chose always wins. An
    /// unknown profile name leaves the label alone. Returns whether it wrote.
    pub(crate) fn seed_label(&mut self, id: &str, push_name: &str) -> bool {
        let push_name = push_name.trim();
        if push_name.is_empty() {
            return false;
        }
        let Some(account) = self.accounts.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        let untouched = {
            let label = account.label.trim();
            label.is_empty() || label == DEFAULT_ACCOUNT_LABEL
        };
        if !untouched {
            return false;
        }
        account.label = push_name.to_string();
        true
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AccountsView {
    pub accounts: Vec<Account>,
    pub active: Option<String>,
}

/// Where this account's data lives.
pub(crate) fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Where an account's files live. The first account keeps the old layout so an
/// existing single-account install is adopted in place.
pub(crate) fn account_base(app: &AppHandle, id: &str) -> std::path::PathBuf {
    if id == "default" {
        data_dir(app)
    } else {
        data_dir(app).join("accounts").join(id)
    }
}

pub(crate) fn accounts_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("accounts.json")
}

pub(crate) fn save_accounts(app: &AppHandle, file: &AccountsFile) {
    let path = accounts_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(file) {
        let _ = std::fs::write(path, json);
    }
}

/// Loads the account list, adopting an existing single-account install the
/// first time.
pub(crate) fn load_accounts(app: &AppHandle) -> AccountsFile {
    if let Ok(json) = std::fs::read_to_string(accounts_path(app)) {
        if let Ok(file) = serde_json::from_str::<AccountsFile>(&json) {
            return file;
        }
    }
    let mut file = AccountsFile::default();
    if data_dir(app).join("session.db").exists() {
        file.accounts.push(Account {
            id: "default".into(),
            label: DEFAULT_ACCOUNT_LABEL.into(),
            jid: None,
        });
        file.active = Some("default".into());
        save_accounts(app, &file);
    }
    file
}

pub(crate) fn active_account(state: &AppState) -> Option<String> {
    state.accounts.lock().unwrap().active.clone()
}

pub(crate) fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Where downloads live by default. The cache, because media is regenerable and
/// should not be carried in a backup of the account.
pub(crate) fn media_cache_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("media"))
        .unwrap_or_else(|_| data_dir(app).join("media"))
}

pub(crate) fn config_for(app: &AppHandle, settings: &UiSettings, account: &str) -> ServiceConfig {
    let base = account_base(app, account);
    let default_media = media_cache_dir(app);
    ServiceConfig {
        session_path: session_path(&base),
        messages_path: if settings.keep_history {
            base.join("messages.db")
        } else {
            PathBuf::from(":memory:")
        },
        // Aliases outlive the history setting, so they never travel with it.
        aliases_path: base.join("aliases.db"),
        retention: settings.retention,
        accept_full_history: settings.accept_full_history,
        auto_download_media: settings.auto_download_media,
        keep_archived: settings.keep_archived,
        // An unset or empty setting falls back to the app data directory.
        media_dir: settings
            .media_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or(Some(default_media)),
    }
}

/// Names the account's current session file; absent means `session.db`.
pub(crate) const SESSION_POINTER: &str = "session_name";

pub(crate) fn session_path(base: &std::path::Path) -> PathBuf {
    let name = std::fs::read_to_string(base.join(SESSION_POINTER)).unwrap_or_default();
    base.join(match name.trim() {
        "" => "session.db",
        name => name,
    })
}

/// Deletes session files other than `current`; one still held open is retried on a later start.
pub(crate) fn remove_stale_sessions(base: &std::path::Path, current: &std::path::Path) {
    let Some(current) = current.file_name().and_then(|n| n.to_str()) else { return };
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_stale_session(name, current) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// True for an old `session*.db` file or its `-wal`/`-shm`/`-journal`; never
/// for the current database or its own sidecars.
pub(crate) fn is_stale_session(name: &str, current: &str) -> bool {
    let db = ["-wal", "-shm", "-journal"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    db.starts_with("session") && db.ends_with(".db") && db != current
}
