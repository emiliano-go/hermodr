use postal_core::StoredMessage;
use tauri::State;
use crate::{AppState, connection::command_error, settings::sends_privacy};

#[tauri::command(async)]
pub(crate) async fn message_page(state: State<'_, AppState>, chat: String, limit: Option<u32>,
    cursor: Option<postal_core::store::MessageCursor>, direction: Option<postal_core::store::MessagePageDirection>,
    anchor_id: Option<String>) -> Result<postal_core::store::MessagePage, String> {
    state.service()?.message_page(&chat, limit.unwrap_or(500), cursor, direction.unwrap_or_default(), anchor_id.as_deref())
        .await.map_err(|error| error.to_string())
}

/// Stored messages for a chat, newest first.
#[tauri::command(async)]
pub(crate) async fn messages(
    state: State<'_, AppState>,
    chat: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .messages(&chat, limit.unwrap_or(200))
        .await.map_err(|e| e.to_string())
}

/// Marks a chat as read. Returns how many messages were newly marked.
#[tauri::command]
pub(crate) async fn mark_read(state: State<'_, AppState>, chat: String) -> Result<usize, String> {
    let service = state.service()?;
    let receipts = sends_privacy(&state, &service, &chat).await.1;
    service.mark_read(&chat, receipts).await.map_err(|e| command_error(&service, e))
}

/// Marks incoming messages up to and including `id` as read.
#[tauri::command]
pub(crate) async fn mark_read_until(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> Result<usize, String> {
    let service = state.service()?;
    let receipts = sends_privacy(&state, &service, &chat).await.1;
    service
        .mark_read_until(&chat, &id, receipts)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Sends a played receipt for a voice note or view-once media, unless receipts are off.
#[tauri::command]
pub(crate) async fn mark_played(state: State<'_, AppState>, chat: String, id: String, sender: String) -> Result<(), String> {
    let service = state.service()?;
    if !sends_privacy(&state, &service, &chat).await.1 {
        return Ok(());
    }
    service.mark_played(&chat, &id, &sender).await.map_err(|e| command_error(&service, e))
}

/// Sends a text message quoting an earlier one.
#[tauri::command]
pub(crate) async fn send_reply(
    state: State<'_, AppState>,
    chat: String,
    text: String,
    reply_to_id: String,
    reply_to_sender: String,
    reply_to_text: String,
    mentions: Option<Vec<String>>,
    reply_to_chat: Option<String>,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .send_reply(
            &chat,
            text,
            &reply_to_id,
            &reply_to_sender,
            &reply_to_text,
            mentions.unwrap_or_default(),
            reply_to_chat.as_deref().filter(|c| *c != chat),
        )
        .await
        .map_err(|e| command_error(&service, e))
}

/// The message a context-menu action applies to.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct Target {
    chat: String,
    id: String,
    sender: String,
    from_me: bool,
}

#[tauri::command]
pub(crate) async fn react(state: State<'_, AppState>, target: Target, emoji: String) -> Result<(), String> {
    let service = state.service()?;
    service
        .react(&target.chat, &target.id, &target.sender, target.from_me, &emoji)
        .await
        .map_err(|e| command_error(&service, e))
}

#[tauri::command]
pub(crate) async fn star(state: State<'_, AppState>, target: Target, starred: bool) -> Result<(), String> {
    let service = state.service()?;
    service
        .star(&target.chat, &target.id, &target.sender, target.from_me, starred)
        .await
        .map_err(|e| command_error(&service, e))
}

#[tauri::command]
pub(crate) async fn pin_message(state: State<'_, AppState>, target: Target, pinned: bool) -> Result<(), String> {
    let service = state.service()?;
    service
        .pin_message(&target.chat, &target.id, &target.sender, target.from_me, pinned)
        .await
        .map_err(|e| command_error(&service, e))
}

#[tauri::command]
pub(crate) async fn delete_message(
    state: State<'_, AppState>,
    target: Target,
    everyone: bool,
) -> Result<(), String> {
    let service = state.service()?;
    let done = if everyone {
        service
            .delete_for_everyone(&target.chat, &target.id, &target.sender, target.from_me)
            .await
    } else {
        service.delete_for_me(&target.chat, &target.id).await
    };
    done.map_err(|e| command_error(&service, e))
}

/// Deletes several messages at once, for everyone or on this device only.
#[tauri::command]
pub(crate) async fn delete_messages(
    state: State<'_, AppState>,
    chat: String,
    ids: Vec<String>,
    everyone: bool,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .delete_messages(&chat, &ids, everyone)
        .await
        .map_err(|e| command_error(&service, e))
}

#[tauri::command]
pub(crate) async fn report_message(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    let service = state.service()?;
    service.report_to_admins(&chat, &id).await.map_err(|e| command_error(&service, e))
}

#[tauri::command]
pub(crate) async fn forward_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    to: String,
) -> Result<(), String> {
    let service = state.service()?;
    service.forward(&chat, &id, &to).await.map_err(|e| command_error(&service, e))
}

#[tauri::command(async)]
pub(crate) async fn marks(state: State<'_, AppState>, chat: String, ids: Option<Vec<String>>) -> Result<postal_core::ChatMarks, String> {
    let service = state.service()?;
    let limit = state.settings.lock().unwrap().message_window_size;
    let ids = match ids {
        Some(ids) => ids,
        None => service.messages(&chat, limit)
            .await.map_err(|error| error.to_string())?.into_iter().map(|m| m.header.id).collect(),
    };
    service.marks_for(&chat, &ids).await.map_err(|e| e.to_string())
}

/// Who got, read and played one of our messages.
#[tauri::command(async)]
pub(crate) async fn message_info(state: State<'_, AppState>, id: String) -> Result<Vec<postal_core::MessageReceipt>, String> {
    state.service()?.message_info(&id).await.map_err(|e| e.to_string())
}

/// Starred messages across every chat, newest first.
#[tauri::command(async)]
pub(crate) async fn starred_messages(state: State<'_, AppState>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.starred_messages().await.map_err(|e| e.to_string())
}

/// Messages that mention us, in one chat or (without `chat`) all of them.
#[tauri::command(async)]
pub(crate) async fn pings(state: State<'_, AppState>, chat: Option<String>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.pings(chat.as_deref()).await.map_err(|e| e.to_string())
}

/// Up to `limit` (default 50) messages in one chat whose text contains `query`.
#[tauri::command(async)]
pub(crate) async fn search_messages(
    state: State<'_, AppState>,
    chat: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .search_messages(&chat, &query, limit.unwrap_or(50).clamp(1, 500))
        .await.map_err(|e| e.to_string())
}

/// Sends a text message to a chat.
#[tauri::command]
pub(crate) async fn send_text(
    state: State<'_, AppState>,
    chat: String,
    text: String,
    mentions: Option<Vec<String>>,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .send_text(&chat, text, mentions.unwrap_or_default())
        .await
        .map_err(|e| command_error(&service, e))
}

/// Replaces the text of one of our own messages.
#[tauri::command]
pub(crate) async fn edit_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    text: String,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .edit_message(&chat, &id, text)
        .await
        .map_err(|e| command_error(&service, e))
}

/// The chat a stored message id belongs to.
#[tauri::command(async)]
pub(crate) async fn chat_for_message(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    state
        .service()?
        .chat_for_message(&id)
        .await.map_err(|e| e.to_string())
}

/// Asks the phone for older messages in a chat.
#[tauri::command]
pub(crate) async fn load_older(
    state: State<'_, AppState>,
    chat: String,
    count: Option<i32>,
) -> Result<(), String> {
    state
        .service()?
        .load_older(&chat, count.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

/// Starts paging every chat's history back from the phone; progress arrives as `backfill` events.
#[tauri::command]
pub(crate) fn backfill_history(state: State<'_, AppState>) -> Result<(), String> {
    let service = state.service()?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = service.backfill_history().await {
            log::warn!("history backfill stopped: {e}");
        }
    });
    Ok(())
}

/// Unread messages that mention us, oldest first.
#[tauri::command(async)]
pub(crate) async fn unread_mentions(state: State<'_, AppState>, chat: String) -> Result<Vec<String>, String> {
    state
        .service()?
        .unread_mentions(&chat)
        .await.map_err(|e| e.to_string())
}
