use std::sync::Arc;
use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn group_creation_contacts(state: State<'_, AppState>, account: String, query: String) -> Result<Vec<postal_core::SearchResult>, String> {
    let service = state.account_service(&account)?;
    let result = service.group_creation_contacts(&query).await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn create_group(state: State<'_, AppState>, account: String, subject: String, jids: Vec<String>) -> Result<postal_core::GroupCreateResult, String> {
    let service = state.account_service(&account)?;
    service.create_group(&subject, &jids, || account_current(&state, &account, &service)
        .map_err(|error| std::io::Error::other(error).into())).await
        .map_err(|error| command_error(&service, error))
}

fn account_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> Result<(), String> {
    let current = state.account_service(account)?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err("account changed during operation".into()) }
}
