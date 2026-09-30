use tauri::State;
use std::sync::Arc;
use crate::{AppState, connection::command_error, groups::Joined};

#[tauri::command]
pub(crate) async fn group_invite_link(state: State<'_, AppState>, account: String, chat: String, reset: bool) -> Result<String, String> {
    let service = state.account_service(&account)?;
    let result = service.group_invite_link(&chat, reset, || account_current(&state, &account, &service).map_err(|error| std::io::Error::other(error).into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn join_group_invite_message(state: State<'_, AppState>, account: String, chat: String, id: String) -> Result<Joined, String> {
    let service = state.account_service(&account)?;
    let result = service.join_group_invite_message(&chat, &id, || account_current(&state, &account, &service).map_err(|error| std::io::Error::other(error).into())).await;
    account_current(&state, &account, &service)?;
    let (jid, pending) = result.map_err(|error| command_error(&service, error))?;
    Ok(Joined { jid, pending })
}

fn account_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> Result<(), String> {
    let current = state.account_service(account)?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err("account changed during operation".into()) }
}
