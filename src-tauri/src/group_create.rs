use crate::command_error::{CommandError, CommandResult};
use std::sync::Arc;
use tauri::State;
use crate::{AppState };

#[tauri::command]
pub(crate) async fn group_creation_contacts(state: State<'_, AppState>, account: String, query: String) -> CommandResult<Vec<postal_core::SearchResult>> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.group_creation_contacts(&query).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn create_group(state: State<'_, AppState>, account: String, subject: String, jids: Vec<String>) -> CommandResult<postal_core::GroupCreateResult> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.create_group(&subject, &jids, || account_current(&state, &account, &service)
        .map_err(|error| error.message.into())).await
        .map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

fn account_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> CommandResult<()> {
    let current = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}
