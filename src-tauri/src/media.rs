use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use postal_core::{SendOptions, VoiceNote};
use postal_core::{Sticker, StickerLibrary, StickerResyncReport};
use tauri::State;
use crate::AppState;
use crate::command_error::{CommandError, CommandResult};

fn command_error(service: &postal_core::WhatsAppService, error: anyhow::Error) -> CommandError {
    service.note_error(&error);
    error.into()
}

fn media_service(state: &AppState) -> CommandResult<std::sync::Arc<postal_core::WhatsAppService>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))
}

fn sticker_service(state: &AppState, account_id: Option<&str>) -> CommandResult<(String, std::sync::Arc<postal_core::WhatsAppService>)> {
    let account = account_id.map(str::to_owned).or_else(|| crate::account_store::active_account(state)).ok_or_else(|| CommandError::code("error.no_active_account"))?;
    Ok((account.clone(), state.account_service(&account).map_err(|error| CommandError::code("error.sticker_account_unavailable").with_diagnostic(error))?))
}

fn sticker_current(state: &AppState, account: &str, expected: &std::sync::Arc<postal_core::WhatsAppService>) -> CommandResult<()> {
    let service = state.account_service(account).map_err(|error| CommandError::code("error.sticker_account_unavailable").with_diagnostic(error))?;
    if !std::sync::Arc::ptr_eq(&service, expected) { return Err(CommandError::code("error.sticker_account_changed")); }
    Ok(())
}

#[tauri::command(async)]
pub(crate) async fn storage_report(
    state: State<'_, AppState>, chat: Option<String>,
    order: Option<postal_core::service::StorageOrder>, offset: Option<usize>,
) -> CommandResult<postal_core::service::StorageReport> {
    media_service(&state)?.storage_report(chat.as_deref(), order.unwrap_or_default(), offset.unwrap_or(0))
        .await.map_err(CommandError::from)
}

#[tauri::command(async)]
pub(crate) async fn storage_cleanup(
    state: State<'_, AppState>, action: postal_core::service::StorageCleanup,
) -> CommandResult<postal_core::service::CleanupResult> {
    media_service(&state)?.cleanup_storage(action).await.map_err(CommandError::from)
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
) -> CommandResult<Option<String>> {
    postal_core::service::writable_target(&chat)?;
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
    let service = media_service(&state)?;
    match (data, upload) {
        (Some(data), None) => {
            let bytes = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
            service.send_media(&chat, &name, bytes, caption, reply, options).await.map_err(|e| command_error(&service, e))
        }
        (None, Some(token)) => {
            let owner = crate::account_store::active_account(&state).ok_or_else(|| CommandError::code("error.no_active_account"))?;
            let uploads = state.uploads.clone();
            let staged = tauri::async_runtime::spawn_blocking(move || uploads.take(&owner, &token)).await.map_err(|e| e.to_string())??;
            service.send_media_file(&chat, &staged.name, staged.path.clone(), caption, reply, options).await.map_err(|e| command_error(&service, e))
        }
        _ => Err(CommandError::code("error.media_input_required")),
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
) -> CommandResult<()> {
    postal_core::service::writable_target(&chat)?;
    let webm = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
    let ogg = postal_core::ogg::webm_to_ogg(&webm).map_err(|error| CommandError::code("error.voice_recording_invalid").with_diagnostic(error))?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        view_once: view_once.unwrap_or(false),
        voice: Some(VoiceNote { seconds, waveform }),
        ..Default::default()
    };
    let service = media_service(&state)?;
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
) -> CommandResult<()> {
    postal_core::service::writable_target(&chat)?;
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
    let reply = reply_of(reply_to_id, reply_to_sender, reply_to_text);
    let service = media_service(&state)?;
    service.send_sticker(&chat, bytes, reply).await.map_err(|e| command_error(&service, e))
}

/// The quoted `(id, sender, text)` a send carries, when all three were given.
pub(crate) fn reply_of(id: Option<String>, sender: Option<String>, text: Option<String>) -> Option<(String, String, String)> {
    Some((id?, sender?, text.unwrap_or_default()))
}

/// Saves base64 image bytes as a sticker without sending it; returns its path.
#[tauri::command(async)]
pub(crate) fn save_sticker(state: State<'_, AppState>, data: String) -> CommandResult<String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error| CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
    media_service(&state)?.save_sticker(&bytes).map_err(CommandError::from)
}

/// Stickers or GIFs already downloaded, newest first.
#[tauri::command(async)]
pub(crate) async fn media_library(
    state: State<'_, AppState>,
    kind: String,
    prefer: Option<Vec<String>>,
) -> CommandResult<Vec<String>> {
    media_service(&state)?
        .media_library(&kind, &prefer.unwrap_or_default())
        .await.map_err(CommandError::from)
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
) -> CommandResult<()> {
    postal_core::service::writable_target(&chat)?;
    let service = media_service(&state)?;
    service
        .send_from_library(&chat, &path, &kind, reply_of(reply_to_id, reply_to_sender, reply_to_text))
        .await
        .map_err(|e| command_error(&service, e))
}

/// Packs, favourites and recents of the sticker library.
#[tauri::command(async)]
pub(crate) async fn sticker_library(state: State<'_, AppState>, account_id: Option<String>) -> CommandResult<StickerLibrary> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.sticker_library().await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// The stickers in one pack, favourite-first.
#[tauri::command(async)]
pub(crate) async fn sticker_pack(state: State<'_, AppState>, pack: String, account_id: Option<String>) -> CommandResult<Vec<Sticker>> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.stickers_in_pack(&pack).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Favourites or unfavourites a sticker by its filehash.
#[tauri::command(async)]
pub(crate) async fn favorite_sticker(state: State<'_, AppState>, filehash: String, favorite: bool, account_id: Option<String>) -> CommandResult<()> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.set_sticker_favorite(&filehash, favorite).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Favourites or unfavourites a sticker that only exists as a file path.
#[tauri::command(async)]
pub(crate) async fn favorite_sticker_path(app: tauri::AppHandle, state: State<'_, AppState>, path: String, favorite: bool, account_id: Option<String>) -> CommandResult<()> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let path = crate::media_access::readable_file(&app, &state, &path)?;
    let result = service.favorite_sticker_path(&path.to_string_lossy(), favorite).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Marks a sticker recently sent, or removes it from recents.
#[tauri::command(async)]
pub(crate) async fn sticker_recent(state: State<'_, AppState>, filehash: String, recent: bool, account_id: Option<String>) -> CommandResult<()> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.set_sticker_recent(&filehash, recent).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Fetches a received pack's contents and records them.
#[tauri::command(async)]
pub(crate) async fn fetch_sticker_pack(state: State<'_, AppState>, pack: String, account_id: Option<String>) -> CommandResult<usize> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.fetch_sticker_pack(&pack).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Downloads a pack sticker, returning its local path.
#[tauri::command(async)]
pub(crate) async fn download_sticker(state: State<'_, AppState>, filehash: String, account_id: Option<String>) -> CommandResult<String> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.download_sticker(&filehash).await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Re-fetches known packs and resyncs sticker app-state; upsert-only.
#[tauri::command(async)]
pub(crate) async fn resync_stickers(state: State<'_, AppState>, account_id: Option<String>) -> CommandResult<StickerResyncReport> {
    let (account, service) = sticker_service(&state, account_id.as_deref())?;
    let result = service.resync_stickers().await;
    sticker_current(&state, &account, &service)?;
    result.map_err(CommandError::from)
}

/// Media extensions the renderer may read.
pub(crate) const READABLE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "mp4", "mov", "m4v", "webm", "mkv", "ogg", "opus", "mp3",
    "m4a", "aac", "wav",
];

/// Reads an authorized media file for WebKitGTK's blob playback fallback.
#[tauri::command(async)]
pub(crate) fn read_file(app: tauri::AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<String> {
    let target = crate::media_access::readable_file(&app, &state, &path)?;
    let bytes = std::fs::read(target).map_err(|e| e.to_string())?;
    Ok(BASE64.encode(bytes))
}

/// A path the web view can play: the source itself when it is already PCM
/// WAV, otherwise a WAV converted from it.
#[tauri::command]
pub(crate) async fn playable_audio(state: State<'_, AppState>, path: String) -> CommandResult<String> {
    let service = media_service(&state)?;
    service.playable_audio(&path).await.map_err(|e| command_error(&service, e))
}

/// A path the web view can play a video from: the original when its codecs are
/// decodable, otherwise a cached H.264 + Opus remux ffmpeg writes.
#[tauri::command]
pub(crate) async fn playable_video(state: State<'_, AppState>, path: String) -> CommandResult<String> {
    let service = media_service(&state)?;
    service.playable_video(&path).await.map_err(|e| command_error(&service, e))
}

/// Deletes downloaded media, keeping the messages.
#[tauri::command(async)]
pub(crate) async fn flush_media(state: State<'_, AppState>) -> CommandResult<usize> {
    media_service(&state)?.flush_media().await.map_err(CommandError::from)
}

/// Downloads a message's media on demand.
#[tauri::command]
pub(crate) async fn download_media(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> CommandResult<()> {
    media_service(&state)?
        .download_media(&chat, &id)
        .await
        .map_err(CommandError::from)
}

/// Takes back the view-once a reply quotes, when this account sent it.
#[tauri::command]
pub(crate) async fn recover_quote_media(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> CommandResult<()> {
    media_service(&state)?
        .recover_quote_media(&chat, &id)
        .await
        .map_err(CommandError::from)
}

/// Marks a view-once message opened and deletes its file.
#[tauri::command(async)]
pub(crate) async fn open_view_once(state: State<'_, AppState>, chat: String, id: String) -> CommandResult<()> {
    media_service(&state)?.open_view_once(&chat, &id).await.map_err(CommandError::from)
}
