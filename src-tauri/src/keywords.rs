use crate::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn keyword_mentions(
    state: State<'_, AppState>,
    account_id: String,
    highlight: Vec<String>,
    hide: Vec<String>,
) -> Result<std::collections::HashMap<String, i64>, String> {
    let service = state.account_service(&account_id)?;
    let counts = service
        .keyword_mentions(&highlight, &hide)
        .await
        .map_err(|error| error.to_string())?;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) {
        return Err("account changed while counting keyword matches".into());
    }
    Ok(counts)
}

#[tauri::command]
pub(crate) async fn keyword_matches(
    state: State<'_, AppState>,
    account_id: String,
    chat: Option<String>,
    unread_only: bool,
    highlight: Vec<String>,
    hide: Vec<String>,
) -> Result<Vec<postal_core::store::StoredMessage>, String> {
    let service = state.account_service(&account_id)?;
    let rows = service
        .keyword_matches(chat.as_deref(), unread_only, &highlight, &hide)
        .await
        .map_err(|error| error.to_string())?;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) {
        return Err("account changed while finding keyword matches".into());
    }
    Ok(rows)
}
