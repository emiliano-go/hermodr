use postal_core::ChatSummary;
use tauri::State;
use crate::AppState;

/// Chat summaries, most recently active first.
#[tauri::command(async)]
pub(crate) async fn chats(state: State<'_, AppState>) -> Result<Vec<ChatSummary>, String> {
    state.service()?.chats().await.map_err(|e| e.to_string())
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
}

#[tauri::command(async)]
pub(crate) async fn chat_settings(state: State<'_, AppState>, chat: String) -> Result<ChatSettings, String> {
    let service = state.service()?;
    let (send_typing, send_receipts) = service.chat_privacy(&chat).await.map_err(|e| e.to_string())?;
    Ok(ChatSettings {
        send_typing,
        send_receipts,
        auto_download: service.chat_auto_download(&chat).await.map_err(|e| e.to_string())?,
        auto_download_types: service.chat_media_auto_download(&chat).await.map_err(|e| e.to_string())?,
        sound_muted: service.chat_sound_muted(&chat).await.map_err(|e| e.to_string())?,
        unarchive: service.chat_unarchive(&chat).await.map_err(|e| e.to_string())?,
        retention: service.chat_retention(&chat).await.map_err(|e| e.to_string())?,
    })
}

#[tauri::command(async)]
pub(crate) async fn set_chat_retention(
    state: State<'_, AppState>,
    chat: String,
    retention: postal_core::ChatRetention,
) -> Result<(), String> {
    state.service()?.set_chat_retention(&chat, &retention).await.map_err(|e| e.to_string())
}

/// Pins or unpins a chat, mirroring it to the account.
#[tauri::command]
pub(crate) async fn set_pinned(state: State<'_, AppState>, chat: String, pinned: bool) -> Result<(), String> {
    state
        .service()?
        .set_pinned(&chat, pinned)
        .await
        .map_err(|e| e.to_string())
}

/// Archives or unarchives a chat on the account.
#[tauri::command]
pub(crate) async fn set_archived(state: State<'_, AppState>, chat: String, archived: bool, account: Option<String>) -> Result<(), String> {
    let service = match account { Some(account) => state.account_service(&account)?, None => state.service()? };
    service.set_archived(&chat, archived).await.map_err(|e| e.to_string())
}

/// Mutes a chat until `until` (Unix seconds; -1 indefinitely, 0 unmutes).
#[tauri::command]
pub(crate) async fn set_muted(state: State<'_, AppState>, chat: String, until: i64, account: Option<String>) -> Result<(), String> {
    let service = match account { Some(account) => state.account_service(&account)?, None => state.service()? };
    service.set_muted(&chat, until).await.map_err(|e| e.to_string())
}

/// Sets or lifts a chat's manual unread mark on the account.
#[tauri::command]
pub(crate) async fn set_marked_unread(state: State<'_, AppState>, chat: String, unread: bool, account: Option<String>) -> Result<(), String> {
    let service = match account { Some(account) => state.account_service(&account)?, None => state.service()? };
    service.set_marked_unread(&chat, unread).await.map_err(|e| e.to_string())
}

/// Deletes every message stored on this device; the phone keeps its copy.
#[tauri::command(async)]
pub(crate) async fn clear_history(state: State<'_, AppState>) -> Result<usize, String> {
    state.service()?.clear_history().await.map_err(|e| e.to_string())
}

/// Clears one chat on this device only: its messages go, the empty chat stays.
/// Never touches the phone or the other side.
#[tauri::command(async)]
pub(crate) async fn clear_chat(state: State<'_, AppState>, chat: String) -> Result<usize, String> {
    state.service()?.clear_chat(&chat).await.map_err(|e| e.to_string())
}

/// Deletes one chat on this device only: its messages go and it leaves the
/// list until a new message arrives. Never touches the phone or the other side.
#[tauri::command(async)]
pub(crate) async fn delete_chat(state: State<'_, AppState>, chat: String) -> Result<usize, String> {
    state.service()?.delete_chat(&chat).await.map_err(|e| e.to_string())
}

/// Sets a chat's auto download override.
#[tauri::command(async)]
pub(crate) async fn set_chat_auto_download(
    state: State<'_, AppState>,
    chat: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .service()?
        .set_chat_auto_download(&chat, enabled)
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn chat_media_auto_download(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> Result<postal_core::store::media_policy::MediaAutoDownloadOverrides, String> {
    state.service_for_account(&account_id)?.chat_media_auto_download(&chat).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_chat_media_auto_download(
    state: State<'_, AppState>, account_id: String, chat: String,
    overrides: postal_core::store::media_policy::MediaAutoDownloadOverrides,
) -> Result<(), String> {
    state.service_for_account(&account_id)?.set_chat_media_auto_download(&chat, overrides).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn chat_unarchive(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> Result<Option<bool>, String> {
    state.service_for_account(&account_id)?.chat_unarchive(&chat).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_chat_unarchive(
    state: State<'_, AppState>, account_id: String, chat: String, enabled: Option<bool>,
) -> Result<(), String> {
    state.service_for_account(&account_id)?.set_chat_unarchive(&chat, enabled).await.map_err(|e| e.to_string())
}

/// Sets a chat's typing and read receipt overrides; `None` follows the global setting.
#[tauri::command(async)]
pub(crate) async fn set_chat_privacy(
    state: State<'_, AppState>,
    chat: String,
    typing: Option<bool>,
    receipts: Option<bool>,
) -> Result<(), String> {
    state
        .service()?
        .set_chat_privacy(&chat, typing, receipts)
        .await.map_err(|e| e.to_string())
}
