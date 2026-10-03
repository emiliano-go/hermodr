use postal_core::store::archive::ArchiveReport;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use crate::command_error::{CommandError, CommandResult};
use crate::{AppState, account_store::{Account, account_base, media_cache_dir, now_millis, save_accounts_checked}};

#[tauri::command]
pub(crate) async fn export_archive(window: WebviewWindow, state: State<'_, AppState>, chat: Option<String>) -> CommandResult<Option<ArchiveReport>> {
    let service = state.service().map_err(|_| CommandError::code("error.not_connected"))?;
    let title = crate::native_locale::text(window.app_handle(), "native.archive_export");
    let selected = tauri::async_runtime::spawn_blocking(move || window.dialog().file()
        .set_parent(&window).set_title(title).blocking_pick_folder()).await.map_err(CommandError::operation_failed)?;
    let Some(selected) = selected else { return Ok(None) };
    let directory = selected.into_path().map_err(|e| e.to_string())?
        .join(format!("postal-{}-{}", if chat.is_some() { "conversation" } else { "backup" }, now_millis()));
    service.export_archive(directory, chat).await.map(Some)
        .map_err(|error| CommandError::code("error.archive_export_failed").with_diagnostic(error))
}

#[tauri::command]
pub(crate) async fn restore_local_backup(app: AppHandle, window: WebviewWindow, state: State<'_, AppState>) -> CommandResult<Option<ArchiveReport>> {
    if !state.settings.lock().unwrap().keep_history {
        return Err(CommandError::code("error.restore_history_disabled"));
    }
    let title = crate::native_locale::text(&app, "native.archive_restore");
    let selected = tauri::async_runtime::spawn_blocking(move || window.dialog().file()
        .set_parent(&window).set_title(title).blocking_pick_folder()).await.map_err(CommandError::operation_failed)?;
    let Some(selected) = selected else { return Ok(None) };
    let source = selected.into_path().map_err(|e| e.to_string())?;
    let id = format!("acct-restored-{}", now_millis());
    let account = account_base(&app, &id);
    let database_key = app.state::<crate::database_encryption::DatabaseEncryption>().new_account_key(&id, &account_base(&app, "default"))
        .map_err(crate::database_encryption::command_failure)?;
    let media = state.settings.lock().unwrap().media_dir.as_deref().filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from).unwrap_or_else(|| media_cache_dir(&app)).join(&id);
    let report = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        if let Some(parent) = account.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        if let Some(parent) = media.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        postal_core::store::archive::restore_backup_with_key(&source, &account, &media, database_key.as_ref()).map_err(|e| format!("{e:#}"))
    }).await.map_err(CommandError::operation_failed)?
        .map_err(|error| CommandError::code("error.archive_restore_failed").with_diagnostic(error))?;
    let mut accounts = state.accounts.lock().unwrap();
    let mut next = accounts.clone();
    next.accounts.push(Account { id, label: "Restored backup".into(), jid: None, once_paired: false });
    save_accounts_checked(&app, &next).map_err(|error| CommandError::new(
        postal_core::message_ref::MessageRef::new("error.archive_account_save_failed")
            .with_param("directory", report.directory.clone())).with_diagnostic(error))?;
    *accounts = next;
    Ok(Some(report))
}
