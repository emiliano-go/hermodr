use crate::{command_error::{CommandError, CommandResult}, AppState};
use postal_core::message_ref::MessageRef;
use std::sync::Arc;
use tauri::State;

fn current(
    state: &AppState,
    account: &str,
    expected: &Arc<postal_core::WhatsAppService>,
) -> anyhow::Result<()> {
    let service = state.account_service(account).map_err(|error| anyhow::Error::new(MessageRef::new("error.account_changed")).context(error))?;
    if Arc::ptr_eq(expected, &service) && service.is_connected() {
        Ok(())
    } else {
        Err(anyhow::Error::new(MessageRef::new("error.account_changed")))
    }
}

#[tauri::command]
pub(crate) async fn own_contact_link(
    state: State<'_, AppState>,
    account: String,
) -> CommandResult<String> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service
        .own_contact_link(|| {
            current(&state, &account, &service)
        })
        .await;
    current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn resolve_contact_link(
    state: State<'_, AppState>,
    account: String,
    link: String,
) -> CommandResult<String> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service
        .resolve_contact_link(&link, || {
            current(&state, &account, &service)
        })
        .await;
    current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn send_contacts(
    state: State<'_, AppState>,
    account: String,
    chat: String,
    contacts: Vec<(String, String)>,
) -> CommandResult<postal_core::ContactSendResult> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service
        .send_contacts(&chat, &contacts, || {
            current(&state, &account, &service)
        })
        .await;
    if current(&state, &account, &service).is_err() {
        return Err(CommandError::code("error.contact_send_account_changed"));
    }
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn message_contacts(
    state: State<'_, AppState>,
    account: String,
    chat: String,
    id: String,
    reveal_spoiler: Option<bool>,
) -> CommandResult<Vec<(String, String)>> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service
        .message_contacts(&chat, &id, reveal_spoiler.unwrap_or(false))
        .await;
    let active = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &active) {
        return Err(CommandError::code("error.account_changed"));
    }
    result.map_err(CommandError::from)
}
