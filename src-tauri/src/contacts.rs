use crate::command_error::{CommandError, CommandResult};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::State;
use crate::{AppState, settings::sends_privacy};

/// Resolves display names for chats that still show a raw number.
///
/// Returns how many were resolved; the UI refreshes when that is non-zero.
#[tauri::command]
pub(crate) async fn resolve_names(state: State<'_, AppState>) -> CommandResult<usize> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service.resolve_missing_names().await.map_err(CommandError::from)
}

/// Chats, contacts and groups matching a query.
#[tauri::command]
pub(crate) async fn search(
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<Vec<postal_core::SearchResult>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.search(&query).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn send_typing(state: State<'_, AppState>, chat: String, typing: bool) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    if typing && !sends_privacy(&state, &service, &chat).await.0 {
        return Ok(());
    }
    service.send_typing(&chat, typing).await.map_err(CommandError::from)
}

/// Every contact alias in the account, keyed by each address form of its
/// contact so the UI can look one up without knowing which form it holds.
#[tauri::command(async)]
pub(crate) async fn contact_aliases(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::HashMap<String, Vec<String>>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.all_aliases().await.map_err(CommandError::from)
}

/// Gives a contact a local alias so they can be addressed as `@alias`.
///
/// Fails when another contact already answers to it, so an alias always names
/// one person. Nothing is sent to the phone and no name is changed.
#[tauri::command(async)]
pub(crate) async fn add_contact_alias(state: State<'_, AppState>, jid: String, alias: String) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .add_alias(&jid, &alias)
        .await.map_err(CommandError::from)
}

/// Drops one of a contact's aliases.
#[tauri::command(async)]
pub(crate) async fn remove_contact_alias(
    state: State<'_, AppState>,
    jid: String,
    alias: String,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .remove_alias(&jid, &alias)
        .await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_online(state: State<'_, AppState>, online: bool) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_online(online).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn watch_presence(state: State<'_, AppState>, jid: String, account: Option<String>) -> CommandResult<()> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    service.watch_presence(&jid).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn profile(state: State<'_, AppState>) -> CommandResult<postal_core::Profile> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.profile().await.map_err(CommandError::from)
}

/// Sets our profile picture from base64 image bytes; an empty string removes it.
#[tauri::command]
pub(crate) async fn set_profile_picture(state: State<'_, AppState>, data: String) -> CommandResult<()> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.profile_picture_base64").with_diagnostic(error))?;
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_own_picture(bytes).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_about(state: State<'_, AppState>, text: String) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_about(&text).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_push_name(state: State<'_, AppState>, name: String) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_push_name(&name).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_privacy(
    state: State<'_, AppState>,
    category: String,
    value: String,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_privacy(&category, &value)
        .await
        .map_err(CommandError::from)
}

/// A chat's cached profile picture path, if it has one; `full` for the full-size one.
#[tauri::command]
pub(crate) async fn avatar(state: State<'_, AppState>, jid: String, full: Option<bool>) -> CommandResult<Option<String>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.avatar(&jid, full.unwrap_or(false)).await.map_err(CommandError::from)
}

/// Best known names for JIDs, keyed by the JID as given.
#[tauri::command]
pub(crate) async fn names(state: State<'_, AppState>, jids: Vec<String>, account: Option<String>,
) -> CommandResult<std::collections::HashMap<String, String>> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    Ok(service.names_for(&jids).await)
}

#[tauri::command]
pub(crate) async fn contact_identities(state: State<'_, AppState>, account: String, jids: Vec<String>)
    -> CommandResult<std::collections::HashMap<String, postal_core::store::contact_identity::ContactIdentity>> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.contact_identities(&jids).await.map_err(CommandError::from)
}
