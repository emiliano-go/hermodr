use crate::command_error::{CommandError, CommandResult};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use postal_core::{GroupSettingChange, GroupSettings, WhatsAppService};
use std::sync::Arc;
use tauri::State;
use crate::{AppState };

#[tauri::command]
pub(crate) async fn group_settings(state: State<'_, AppState>, account: String, chat: String) -> CommandResult<GroupSettings> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.group_settings(&chat, || account_current(&state, &account, &service)
        .map_err(|error| error.message.into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn change_group_setting(state: State<'_, AppState>, account: String, chat: String, change: GroupSettingChange) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.change_group_setting(&chat, change, || account_current(&state, &account, &service)
        .map_err(|error| error.message.into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn set_group_picture(state: State<'_, AppState>, account: String, chat: String, data: String) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.group_picture_base64").with_diagnostic(error))?;
    let result = service.set_group_picture(&chat, bytes, || account_current(&state, &account, &service)
        .map_err(|error| error.message.into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

fn account_current(state: &AppState, account: &str, expected: &Arc<WhatsAppService>) -> CommandResult<()> {
    let current = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}
