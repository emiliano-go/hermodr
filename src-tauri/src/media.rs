use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use postal_core::{SendOptions, VoiceNote};
use postal_core::{Sticker, StickerLibrary, StickerResyncReport};
use tauri::State;
use crate::AppState;
use crate::connection::command_error;

fn sticker_service(state: &AppState, account_id: Option<&str>) -> Result<(String, std::sync::Arc<postal_core::WhatsAppService>), String> {
    let account = account_id.map(str::to_owned).or_else(|| crate::account_store::active_account(state)).ok_or("No active sticker account.")?;
    Ok((account.clone(), state.account_service(&account)?))
}

fn sticker_current(state: &AppState, account: &str, expected: &std::sync::Arc<postal_core::WhatsAppService>) -> Result<(), String> {
    let service = state.account_service(account)?;
    if !std::sync::Arc::ptr_eq(&service, expected) { return Err("Sticker account changed.".into()); }
    Ok(())
}

#[tauri::command(async)]
pub(crate) async fn storage_report(
    state: State<'_, AppState>, chat: Option<String>,
    order: Option<postal_core::service::StorageOrder>, offset: Option<usize>,
) -> Result<postal_core::service::StorageReport, String> {
    state.service()?.storage_report(chat.as_deref(), order.unwrap_or_default(), offset.unwrap_or(0))
        .await.map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub(crate) async fn storage_cleanup(
    state: State<'_, AppState>, action: postal_core::service::StorageCleanup,
) -> Result<postal_core::service::CleanupResult, String> {
    state.service()?.cleanup_storage(action).await.map_err(|error| error.to_string())
}

/// Sends an attachment as an image or document.
///
/// Small files arrive as base64; large files name an account-owned staged upload.
/// The renderer never chooses the source filesystem path.
#[tauri::command]
pub(crate) async fn send_media(
    state: State<'_, AppState>,
    chat: String,
    name: String,
    data: Option<String>,
    upload: Option<String>,
    caption: Option<String>,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
    gif: Option<bool>,
    view_once: Option<bool>,
    mentions: Option<Vec<String>>,
    progress: Option<String>,
    quality: Option<postal_core::MediaQuality>,
) -> Result<Option<String>, String> {
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        gif: gif.unwrap_or(false),
        view_once: view_once.unwrap_or(false),
        mentions: mentions.unwrap_or_default(),
        progress,
        quality,
        ..Default::default()
    };
    let service = state.service()?;
    match (data, upload) {
        (Some(data), None) => {
            let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
            service.send_media(&chat, &name, bytes, caption, reply, options).await.map_err(|e| command_error(&service, e))
        }
        (None, Some(token)) => {
            let owner = crate::account_store::active_account(&state).ok_or("no active account")?;
            let uploads = state.uploads.clone();
            let staged = tauri::async_runtime::spawn_blocking(move || uploads.take(&owner, &token)).await.map_err(|e| e.to_string())??;
            service.send_media_file(&chat, &staged.name, staged.path.clone(), caption, reply, options).await.map_err(|e| command_error(&service, e))
        }
        _ => Err("provide either attachment bytes or a staged upload".into()),
    }
}

/// Sends a voice note recorded by the webview (WebM/Opus, base64).
#[tauri::command]
pub(crate) async fn send_voice(
    state: State<'_, AppState>,
    chat: String,
    data: String,
    seconds: u32,
    waveform: Vec<u8>,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
    view_once: Option<bool>,
) -> Result<(), String> {
    let webm = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    let ogg = postal_core::ogg::webm_to_ogg(&webm).map_err(|e| e.to_string())?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        view_once: view_once.unwrap_or(false),
        voice: Some(VoiceNote { seconds, waveform }),
        ..Default::default()
    };
    let service = state.service()?;
    service
        .send_media(&chat, "voice.ogg", ogg, None, reply, options)
        .await
        .map(|_| ())
        .map_err(|e| command_error(&service, e))
}

/// Sends base64 image bytes as a sticker.
#[tauri::command]
pub(crate) async fn send_sticker(
    state: State<'_, AppState>,
    chat: String,
    data: String,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
) -> Result<(), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    let reply = reply_of(reply_to_id, reply_to_sender, reply_to_text);
    let service = state.service()?;
    service.send_sticker(&chat, bytes, reply).await.map_err(|e| command_error(&service, e))
}

/// The quoted `(id, sender, text)` a send carries, when all three were given.
pub(crate) fn reply_of(id: Option<String>, sender: Option<String>, text: Option<String>) -> Option<(String, String, String)> {
    Some((id?, sender?, text.unwrap_or_default()))
}

/// Saves base64 image bytes as a sticker without sending it; returns its path.
#[tauri::command(async)]
pub(crate) fn save_sticker(state: State<'_, AppState>, data: String) -> Result<String, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    state.service()?.save_sticker(&bytes).map_err(|e| e.to_string())
}

/// Stickers or GIFs already downloaded, newest first.
#[tauri::command(async)]
pub(crate) async fn media_library(
    state: State<'_, AppState>,
    kind: String,
    prefer: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    state
        .service()?
        .media_library(&kind, &prefer.unwrap_or_default())
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn send_from_library(
    state: State<'_, AppState>,
    chat: String,
    path: String,
    kind: String,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .send_from_library(&chat, &path, &kind, reply_of(reply_to_id, reply_to_sender, reply_to_text))
        .await
        .map_err(|e| command_error(&service, e))
}

/// Packs, favourites and recents of the sticker library.
#[tauri::command(async)]
pub(crate) async fn sticker_library(state: State<'_, AppState>, account_id: Option<String>) -> Result<StickerLibrary, String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.sticker_library().await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// The stickers in one pack, favourite-first.
#[tauri::command(async)]
pub(crate) async fn sticker_pack(state: State<'_, AppState>, pack: String, account_id: Option<String>) -> Result<Vec<Sticker>, String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.stickers_in_pack(&pack).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Favourites or unfavourites a sticker by its filehash.
#[tauri::command(async)]
pub(crate) async fn favorite_sticker(state: State<'_, AppState>, filehash: String, favorite: bool, account_id: Option<String>) -> Result<(), String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.set_sticker_favorite(&filehash, favorite).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Favourites or unfavourites a sticker that only exists as a file path.
#[tauri::command(async)]
pub(crate) async fn favorite_sticker_path(app: tauri::AppHandle, state: State<'_, AppState>, path: String, favorite: bool, account_id: Option<String>) -> Result<(), String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let path = crate::media_access::readable_file(&app, &state, &path)?;
    let result = service.favorite_sticker_path(&path.to_string_lossy(), favorite).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Marks a sticker recently sent, or removes it from recents.
#[tauri::command(async)]
pub(crate) async fn sticker_recent(state: State<'_, AppState>, filehash: String, recent: bool, account_id: Option<String>) -> Result<(), String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.set_sticker_recent(&filehash, recent).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Fetches a received pack's contents and records them.
#[tauri::command(async)]
pub(crate) async fn fetch_sticker_pack(state: State<'_, AppState>, pack: String, account_id: Option<String>) -> Result<usize, String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.fetch_sticker_pack(&pack).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Downloads a pack sticker, returning its local path.
#[tauri::command(async)]
pub(crate) async fn download_sticker(state: State<'_, AppState>, filehash: String, account_id: Option<String>) -> Result<String, String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.download_sticker(&filehash).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Re-fetches known packs and resyncs sticker app-state; upsert-only.
#[tauri::command(async)]
pub(crate) async fn resync_stickers(state: State<'_, AppState>, account_id: Option<String>) -> Result<StickerResyncReport, String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.resync_stickers().await;
    sticker_current(&state, &account, &service)?;
    result.map_err(|e| e.to_string())
}

/// Media extensions the renderer may read.
pub(crate) const READABLE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "mp4", "mov", "m4v", "webm", "mkv", "ogg", "opus", "mp3",
    "m4a", "aac", "wav",
];

/// Reads an authorized media file for WebKitGTK's blob playback fallback.
#[tauri::command(async)]
pub(crate) fn read_file(app: tauri::AppHandle, state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target = crate::media_access::readable_file(&app, &state, &path)?;
    let bytes = std::fs::read(target).map_err(|e| e.to_string())?;
    Ok(BASE64.encode(bytes))
}

/// A path the web view can play: the source itself when it is already PCM
/// WAV, otherwise a WAV converted from it.
#[tauri::command]
pub(crate) async fn playable_audio(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let service = state.service()?;
    service.playable_audio(&path).await.map_err(|e| command_error(&service, e))
}

/// A path the web view can play a video from: the original when its codecs are
/// decodable, otherwise a cached H.264 + Opus remux ffmpeg writes.
#[tauri::command]
pub(crate) async fn playable_video(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let service = state.service()?;
    service.playable_video(&path).await.map_err(|e| command_error(&service, e))
}

/// Deletes downloaded media, keeping the messages.
#[tauri::command(async)]
pub(crate) async fn flush_media(state: State<'_, AppState>) -> Result<usize, String> {
    state.service()?.flush_media().await.map_err(|e| e.to_string())
}

/// Downloads a message's media on demand.
#[tauri::command]
pub(crate) async fn download_media(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> Result<(), String> {
    state
        .service()?
        .download_media(&chat, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Takes back the view-once a reply quotes, when this account sent it.
#[tauri::command]
pub(crate) async fn recover_quote_media(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> Result<(), String> {
    state
        .service()?
        .recover_quote_media(&chat, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Marks a view-once message opened and deletes its file.
#[tauri::command(async)]
pub(crate) async fn open_view_once(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    state.service()?.open_view_once(&chat, &id).await.map_err(|e| e.to_string())
}
