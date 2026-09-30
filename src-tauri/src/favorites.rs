use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn favorite_chats(state: State<'_, AppState>, account: String) -> Result<Vec<String>, String> {
    state.account_service(&account)?.favorite_chats().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn set_favorite(state: State<'_, AppState>, account: String, chat: String, favorite: bool) -> Result<(), String> {
    let service = state.account_service(&account)?;
    service.set_favorite(&chat, favorite).await.map_err(|error| command_error(&service, error))
}
