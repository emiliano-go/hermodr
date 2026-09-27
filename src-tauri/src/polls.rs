use tauri::State;
use crate::AppState;

#[tauri::command]
pub(crate) async fn create_poll(
    state: State<'_, AppState>,
    chat: String,
    question: String,
    options: Vec<String>,
    multi: bool,
) -> Result<(), String> {
    state
        .service()?
        .create_poll(&chat, question.trim(), options, multi)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn vote_poll(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    options: Vec<String>,
) -> Result<(), String> {
    state.service()?.vote_poll(&chat, &id, options).await.map_err(|e| e.to_string())
}

/// An event as the create dialog fills it; times are Unix seconds.
#[derive(serde::Deserialize)]
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
pub(crate) async fn create_event(state: State<'_, AppState>, chat: String, event: EventForm) -> Result<(), String> {
    state.service()?.create_event(&chat, event.into()).await.map_err(|e| e.to_string())
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
