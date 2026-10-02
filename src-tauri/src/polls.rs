use tauri::State;
use crate::AppState;
use std::sync::Arc;

fn operation_service(state: &AppState, account_id: Option<&str>) -> Result<(String, Arc<postal_core::WhatsAppService>), String> {
    let account = account_id.map(str::to_owned).or_else(|| crate::account_store::active_account(state))
        .ok_or("no active account")?;
    let service = state.account_service(&account)?;
    Ok((account, service))
}

fn operation_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> Result<(), String> {
    let current = state.account_service(account)?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err("account changed during operation".into()) }
}

#[tauri::command]
pub(crate) async fn create_poll(
    state: State<'_, AppState>,
    chat: String,
    question: String,
    options: Vec<String>,
    multi: bool,
    account_id: Option<String>,
) -> Result<(), String> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.create_poll(&chat, question.trim(), options, multi).await;
    operation_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn create_quiz(
    state: State<'_, AppState>, account_id: String, chat: String, question: String,
    options: Vec<String>, correct_index: usize,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    let result = service.create_quiz(&chat, question.trim(), options, correct_index).await;
    operation_current(&state, &account_id, &service)?;
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn vote_poll(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    options: Vec<String>,
    account_id: Option<String>,
) -> Result<(), String> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.vote_poll(&chat, &id, options).await;
    operation_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// An event as the create dialog fills it; times are Unix seconds.
#[derive(serde::Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct EventForm {
    name: String,
    description: Option<String>,
    start: Option<i64>,
    end: Option<i64>,
    location: Option<String>,
    link: Option<String>,
    #[serde(default)]
    canceled: bool,
}

impl From<EventForm> for postal_core::NewEvent {
    fn from(event: EventForm) -> Self {
        Self {
            name: event.name.trim().to_string(),
            description: event.description.filter(|s| !s.trim().is_empty()),
            start: event.start,
            end: event.end,
            location: event.location.filter(|s| !s.trim().is_empty()),
            link: event.link.filter(|s| !s.trim().is_empty()),
            canceled: event.canceled,
        }
    }
}

#[tauri::command]
pub(crate) async fn create_event(state: State<'_, AppState>, chat: String, event: EventForm, account_id: Option<String>) -> Result<(), String> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.create_event(&chat, event.into()).await;
    operation_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Edits or cancels one of our events.
#[tauri::command]
pub(crate) async fn edit_event(state: State<'_, AppState>, chat: String, id: String, event: EventForm) -> Result<(), String> {
    state.service()?.edit_event(&chat, &id, event.into()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn respond_event(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    response: String,
) -> Result<(), String> {
    state
        .service()?
        .respond_event(&chat, &id, &response)
        .await
        .map_err(|e| e.to_string())
}
