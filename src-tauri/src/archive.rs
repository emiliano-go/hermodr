use postal_core::store::archive::ArchiveReport;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use crate::{AppState, account_store::{Account, account_base, media_cache_dir, now_millis, save_accounts_checked}};

#[tauri::command]
pub(crate) async fn export_archive(window: WebviewWindow, state: State<'_, AppState>, chat: Option<String>) -> Result<Option<ArchiveReport>, String> {
    let service = state.service()?;
    let selected = tauri::async_runtime::spawn_blocking(move || window.dialog().file()
        .set_parent(&window).set_title("Choose export destination").blocking_pick_folder()).await.map_err(|e| e.to_string())?;
    let Some(selected) = selected else { return Ok(None) };
    let directory = selected.into_path().map_err(|e| e.to_string())?
        .join(format!("postal-{}-{}", if chat.is_some() { "conversation" } else { "backup" }, now_millis()));
    service.export_archive(directory, chat).await.map(Some).map_err(|e| format!("Export failed: {e:#}"))
}

#[tauri::command]
pub(crate) async fn restore_local_backup(app: AppHandle, window: WebviewWindow, state: State<'_, AppState>) -> Result<Option<ArchiveReport>, String> {
    if !state.settings.lock().unwrap().keep_history {
        return Err("Enable Keep history before restoring an account".into());
    }
    let selected = tauri::async_runtime::spawn_blocking(move || window.dialog().file()
        .set_parent(&window).set_title("Choose a Postal backup folder").blocking_pick_folder()).await.map_err(|e| e.to_string())?;
    let Some(selected) = selected else { return Ok(None) };
    let source = selected.into_path().map_err(|e| e.to_string())?;
    let id = format!("acct-restored-{}", now_millis());
    let account = account_base(&app, &id);
    let media = state.settings.lock().unwrap().media_dir.as_deref().filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from).unwrap_or_else(|| media_cache_dir(&app)).join(&id);
    let report = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        if let Some(parent) = account.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        if let Some(parent) = media.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        postal_core::store::archive::restore_backup(&source, &account, &media).map_err(|e| format!("{e:#}"))
    }).await.map_err(|e| e.to_string())?.map_err(|e| format!("Restore failed: {e:#}"))?;
    let mut accounts = state.accounts.lock().unwrap();
    let mut next = accounts.clone();
    next.accounts.push(Account { id, label: "Restored backup".into(), jid: None, once_paired: false });
    save_accounts_checked(&app, &next).map_err(|error| format!("Archive restored to {}, but the account list could not be saved: {error}", report.directory))?;
    *accounts = next;
    Ok(Some(report))
}
