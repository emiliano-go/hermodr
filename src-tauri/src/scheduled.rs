use std::sync::Arc;
use postal_core::{WhatsAppService, store::scheduled::ScheduledMessage};
use tauri::State;
use crate::{AppState, connection::command_error};

impl AppState {
    pub(crate) fn account_service(&self, account: &str) -> Result<Arc<WhatsAppService>, String> {
        let binding = self.account_service.lock().unwrap();
        let service = binding.as_ref().filter(|(id, _)| id == account)
            .and_then(|(_, service)| service.upgrade()).ok_or("account changed before operation")?;
        let current = self.service()?;
        if self.accounts.lock().unwrap().active.as_deref() != Some(account) || !Arc::ptr_eq(&service, &current) {
            return Err("account changed before operation".into());
        }
        Ok(service)
    }
}

#[tauri::command]
pub(crate) async fn schedule_message(state: State<'_, AppState>, account: String, chat: String,
    text: String, mentions: Option<Vec<String>>, due_at: i64) -> Result<String, String> {
    state.account_service(&account)?.schedule_message(&chat, text, mentions.unwrap_or_default(), due_at)
        .await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn scheduled_messages(state: State<'_, AppState>, account: String) -> Result<Vec<ScheduledMessage>, String> {
    state.account_service(&account)?.scheduled_messages().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn update_scheduled_message(state: State<'_, AppState>, account: String,
    id: String, text: String, due_at: i64) -> Result<(), String> {
    state.account_service(&account)?.update_scheduled_message(id, text, due_at).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn cancel_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> Result<(), String> {
    state.account_service(&account)?.cancel_scheduled_message(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn retry_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> Result<(), String> {
    state.account_service(&account)?.retry_scheduled_message(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn send_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> Result<bool, String> {
    let service = state.account_service(&account)?;
    service.send_scheduled_message(id).await.map_err(|error| command_error(&service, error))
}
