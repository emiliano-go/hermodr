use crate::{AppState, command_error::{CommandError, CommandResult}};
use postal_core::store::group_audit::{GroupAuditFilter, GroupAuditPage};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn group_audit_page(state: State<'_, AppState>, account_id: String, chat: Option<String>, filter: GroupAuditFilter) -> CommandResult<GroupAuditPage> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service.group_audit_page(chat.as_deref(), filter).await;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}
