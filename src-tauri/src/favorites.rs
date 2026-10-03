use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[tauri::command]
pub(crate) async fn favorite_chats(state: State<'_, AppState>, account: String) -> CommandResult<Vec<String>> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .favorite_chats().await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn set_favorite(state: State<'_, AppState>, account: String, chat: String, favorite: bool) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.set_favorite(&chat, favorite).await.map_err(|error| { service.note_error(&error); error.into() })
}
