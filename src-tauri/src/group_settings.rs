use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use postal_core::{GroupSettingChange, GroupSettings, WhatsAppService};
use std::sync::Arc;
use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn group_settings(state: State<'_, AppState>, account: String, chat: String) -> Result<GroupSettings, String> {
    let service = state.account_service(&account)?;
    let result = service.group_settings(&chat, || account_current(&state, &account, &service)
        .map_err(|error| std::io::Error::other(error).into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn change_group_setting(state: State<'_, AppState>, account: String, chat: String, change: GroupSettingChange) -> Result<(), String> {
    let service = state.account_service(&account)?;
    let result = service.change_group_setting(&chat, change, || account_current(&state, &account, &service)
        .map_err(|error| std::io::Error::other(error).into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn set_group_picture(state: State<'_, AppState>, account: String, chat: String, data: String) -> Result<(), String> {
    let service = state.account_service(&account)?;
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error| error.to_string())?;
    let result = service.set_group_picture(&chat, bytes, || account_current(&state, &account, &service)
        .map_err(|error| std::io::Error::other(error).into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

fn account_current(state: &AppState, account: &str, expected: &Arc<WhatsAppService>) -> Result<(), String> {
    let current = state.account_service(account)?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err("account changed during operation".into()) }
}
