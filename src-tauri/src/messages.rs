use postal_core::StoredMessage;
use tauri::State;
use crate::{AppState, settings::sends_privacy};

/// Stored messages for a chat, newest first.
#[tauri::command(async)]
pub(crate) fn messages(
    state: State<'_, AppState>,
    chat: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .messages(&chat, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Marks a chat as read. Returns how many messages were newly marked.
#[tauri::command]
pub(crate) async fn mark_read(state: State<'_, AppState>, chat: String) -> Result<usize, String> {
    let service = state.service()?;
    let receipts = sends_privacy(&state, &service, &chat).1;
    service.mark_read(&chat, receipts).await.map_err(|e| e.to_string())
}

/// Marks incoming messages up to and including `id` as read.
#[tauri::command]
pub(crate) async fn mark_read_until(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> Result<usize, String> {
    let service = state.service()?;
    let receipts = sends_privacy(&state, &service, &chat).1;
    service
        .mark_read_until(&chat, &id, receipts)
        .await
        .map_err(|e| e.to_string())
}

/// Sends a played receipt for a voice note or view-once media, unless receipts are off.
#[tauri::command]
pub(crate) async fn mark_played(state: State<'_, AppState>, chat: String, id: String, sender: String) -> Result<(), String> {
    let service = state.service()?;
    if !sends_privacy(&state, &service, &chat).1 {
        return Ok(());
    }
    service.mark_played(&chat, &id, &sender).await.map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())
}

/// The message a context-menu action applies to.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Target {
    chat: String,
    id: String,
    sender: String,
    from_me: bool,
}

#[tauri::command]
pub(crate) async fn react(state: State<'_, AppState>, target: Target, emoji: String) -> Result<(), String> {
    state
        .service()?
        .react(&target.chat, &target.id, &target.sender, target.from_me, &emoji)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn star(state: State<'_, AppState>, target: Target, starred: bool) -> Result<(), String> {
    state
        .service()?
        .star(&target.chat, &target.id, &target.sender, target.from_me, starred)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn pin_message(state: State<'_, AppState>, target: Target, pinned: bool) -> Result<(), String> {
    state
        .service()?
        .pin_message(&target.chat, &target.id, &target.sender, target.from_me, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn delete_message(
    state: State<'_, AppState>,
    target: Target,
    everyone: bool,
    timestamp: i64,
) -> Result<(), String> {
    let service = state.service()?;
    let done = if everyone {
        service
            .delete_for_everyone(&target.chat, &target.id, &target.sender, target.from_me)
            .await
    } else {
        service
            .delete_for_me(&target.chat, &target.id, &target.sender, target.from_me, timestamp)
            .await
    };
    done.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn report_message(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    state.service()?.report_to_admins(&chat, &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn forward_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    to: String,
) -> Result<(), String> {
    state.service()?.forward(&chat, &id, &to).await.map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn marks(state: State<'_, AppState>, chat: String) -> Result<postal_core::ChatMarks, String> {
    state.service()?.marks(&chat).map_err(|e| e.to_string())
}

/// Who got, read and played one of our messages.
#[tauri::command(async)]
pub(crate) fn message_info(state: State<'_, AppState>, id: String) -> Result<Vec<postal_core::MessageReceipt>, String> {
    state.service()?.message_info(&id).map_err(|e| e.to_string())
}

/// Starred messages across every chat, newest first.
#[tauri::command(async)]
pub(crate) fn starred_messages(state: State<'_, AppState>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.starred_messages().map_err(|e| e.to_string())
}

/// Messages that mention us, in one chat or (without `chat`) all of them.
#[tauri::command(async)]
pub(crate) fn pings(state: State<'_, AppState>, chat: Option<String>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.pings(chat.as_deref()).map_err(|e| e.to_string())
}

/// Up to `limit` (default 50) messages in one chat whose text contains `query`.
#[tauri::command(async)]
pub(crate) fn search_messages(
    state: State<'_, AppState>,
    chat: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .search_messages(&chat, &query, limit.unwrap_or(50).clamp(1, 500))
        .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())
}

/// Replaces the text of one of our own messages.
#[tauri::command]
pub(crate) async fn edit_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    text: String,
) -> Result<(), String> {
    state
        .service()?
        .edit_message(&chat, &id, text)
        .await
        .map_err(|e| e.to_string())
}

/// The chat a stored message id belongs to.
#[tauri::command(async)]
pub(crate) fn chat_for_message(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    state
        .service()?
        .chat_for_message(&id)
        .map_err(|e| e.to_string())
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
pub(crate) fn unread_mentions(state: State<'_, AppState>, chat: String) -> Result<Vec<String>, String> {
    state
        .service()?
        .unread_mentions(&chat)
        .map_err(|e| e.to_string())
}
