use crate::{command_error::{CommandError, CommandResult}, AppState};
use postal_core::message_ref::MessageRef;
use postal_core::{store::quick_replies::QuickRepliesView, WhatsAppService};
use std::sync::Arc;
use tauri::State;

fn current(state: &AppState, account: &str, expected: &Arc<WhatsAppService>) -> anyhow::Result<()> {
    let service = state.account_service(account).map_err(|error| anyhow::Error::new(MessageRef::new("error.account_changed")).context(error))?;
    if Arc::ptr_eq(expected, &service) {
        Ok(())
    } else {
        Err(anyhow::Error::new(MessageRef::new("error.account_changed")))
    }
}

#[tauri::command]
pub(crate) async fn quick_replies_view(
    state: State<'_, AppState>, account_id: String,
) -> CommandResult<QuickRepliesView> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.quick_replies_view().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn sync_quick_replies(
    state: State<'_, AppState>, account_id: String,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.sync_quick_replies(|| {
        current(&state, &account_id, &service)
    }).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}
