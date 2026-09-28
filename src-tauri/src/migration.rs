use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use crate::account_store::{AccountsFile, data_dir, media_cache_dir};

/// Moves media out of the folders earlier versions used: next to the session,
/// then the shared cache root (`~/.cache/media`, `%LOCALAPPDATA%\media`).
pub(crate) fn migrate_media(app: &AppHandle, accounts: &AccountsFile) {
    let legacy: Vec<PathBuf> = [Ok(data_dir(app).join("media")), app.path().cache_dir().map(|d| d.join("media"))]
        .into_iter()
        .flatten()
        .filter(|dir| dir.is_dir())
        .collect();
    if legacy.is_empty() {
        return;
    }
    let from: Vec<&std::path::Path> = legacy.iter().map(PathBuf::as_path).collect();
    let to = media_cache_dir(app);
    let settings = crate::settings::load_settings(app);
    for account in &accounts.accounts {
        let db = crate::account_store::history_base(app, &settings, &account.id).join("messages.db");
        if !db.exists() {
            continue;
        }
        match postal_core::MessageStore::open(&db) {
            Ok(store) => {
                if let Err(e) = store.relocate_media(&from, &to) {
                    log::warn!("media migration for {}: {e}", account.id);
                }
            }
            Err(e) => log::warn!("media migration for {}: {e}", account.id),
        }
    }
    for dir in &legacy {
        // Only succeeds once empty, so another program's files keep the folder.
        let _ = std::fs::remove_dir(dir);
    }
}

/// Bundle identifier from before the rename to Postal. Data written under it is
/// moved over on first launch of the renamed app.
pub(crate) const LEGACY_IDENTIFIER: &str = "com.hermodr.app";

/// Current bundle identifier; must match `tauri.conf.json`.
pub(crate) const IDENTIFIER: &str = "com.postal.app";

/// Adopts a pre-rename install: when the old bundle's data, config or cache
/// directory exists and the new one does not, it is moved over, keeping the
/// WhatsApp session, message history, settings and webview storage.
pub(crate) fn migrate_bundle_id() {
    for base in [dirs::data_dir(), dirs::config_dir(), dirs::cache_dir()] {
        let Some(base) = base else { continue };
        move_dir(&base.join(LEGACY_IDENTIFIER), &base.join(IDENTIFIER));
    }
    rename_legacy_logs();
}

/// Renames `from` onto `to` when only the old path exists, copying across
/// filesystems when a rename is not possible.
pub(crate) fn move_dir(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_dir() || to.exists() {
        return;
    }
    if std::fs::rename(from, to).is_ok() {
        return;
    }
    if copy_dir(from, to).is_err() {
        eprintln!("could not migrate {} to {}", from.display(), to.display());
    }
}

/// Recursively copies, then removes the source once the copy is complete.
pub(crate) fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    std::fs::remove_dir_all(from)
}

/// The log moved with the data directory; its name changed with the app.
pub(crate) fn rename_legacy_logs() {
    let Some(dir) = dirs::data_dir().map(|d| d.join(IDENTIFIER)) else { return };
    for (from, to) in [("hermodr.log", "postal.log"), ("hermodr.log.old", "postal.log.old")] {
        let from = dir.join(from);
        if from.is_file() {
            let _ = std::fs::rename(&from, dir.join(to));
        }
    }
}
