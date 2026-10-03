use crate::command_error::{CommandError, CommandResult};
use postal_core::StoredMessage;
use tauri::State;
use crate::{AppState, settings::sends_privacy};

#[tauri::command(async)]
pub(crate) async fn message_page(state: State<'_, AppState>, chat: String, limit: Option<u32>,
    cursor: Option<postal_core::store::MessageCursor>, direction: Option<postal_core::store::MessagePageDirection>,
    anchor_id: Option<String>) -> CommandResult<postal_core::store::MessagePage> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.message_page(&chat, limit.unwrap_or(500), cursor, direction.unwrap_or_default(), anchor_id.as_deref())
        .await.map_err(CommandError::from)
}

/// Stored messages for a chat, newest first.
#[tauri::command(async)]
pub(crate) async fn messages(
    state: State<'_, AppState>,
    chat: String,
    limit: Option<u32>,
) -> CommandResult<Vec<StoredMessage>> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .messages(&chat, limit.unwrap_or(200))
        .await.map_err(CommandError::from)
}

/// Marks a chat as read. Returns how many messages were newly marked.
#[tauri::command]
pub(crate) async fn mark_read(state: State<'_, AppState>, chat: String, account: Option<String>) -> CommandResult<usize> {
    let service = match account { Some(account) => state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    let receipts = sends_privacy(&state, &service, &chat).await.1;
    service.mark_read(&chat, receipts).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Marks incoming messages up to and including `id` as read.
#[tauri::command]
pub(crate) async fn mark_read_until(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> CommandResult<usize> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let receipts = sends_privacy(&state, &service, &chat).await.1;
    service
        .mark_read_until(&chat, &id, receipts)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Sends a played receipt for a voice note or view-once media, unless receipts are off.
#[tauri::command]
pub(crate) async fn mark_played(state: State<'_, AppState>, chat: String, id: String, sender: String) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    if !sends_privacy(&state, &service, &chat).await.1 {
        return Ok(());
    }
    service.mark_played(&chat, &id, &sender).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
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
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
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
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
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
pub(crate) async fn react(state: State<'_, AppState>, target: Target, emoji: String) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .react(&target.chat, &target.id, &target.sender, target.from_me, &emoji)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn star(state: State<'_, AppState>, target: Target, starred: bool) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .star(&target.chat, &target.id, &target.sender, target.from_me, starred)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn pin_message(state: State<'_, AppState>, target: Target, pinned: bool) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .pin_message(&target.chat, &target.id, &target.sender, target.from_me, pinned)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn delete_message(
    state: State<'_, AppState>,
    target: Target,
    everyone: bool,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let done = if everyone {
        service
            .delete_for_everyone(&target.chat, &target.id, &target.sender, target.from_me)
            .await
    } else {
        service.delete_for_me(&target.chat, &target.id).await
    };
    done.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Deletes several messages at once, for everyone or on this device only.
#[tauri::command]
pub(crate) async fn delete_messages(
    state: State<'_, AppState>,
    chat: String,
    ids: Vec<String>,
    everyone: bool,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .delete_messages(&chat, &ids, everyone)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn report_message(state: State<'_, AppState>, chat: String, id: String) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service.report_to_admins(&chat, &id).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn forward_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    to: String,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service.forward(&chat, &id, &to).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command(async)]
pub(crate) async fn marks(state: State<'_, AppState>, chat: String, ids: Option<Vec<String>>) -> CommandResult<postal_core::ChatMarks> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let limit = state.settings.lock().unwrap().message_window_size;
    let ids = match ids {
        Some(ids) => ids,
        None => service.messages(&chat, limit)
            .await.map_err(CommandError::from)?.into_iter().map(|m| m.header.id).collect(),
    };
    service.marks_for(&chat, &ids).await.map_err(CommandError::from)
}

/// Who got, read and played one of our messages.
#[tauri::command(async)]
pub(crate) async fn message_info(state: State<'_, AppState>, id: String) -> CommandResult<Vec<postal_core::MessageReceipt>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.message_info(&id).await.map_err(CommandError::from)
}

/// Starred messages across every chat, newest first.
#[tauri::command(async)]
pub(crate) async fn starred_messages(state: State<'_, AppState>, account_id: Option<String>) -> CommandResult<Vec<StoredMessage>> {
    let service = match account_id.as_deref() { Some(account) => state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    let result = service.starred_messages().await;
    if let Some(account) = account_id {
        if !std::sync::Arc::ptr_eq(&service, &state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?) { return Err(CommandError::code("error.account_changed")); }
    }
    result.map_err(CommandError::from)
}

/// Messages that mention us, in one chat or (without `chat`) all of them.
#[tauri::command(async)]
pub(crate) async fn pings(state: State<'_, AppState>, chat: Option<String>, mute_all_at_all: Option<bool>) -> CommandResult<Vec<StoredMessage>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.pings_with(chat.as_deref(), mute_all_at_all.unwrap_or(false)).await.map_err(CommandError::from)
}

/// Up to `limit` (default 50) messages in one chat whose text contains `query`.
#[tauri::command(async)]
pub(crate) async fn search_messages(
    state: State<'_, AppState>,
    chat: String,
    query: String,
    limit: Option<u32>,
    account_id: Option<String>,
) -> CommandResult<Vec<StoredMessage>> {
    let service = match account_id.as_deref() { Some(account) => state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    let result = service.search_messages(&chat, &query, limit.unwrap_or(50).clamp(1, 500)).await;
    if let Some(account) = account_id {
        if !std::sync::Arc::ptr_eq(&service, &state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?) { return Err(CommandError::code("error.account_changed")); }
    }
    result.map_err(CommandError::from)
}

/// Sends a text message to a chat.
#[tauri::command]
pub(crate) async fn send_text(
    state: State<'_, AppState>,
    chat: String,
    text: String,
    mentions: Option<Vec<String>>,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .send_text(&chat, text, mentions.unwrap_or_default())
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Replaces the text of one of our own messages.
#[tauri::command]
pub(crate) async fn edit_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    text: String,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .edit_message(&chat, &id, text)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// The chat a stored message id belongs to.
#[tauri::command(async)]
pub(crate) async fn chat_for_message(state: State<'_, AppState>, id: String) -> CommandResult<Option<String>> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .chat_for_message(&id)
        .await.map_err(CommandError::from)
}

/// Asks the phone for older messages in a chat.
#[tauri::command]
pub(crate) async fn load_older(
    state: State<'_, AppState>,
    chat: String,
    count: Option<i32>,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .load_older(&chat, count.unwrap_or(50))
        .await
        .map_err(CommandError::from)
}

/// Starts paging every chat's history back from the phone; progress arrives as `backfill` events.
#[tauri::command]
pub(crate) fn backfill_history(state: State<'_, AppState>) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = service.backfill_history().await {
            log::warn!("history backfill stopped: {e}");
        }
    });
    Ok(())
}

/// Unread messages that mention us, oldest first.
#[tauri::command(async)]
pub(crate) async fn unread_mentions(state: State<'_, AppState>, chat: String, mute_all_at_all: Option<bool>) -> CommandResult<Vec<String>> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .unread_mentions_with(&chat, mute_all_at_all.unwrap_or(false))
        .await.map_err(CommandError::from)
}
