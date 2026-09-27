use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use postal_core::{SendOptions, VoiceNote};
use tauri::State;
use crate::AppState;

#[tauri::command(async)]
pub(crate) fn storage_report(
    state: State<'_, AppState>, chat: Option<String>,
    order: Option<postal_core::service::StorageOrder>, offset: Option<usize>,
) -> Result<postal_core::service::StorageReport, String> {
    state.service()?.storage_report(chat.as_deref(), order.unwrap_or_default(), offset.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub(crate) fn storage_cleanup(
    state: State<'_, AppState>, action: postal_core::service::StorageCleanup,
) -> Result<postal_core::service::CleanupResult, String> {
    state.service()?.cleanup_storage(action).map_err(|error| error.to_string())
}

/// Sends an attachment as an image or document.
///
/// The file arrives base64-encoded because the webview cannot hand out a real
/// filesystem path, and the plugin that could is not usable alongside the
/// pinned Tauri checkout.
#[tauri::command]
pub(crate) async fn send_media(
    state: State<'_, AppState>,
    chat: String,
    name: String,
    data: String,
    caption: Option<String>,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
    gif: Option<bool>,
    view_once: Option<bool>,
    mentions: Option<Vec<String>>,
    progress: Option<String>,
) -> Result<Option<String>, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        gif: gif.unwrap_or(false),
        view_once: view_once.unwrap_or(false),
        mentions: mentions.unwrap_or_default(),
        progress,
        ..Default::default()
    };
    let service = state.service()?;
    service
        .send_media(&chat, &name, bytes, caption, reply, options)
        .await
        .map_err(|e| e.to_string())
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
    state
        .service()?
        .send_media(&chat, "voice.ogg", ogg, None, reply, options)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
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
    state.service()?.send_sticker(&chat, bytes, reply).await.map_err(|e| e.to_string())
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
pub(crate) fn media_library(
    state: State<'_, AppState>,
    kind: String,
    prefer: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    state
        .service()?
        .media_library(&kind, &prefer.unwrap_or_default())
        .map_err(|e| e.to_string())
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
    state
        .service()?
        .send_from_library(&chat, &path, &kind, reply_of(reply_to_id, reply_to_sender, reply_to_text))
        .await
        .map_err(|e| e.to_string())
}

/// Media extensions the renderer may read.
pub(crate) const READABLE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "mp4", "mov", "m4v", "webm", "mkv", "ogg", "opus", "mp3",
    "m4a", "aac", "wav",
];

/// Reads a media file and returns it base64-encoded.
///
/// WebKitGTK's media pipeline cannot load the custom asset scheme, so audio and
/// video have to arrive as bytes and be turned into a blob URL by the page. The
/// extension allowlist keeps this from becoming a general file-read primitive,
/// which matters because a pasted file can live anywhere on disk.
#[tauri::command]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    let extension = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !READABLE_EXTENSIONS.contains(&extension.as_str()) {
        return Err("unsupported file type".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(BASE64.encode(bytes))
}

/// Deletes downloaded media, keeping the messages.
#[tauri::command(async)]
pub(crate) fn flush_media(state: State<'_, AppState>) -> Result<usize, String> {
    state.service()?.flush_media().map_err(|e| e.to_string())
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
pub(crate) fn open_view_once(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    state.service()?.open_view_once(&chat, &id).map_err(|e| e.to_string())
}
