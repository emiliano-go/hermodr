use crate::{connection::command_error, AppState};
use postal_core::{service::{AlbumMediaInput, AlbumSendResult, MAX_ALBUM_ITEMS}, MediaQuality, WhatsAppService};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct AlbumUploadItem {
    pub upload: String,
    pub caption: Option<String>,
    pub quality: Option<MediaQuality>,
    pub progress: Option<String>,
}

fn current(state: &AppState, account: &str, expected: &Arc<WhatsAppService>) -> anyhow::Result<()> {
    let service = state.account_service(account).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(Arc::ptr_eq(expected, &service) && service.is_connected(), "Album account changed or disconnected.");
    Ok(())
}

#[tauri::command]
pub(crate) async fn send_album(
    state: State<'_, AppState>, account_id: String, chat: String, items: Vec<AlbumUploadItem>,
    parent_id: Option<String>,
    reply_to_id: Option<String>, reply_to_sender: Option<String>, reply_to_text: Option<String>,
    mentions: Option<Vec<String>>,
) -> Result<AlbumSendResult, String> {
    Ok(send_album_inner(&state, &account_id, &chat, items, parent_id.as_deref(),
        reply_to_id, reply_to_sender, reply_to_text, mentions).await.unwrap_or_else(|error| {
            AlbumSendResult::preflight_failure(&account_id, &chat, parent_id.as_deref(), error)
        }))
}

async fn send_album_inner(
    state: &AppState, account_id: &str, chat: &str, items: Vec<AlbumUploadItem>, parent_id: Option<&str>,
    reply_to_id: Option<String>, reply_to_sender: Option<String>, reply_to_text: Option<String>, mentions: Option<Vec<String>>,
) -> Result<AlbumSendResult, String> {
    postal_core::service::writable_target(chat).map_err(|error| error.to_string())?;
    validate_uploads(&items, parent_id)?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (None, None, None) => None,
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => return Err("Provide a complete album reply target.".into()),
    };
    let service = state.account_service(account_id)?;
    current(state, account_id, &service).map_err(|error| error.to_string())?;
    let uploads = state.uploads.clone();
    let owner = account_id.to_owned();
    let tokens = items.iter().map(|item| item.upload.clone()).collect::<Vec<_>>();
    let staged = tauri::async_runtime::spawn_blocking(move || uploads.take_many(&owner, &tokens))
        .await.map_err(|error| error.to_string())??;
    current(state, account_id, &service).map_err(|error| error.to_string())?;
    let inputs = staged.iter().zip(items).map(|(file, item)| AlbumMediaInput {
        name: file.name.clone(), path: file.path.clone(), caption: item.caption,
        quality: item.quality, progress: item.progress,
    }).collect();
    service.send_album(account_id, chat, inputs, parent_id, reply, mentions.unwrap_or_default(),
        || current(state, account_id, &service)).await
        .map_err(|error| command_error(&service, error))
}

fn validate_uploads(items: &[AlbumUploadItem], parent: Option<&str>) -> Result<(), String> {
    if !((if parent.is_some() { 1 } else { 2 })..=MAX_ALBUM_ITEMS).contains(&items.len()) {
        return Err("New albums require 2 to 8 items; a continuation accepts 1 to 8 (application staging bound).".into());
    }
    if parent.is_some_and(|id| id.is_empty() || id.len() > 256) { return Err("Invalid album continuation parent.".into()); }
    for (index, item) in items.iter().enumerate() {
        if item.upload.is_empty() || item.upload.len() > 256 || items[..index].iter().any(|prior| prior.upload == item.upload)
            || item.caption.as_ref().is_some_and(|caption| caption.len() > 64 * 1024)
            || item.progress.as_ref().is_some_and(|token| token.is_empty() || token.len() > 1024) {
            return Err("Invalid album upload metadata.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(upload: &str) -> AlbumUploadItem {
        AlbumUploadItem { upload: upload.into(), caption: None, quality: None, progress: None }
    }

    #[test]
    fn uploads_require_unique_bounded_staged_tokens_and_metadata() {
        assert!(validate_uploads(&[item("one"), item("two")], None).is_ok());
        assert!(validate_uploads(&[], None).is_err());
        assert!(validate_uploads(&[item("one")], None).is_err());
        assert!(validate_uploads(&[item("one")], Some("existing-parent")).is_ok());
        assert!(validate_uploads(&[item("one")], Some("")).is_err());
        assert!(validate_uploads(&[item("one"), item("one")], None).is_err());
        assert!(validate_uploads(&[item("one"), item("")], None).is_err());
        let mut large = item("two");
        large.caption = Some("x".repeat(64 * 1024 + 1));
        assert!(validate_uploads(&[item("one"), large], None).is_err());
        assert!(validate_uploads(&(0..9).map(|index| item(&index.to_string())).collect::<Vec<_>>(), None).is_err());
    }
}
