use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};
use std::sync::Arc;

fn operation_service(state: &AppState, account_id: Option<&str>) -> CommandResult<(String, Arc<postal_core::WhatsAppService>)> {
    let account = account_id.map(str::to_owned).or_else(|| crate::account_store::active_account(state))
        .ok_or_else(|| CommandError::code("error.not_connected"))?;
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    Ok((account, service))
}

fn operation_current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> CommandResult<()> {
    let current = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &current) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}

#[tauri::command]
pub(crate) async fn create_poll(
    state: State<'_, AppState>,
    chat: String,
    question: String,
    options: Vec<String>,
    multi: bool,
    account_id: Option<String>,
) -> CommandResult<()> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.create_poll(&chat, question.trim(), options, multi).await;
    operation_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn create_quiz(
    state: State<'_, AppState>, account_id: String, chat: String, question: String,
    options: Vec<String>, correct_index: usize,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.create_quiz(&chat, question.trim(), options, correct_index).await;
    operation_current(&state, &account_id, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn vote_poll(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    options: Vec<String>,
    account_id: Option<String>,
) -> CommandResult<()> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.vote_poll(&chat, &id, options).await;
    operation_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
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
    #[serde(default)]
    extra_guests_allowed: Option<bool>,
    #[serde(default)]
    is_scheduled_call: Option<bool>,
    #[serde(default)]
    has_reminder: Option<bool>,
    #[serde(default)]
    reminder_offset_sec: Option<i64>,
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
            extra_guests_allowed: event.extra_guests_allowed,
            is_scheduled_call: event.is_scheduled_call,
            has_reminder: event.has_reminder,
            reminder_offset_sec: event.reminder_offset_sec,
            invitation_id: None,
            invitation: false,
        }
    }
}

#[tauri::command]
pub(crate) async fn create_event(state: State<'_, AppState>, chat: String, event: EventForm, account_id: Option<String>) -> CommandResult<()> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.create_event(&chat, event.into()).await;
    operation_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Edits or cancels one of our events.
#[tauri::command]
pub(crate) async fn edit_event(state: State<'_, AppState>, chat: String, id: String, event: EventForm, account_id: Option<String>) -> CommandResult<()> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.edit_event(&chat, &id, event.into()).await;
    operation_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn respond_event(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    response: String,
    account_id: Option<String>,
    extra_guest_count: Option<i32>,
) -> CommandResult<()> {
    let (account, service) = operation_service(&state, account_id.as_deref())?;
    let result = service.respond_event(&chat, &id, &response, extra_guest_count).await;
    operation_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}
