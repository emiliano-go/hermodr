use crate::{connection::command_error, AppState};
use postal_core::{store::quick_replies::QuickRepliesView, WhatsAppService};
use std::sync::Arc;
use tauri::State;

fn current(state: &AppState, account: &str, expected: &Arc<WhatsAppService>) -> Result<(), String> {
    let service = state.account_service(account)?;
    if Arc::ptr_eq(expected, &service) {
        Ok(())
    } else {
        Err("Account changed during quick-reply operation.".into())
    }
}

#[tauri::command]
pub(crate) async fn quick_replies_view(
    state: State<'_, AppState>, account_id: String,
) -> Result<QuickRepliesView, String> {
    let service = state.account_service(&account_id)?;
    let result = service.quick_replies_view().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn sync_quick_replies(
    state: State<'_, AppState>, account_id: String,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    let result = service.sync_quick_replies(|| {
        current(&state, &account_id, &service).map_err(anyhow::Error::msg)
    }).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| command_error(&service, error))
}
