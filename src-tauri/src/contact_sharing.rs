use crate::{connection::command_error, AppState};
use std::sync::Arc;
use tauri::State;

fn current(
    state: &AppState,
    account: &str,
    expected: &Arc<postal_core::WhatsAppService>,
) -> Result<(), String> {
    let service = state.account_service(account)?;
    if Arc::ptr_eq(expected, &service) && service.is_connected() {
        Ok(())
    } else {
        Err("Account changed or disconnected during contact operation.".into())
    }
}

#[tauri::command]
pub(crate) async fn own_contact_link(
    state: State<'_, AppState>,
    account: String,
) -> Result<String, String> {
    let service = state.account_service(&account)?;
    let result = service
        .own_contact_link(|| {
            current(&state, &account, &service).map_err(|error| std::io::Error::other(error).into())
        })
        .await;
    current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn resolve_contact_link(
    state: State<'_, AppState>,
    account: String,
    link: String,
) -> Result<String, String> {
    let service = state.account_service(&account)?;
    let result = service
        .resolve_contact_link(&link, || {
            current(&state, &account, &service).map_err(|error| std::io::Error::other(error).into())
        })
        .await;
    current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn send_contacts(
    state: State<'_, AppState>,
    account: String,
    chat: String,
    contacts: Vec<(String, String)>,
) -> Result<postal_core::ContactSendResult, String> {
    let service = state.account_service(&account)?;
    let result = service
        .send_contacts(&chat, &contacts, || {
            current(&state, &account, &service).map_err(|error| std::io::Error::other(error).into())
        })
        .await;
    if current(&state, &account, &service).is_err() {
        return Err(
            "Account changed during contact send; check the original chat before retrying.".into(),
        );
    }
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn message_contacts(
    state: State<'_, AppState>,
    account: String,
    chat: String,
    id: String,
    reveal_spoiler: Option<bool>,
) -> Result<Vec<(String, String)>, String> {
    let service = state.account_service(&account)?;
    let result = service
        .message_contacts(&chat, &id, reveal_spoiler.unwrap_or(false))
        .await;
    let active = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &active) {
        return Err("Account changed while loading contacts.".into());
    }
    result.map_err(|error| error.to_string())
}
