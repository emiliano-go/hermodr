use crate::{AppState, connection::command_error};
use postal_core::store::group_audit::{GroupAuditFilter, GroupAuditPage};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn group_audit_page(state: State<'_, AppState>, account_id: String, chat: Option<String>, filter: GroupAuditFilter) -> Result<GroupAuditPage, String> {
    let service = state.account_service(&account_id)?;
    let result = service.group_audit_page(chat.as_deref(), filter).await;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed while reading group audit".into()); }
    result.map_err(|error| command_error(&service, error))
}
