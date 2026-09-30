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
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Account {
    pub id: String,
    pub label: String,
    /// Learned once the account connects, so its picture shows while inactive.
    #[serde(default)]
    pub jid: Option<String>,
    /// Whether the optional Android instance has been paired. Kept while it is
    /// stopped so the toggle can say so without starting it.
    #[serde(default)]
    pub once_paired: bool,
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
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
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
    if let Err(error) = save_accounts_checked(app, file) {
        log::error!("could not save account list: {error}");
    }
}

pub(crate) fn save_accounts_checked(app: &AppHandle, file: &AccountsFile) -> Result<(), String> {
    persist_accounts(&accounts_path(app), file).map_err(|error| error.to_string())
}

fn persist_accounts(path: &std::path::Path, file: &AccountsFile) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temporary = path.with_extension(format!("{}.tmp", now_millis()));
    let mut output = std::fs::File::create_new(&temporary)?;
    serde_json::to_writer_pretty(&mut output, file).map_err(std::io::Error::other)?;
    output.flush()?;
    output.sync_all()?;
    drop(output);
    std::fs::rename(temporary, path)
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
            once_paired: false,

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
        session_path: session_path(&base, false),
        messages_path: if settings.keep_history {
            history_base(app, settings, account).join("messages.db")
        } else {
            PathBuf::from(":memory:")
        },
        scheduled_path: base.join("scheduled.db"),
        favorites_path: base.join("favorites.db"),
        // Aliases outlive the history setting, so they never travel with it.
        aliases_path: base.join("aliases.db"),
        retention: settings.retention,
        request_full_history: settings.request_full_history,
        auto_download_media: settings.auto_download_media,
        keep_archived: settings.keep_archived,
        // The main link is the ordinary companion; view-once media is fetched
        // by the optional Android instance instead.
        android_pair: false,
        keep_view_once: false,
        // The main link ingests everything; only the Android instance runs
        // one-time-only.
        one_time_only: false,
        // An unset or empty setting falls back to the app data directory.
        media_dir: settings
            .media_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or(Some(default_media)),
    }
}

/// The optional Android instance: a second link that receives view-once media
/// the External companion never gets, and writes what it keeps into the same
/// message store so the main UI sees it immediately.
pub(crate) fn once_config_for(app: &AppHandle, settings: &UiSettings, account: &str) -> Result<ServiceConfig, String> {
    if !settings.keep_history {
        return Err("the Android instance needs history kept on this computer".into());
    }
    let mut config = config_for(app, settings, account);
    config.session_path = session_path(&account_base(app, account), true);
    config.android_pair = true;
    // The instance is not the user's session: it takes messages from the shared
    // store but must not download ordinary media twice or ask for old history.
    config.auto_download_media = false;
    config.keep_view_once = true;
    config.one_time_only = true;
    config.request_full_history = false;
    config.keep_archived = true;
    Ok(config)
}

/// Removes an account's message archive wherever cold storage put it, so a
/// removed account leaves no history behind outside its own folder.
pub(crate) fn remove_history(app: &AppHandle, settings: &UiSettings, account: &str) {
    let base = account_base(app, account);
    let folder = history_base(app, settings, account);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(folder.join(format!("messages.db{suffix}")));
    }
    if folder != base {
        // Only succeeds once empty, so another account's files stay.
        let _ = std::fs::remove_dir(&folder);
    }
    let _ = std::fs::remove_file(base.join(HISTORY_POINTER));
}

/// Names the account's current session file; absent means the default below.
/// Each device mode keeps its own link, so switching modes never unlinks.
pub(crate) const SESSION_POINTER: &str = "session_name";
pub(crate) const SESSION_POINTER_ANDROID: &str = "session_name_android";
/// Folder the message archive was last opened from, so a change of the
/// cold-storage setting can move the database exactly once.
pub(crate) const HISTORY_POINTER: &str = "history_dir";

/// Where an account's message archive lives: the configured cold-storage
/// folder when one is set, else beside the rest of the account's files. Each
/// account gets its own subfolder there, so several accounts never share one
/// messages.db.
pub(crate) fn history_base(app: &AppHandle, settings: &UiSettings, account: &str) -> PathBuf {
    history_base_for(&account_base(app, account), settings.history_dir.as_deref(), account)
}

/// [`history_base`] without the app handle, so the layout rule is testable.
pub(crate) fn history_base_for(base: &std::path::Path, configured: Option<&str>, account: &str) -> PathBuf {
    let configured = configured
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    match configured {
        Some(dir) if account == "default" => dir,
        Some(dir) => dir.join("accounts").join(account),
        None => base.to_path_buf(),
    }
}

/// Moves the archive to where the cold-storage setting now points, once, before
/// it is opened. The `-wal`, `-shm` and `-journal` sidecars travel with it, so
/// no committed page is left behind; a rename that fails across filesystems
/// falls back to a copy. The pointer only moves after every file did.
pub(crate) fn migrate_history(app: &AppHandle, settings: &UiSettings, account: &str) {
    let base = account_base(app, account);
    let wanted = history_base(app, settings, account);
    let remember = || {
        if let Err(error) = std::fs::write(base.join(HISTORY_POINTER), wanted.to_string_lossy().as_bytes()) {
            log::warn!("could not remember the history folder: {error}");
        }
    };
    let last = std::fs::read_to_string(base.join(HISTORY_POINTER))
        .ok()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| base.clone());
    // Nothing to do when it already lives there, or there is no archive yet.
    if wanted == last || !last.join("messages.db").exists() || wanted.join("messages.db").exists() {
        remember();
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&wanted) {
        log::error!("could not create the history folder {}: {error}", wanted.display());
        return;
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let from = last.join(format!("messages.db{suffix}"));
        if !from.exists() {
            continue;
        }
        let to = wanted.join(format!("messages.db{suffix}"));
        if std::fs::rename(&from, &to).is_ok() {
            continue;
        }
        match std::fs::copy(&from, &to) {
            Ok(_) => {
                let _ = std::fs::remove_file(&from);
            }
            Err(error) => {
                log::error!("could not move {} to {}: {error}", from.display(), to.display());
                return;
            }
        }
    }
    log::info!("moved the message archive to {}", wanted.display());
    remember();
}

pub(crate) fn session_path(base: &std::path::Path, android: bool) -> PathBuf {
    let (pointer, default) = if android {
        (SESSION_POINTER_ANDROID, "session-android.db")
    } else {
        (SESSION_POINTER, "session.db")
    };
    let name = std::fs::read_to_string(base.join(pointer)).unwrap_or_default();
    base.join(match name.trim() {
        "" => default,
        name => name,
    })
}

/// Both modes' current session files: the stale sweep must spare both, since
/// each is a live link kept for its own mode.
pub(crate) fn current_sessions(base: &std::path::Path) -> [PathBuf; 2] {
    [session_path(base, false), session_path(base, true)]
}

/// Deletes session files other than the two current ones; a file still held open is retried on a later start.
pub(crate) fn remove_stale_sessions(base: &std::path::Path, current: &[PathBuf]) {
    let keep: Vec<String> = current
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
        .collect();
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_stale_session(name, &keep) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// True for an old `session*.db` file or its `-wal`/`-shm`/`-journal`; never
/// for a current database or its own sidecars.
pub(crate) fn is_stale_session(name: &str, keep: &[String]) -> bool {
    let db = ["-wal", "-shm", "-journal"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    db.starts_with("session") && db.ends_with(".db") && !keep.iter().any(|k| k == db)
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn account_list_replacement_preserves_active_account() {
        let root = std::env::temp_dir().join(format!("postal-accounts-{}-{}", std::process::id(), now_millis()));
        let path = root.join("accounts.json");
        let mut accounts = AccountsFile { accounts: vec![], active: Some("existing".into()) };
        persist_accounts(&path, &accounts).unwrap();
        accounts.accounts.push(Account { id: "restored".into(), label: "Restored backup".into(), jid: None, once_paired: false });
        persist_accounts(&path, &accounts).unwrap();
        let loaded: AccountsFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded.active.as_deref(), Some("existing"));
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(persist_accounts(&path.join("invalid-parent"), &accounts).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), serde_json::to_vec_pretty(&accounts).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
