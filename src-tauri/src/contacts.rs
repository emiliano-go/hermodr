use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::State;
use crate::{AppState, settings::sends_privacy};

/// Resolves display names for chats that still show a raw number.
///
/// Returns how many were resolved; the UI refreshes when that is non-zero.
#[tauri::command]
pub(crate) async fn resolve_names(state: State<'_, AppState>) -> Result<usize, String> {
    let service = state.service()?;
    service.resolve_missing_names().await.map_err(|e| e.to_string())
}

/// Chats, contacts and groups matching a query.
#[tauri::command]
pub(crate) async fn search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<postal_core::SearchResult>, String> {
    state.service()?.search(&query).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn send_typing(state: State<'_, AppState>, chat: String, typing: bool) -> Result<(), String> {
    let service = state.service()?;
    if typing && !sends_privacy(&state, &service, &chat).await.0 {
        return Ok(());
    }
    service.send_typing(&chat, typing).await.map_err(|e| e.to_string())
}

/// Every contact alias in the account, keyed by each address form of its
/// contact so the UI can look one up without knowing which form it holds.
#[tauri::command(async)]
pub(crate) async fn contact_aliases(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Vec<String>>, String> {
    state.service()?.all_aliases().await.map_err(|e| e.to_string())
}

/// Gives a contact a local alias so they can be addressed as `@alias`.
///
/// Fails when another contact already answers to it, so an alias always names
/// one person. Nothing is sent to the phone and no name is changed.
#[tauri::command(async)]
pub(crate) async fn add_contact_alias(state: State<'_, AppState>, jid: String, alias: String) -> Result<(), String> {
    state
        .service()?
        .add_alias(&jid, &alias)
        .await.map_err(|e| e.to_string())
}

/// Drops one of a contact's aliases.
#[tauri::command(async)]
pub(crate) async fn remove_contact_alias(
    state: State<'_, AppState>,
    jid: String,
    alias: String,
) -> Result<(), String> {
    state
        .service()?
        .remove_alias(&jid, &alias)
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_online(state: State<'_, AppState>, online: bool) -> Result<(), String> {
    state.service()?.set_online(online).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn watch_presence(state: State<'_, AppState>, jid: String) -> Result<(), String> {
    state.service()?.watch_presence(&jid).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn profile(state: State<'_, AppState>) -> Result<postal_core::Profile, String> {
    state.service()?.profile().await.map_err(|e| e.to_string())
}

/// Sets our profile picture from base64 image bytes; an empty string removes it.
#[tauri::command]
pub(crate) async fn set_profile_picture(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    state.service()?.set_own_picture(bytes).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_about(state: State<'_, AppState>, text: String) -> Result<(), String> {
    state.service()?.set_about(&text).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_push_name(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.service()?.set_push_name(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_privacy(
    state: State<'_, AppState>,
    category: String,
    value: String,
) -> Result<(), String> {
    state
        .service()?
        .set_privacy(&category, &value)
        .await
        .map_err(|e| e.to_string())
}

/// A chat's cached profile picture path, if it has one; `full` for the full-size one.
#[tauri::command]
pub(crate) async fn avatar(state: State<'_, AppState>, jid: String, full: Option<bool>) -> Result<Option<String>, String> {
    state.service()?.avatar(&jid, full.unwrap_or(false)).await.map_err(|e| e.to_string())
}

/// Someone's profile card: names, number, username, about.
#[tauri::command]
pub(crate) async fn user_profile(state: State<'_, AppState>, jid: String) -> Result<postal_core::UserProfile, String> {
    state.service()?.user_profile(&jid).await.map_err(|e| e.to_string())
}

/// Best known names for JIDs, keyed by the JID as given.
#[tauri::command]
pub(crate) async fn names(state: State<'_, AppState>, jids: Vec<String>, account: Option<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let service = match account { Some(account) => state.account_service(&account)?, None => state.service()? };
    Ok(service.names_for(&jids).await)
}

#[tauri::command]
pub(crate) async fn contact_identities(state: State<'_, AppState>, account: String, jids: Vec<String>)
    -> Result<std::collections::HashMap<String, postal_core::store::contact_identity::ContactIdentity>, String> {
    state.account_service(&account)?.contact_identities(&jids).await.map_err(|error| error.to_string())
}
