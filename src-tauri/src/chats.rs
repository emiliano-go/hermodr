use postal_core::ChatSummary;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

/// Chat summaries, most recently active first.
#[tauri::command(async)]
pub(crate) async fn chats(state: State<'_, AppState>, mute_all_at_all: Option<bool>) -> CommandResult<Vec<ChatSummary>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.chats_with(mute_all_at_all.unwrap_or(false)).await.map_err(CommandError::from)
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct ChatSettings {
    /// The chat's auto download override, `None` when it follows the global one.
    auto_download: Option<bool>,
    auto_download_types: postal_core::store::media_policy::MediaAutoDownloadOverrides,
    sound_muted: Option<bool>,
    unarchive: Option<bool>,
    retention: postal_core::ChatRetention,
    /// Typing and read receipt overrides, `None` when following the global ones.
    send_typing: Option<bool>,
    send_receipts: Option<bool>,
    /// Whether @all mentions stay silent in this chat.
    mute_at_all: bool,
}

#[tauri::command(async)]
pub(crate) async fn chat_settings(state: State<'_, AppState>, chat: String) -> CommandResult<ChatSettings> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let (send_typing, send_receipts) = service.chat_privacy(&chat).await.map_err(CommandError::from)?;
    Ok(ChatSettings {
        send_typing,
        send_receipts,
        auto_download: service.chat_auto_download(&chat).await.map_err(CommandError::from)?,
        auto_download_types: service.chat_media_auto_download(&chat).await.map_err(CommandError::from)?,
        sound_muted: service.chat_sound_muted(&chat).await.map_err(CommandError::from)?,
        unarchive: service.chat_unarchive(&chat).await.map_err(CommandError::from)?,
        retention: service.chat_retention(&chat).await.map_err(CommandError::from)?,
        mute_at_all: service.chat_mute_at_all(&chat).await.map_err(CommandError::from)?,
    })
}

/// Mutes or unmutes @all mentions in one chat; direct mentions still ping.
#[tauri::command(async)]
pub(crate) async fn set_chat_mute_at_all(
    state: State<'_, AppState>,
    chat: String,
    muted: bool,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_chat_mute_at_all(&chat, muted)
        .await
        .map_err(CommandError::from)
}

#[tauri::command(async)]
pub(crate) async fn set_chat_retention(
    state: State<'_, AppState>,
    chat: String,
    retention: postal_core::ChatRetention,
) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_chat_retention(&chat, &retention).await.map_err(CommandError::from)
}

/// Pins or unpins a chat, mirroring it to the account.
#[tauri::command]
pub(crate) async fn set_pinned(state: State<'_, AppState>, chat: String, pinned: bool) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_pinned(&chat, pinned)
        .await
        .map_err(CommandError::from)
}

/// Archives or unarchives a chat on the account.
#[tauri::command]
pub(crate) async fn set_archived(state: State<'_, AppState>, chat: String, archived: bool, account: Option<String>) -> CommandResult<()> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    service.set_archived(&chat, archived).await.map_err(CommandError::from)
}

/// Mutes a chat until `until` (Unix seconds; -1 indefinitely, 0 unmutes).
#[tauri::command]
pub(crate) async fn set_muted(state: State<'_, AppState>, chat: String, until: i64, account: Option<String>) -> CommandResult<()> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    service.set_muted(&chat, until).await.map_err(CommandError::from)
}

/// Sets or lifts a chat's manual unread mark on the account.
#[tauri::command]
pub(crate) async fn set_marked_unread(state: State<'_, AppState>, chat: String, unread: bool, account: Option<String>) -> CommandResult<()> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    service.set_marked_unread(&chat, unread).await.map_err(CommandError::from)
}

/// Deletes every message stored on this device; the phone keeps its copy.
#[tauri::command(async)]
pub(crate) async fn clear_history(state: State<'_, AppState>) -> CommandResult<usize> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.clear_history().await.map_err(CommandError::from)
}

/// Clears one chat on this device only: its messages go, the empty chat stays.
/// Never touches the phone or the other side.
#[tauri::command(async)]
pub(crate) async fn clear_chat(state: State<'_, AppState>, chat: String) -> CommandResult<usize> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.clear_chat(&chat).await.map_err(CommandError::from)
}

/// Deletes one chat on this device only: its messages go and it leaves the
/// list until a new message arrives. Never touches the phone or the other side.
#[tauri::command(async)]
pub(crate) async fn delete_chat(state: State<'_, AppState>, chat: String) -> CommandResult<usize> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.delete_chat(&chat).await.map_err(CommandError::from)
}

/// Sets a chat's auto download override.
#[tauri::command(async)]
pub(crate) async fn set_chat_auto_download(
    state: State<'_, AppState>,
    chat: String,
    enabled: bool,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_chat_auto_download(&chat, enabled)
        .await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn chat_media_auto_download(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> CommandResult<postal_core::store::media_policy::MediaAutoDownloadOverrides> {
    state.service_for_account(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.chat_media_auto_download(&chat).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_chat_media_auto_download(
    state: State<'_, AppState>, account_id: String, chat: String,
    overrides: postal_core::store::media_policy::MediaAutoDownloadOverrides,
) -> CommandResult<()> {
    state.service_for_account(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.set_chat_media_auto_download(&chat, overrides).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn chat_unarchive(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> CommandResult<Option<bool>> {
    state.service_for_account(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.chat_unarchive(&chat).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_chat_unarchive(
    state: State<'_, AppState>, account_id: String, chat: String, enabled: Option<bool>,
) -> CommandResult<()> {
    state.service_for_account(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.set_chat_unarchive(&chat, enabled).await.map_err(CommandError::from)
}

/// Sets a chat's typing and read receipt overrides; `None` follows the global setting.
#[tauri::command(async)]
pub(crate) async fn set_chat_privacy(
    state: State<'_, AppState>,
    chat: String,
    typing: Option<bool>,
    receipts: Option<bool>,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_chat_privacy(&chat, typing, receipts)
        .await.map_err(CommandError::from)
}
