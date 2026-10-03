use crate::command_error::{CommandError, CommandResult};
use tauri::State;
use std::sync::Arc;
use crate::{AppState, groups::Joined};

#[tauri::command]
pub(crate) async fn group_invite_link(state: State<'_, AppState>, account: String, chat: String, reset: bool) -> CommandResult<String> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.group_invite_link(&chat, reset, || account_current(&state, &account, &service).map_err(|error| error.message.into())).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn join_group_invite_message(state: State<'_, AppState>, account: String, chat: String, id: String) -> CommandResult<Joined> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.join_group_invite_message(&chat, &id, || account_current(&state, &account, &service).map_err(|error| error.message.into())).await;
    account_current(&state, &account, &service)?;
    let (jid, pending) = result.map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    Ok(Joined { jid, pending })
}

fn account_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> CommandResult<()> {
    let current = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}
