use crate::{command_error::{CommandError, CommandResult}, settings::sends_privacy, AppState};
use postal_core::{ChatSummary, WhatsAppService};
use std::{future::Future, sync::Arc};
use tauri::State;

#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct MarkReadResult {
    pub chat: String,
    pub changed: Option<usize>,
    pub error: Option<CommandError>,
}

fn current_service(
    state: &AppState,
    account_id: &str,
    service: &Arc<WhatsAppService>,
) -> CommandResult<()> {
    let current = state.service_for_account(account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(())
}

async fn mark_unread_chats<F, Fut>(
    chats: Vec<ChatSummary>,
    current: impl Fn() -> CommandResult<()>,
    mut mark: F,
) -> CommandResult<Vec<MarkReadResult>>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = CommandResult<usize>>,
{
    let mut results = Vec::new();
    for chat in chats
        .into_iter()
        .filter(|chat| chat.unread_count > 0 || chat.marked_unread)
    {
        current()?;
        let result = mark(chat.chat.clone()).await;
        let (changed, error) = match result {
            Ok(changed) => (Some(changed), None),
            Err(error) => (None, Some(error)),
        };
        results.push(MarkReadResult {
            chat: chat.chat,
            changed,
            error,
        });
    }
    current()?;
    Ok(results)
}

#[tauri::command]
pub(crate) async fn mark_all_read(
    state: State<'_, AppState>,
    account_id: String,
) -> CommandResult<Vec<MarkReadResult>> {
    let service = state.service_for_account(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let chats = service
        .chats()
        .await
        .map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let state = &*state;
    let service = &service;
    let account_id = &account_id;
    mark_unread_chats(
        chats,
        || current_service(state, account_id, service),
        |chat| async move {
            let receipts = sends_privacy(state, service, &chat).await.1;
            current_service(state, account_id, service)?;
            service
                .mark_read(&chat, receipts)
                .await
                .map_err(|error| { service.note_error(&error); error.into() })
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn chat(jid: &str, unread_count: i64, marked_unread: bool, archived: bool) -> ChatSummary {
        ChatSummary {
            chat: jid.into(),
            display_name: None,
            last_message_at: 0,
            last_text: String::new(),
            last_from_me: false,
            last_sender_name: None,
            last_sender: String::new(),
            last_media_kind: None,
            message_count: 0,
            unread_count,
            mention_count: 0,
            pinned: false,
            archived,
            muted_until: 0,
            mute_at_all: false,
            marked_unread,
        }
    }

    #[test]
    fn batch_includes_archived_and_manual_unread_continues_failures_and_stops_on_account_change() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let snapshot = vec![
                    chat("read", 0, false, false),
                    chat("archived", 3, false, true),
                    chat("failed", 1, false, false),
                    chat("manual", 0, true, true),
                ];
                let calls = RefCell::new(Vec::new());
                let results = mark_unread_chats(
                    snapshot,
                    || Ok(()),
                    |chat| {
                        let calls = &calls;
                        async move {
                            calls.borrow_mut().push(chat.clone());
                            if chat == "failed" {
                                Err(CommandError::code("error.bulk_read_failed").with_diagnostic("synthetic failure"))
                            } else {
                                Ok(if chat == "archived" { 3 } else { 0 })
                            }
                        }
                    },
                )
                .await
                .unwrap();
                assert_eq!(*calls.borrow(), ["archived", "failed", "manual"]);
                assert_eq!(results.len(), 3);
                assert_eq!(results[0].changed, Some(3));
                assert_eq!(results[1].error.as_ref().unwrap().message.code, "error.bulk_read_failed");
                assert_eq!(results[1].error.as_ref().unwrap().diagnostic.as_deref(), Some("synthetic failure"));
                assert_eq!(results[1].changed, None);
                assert_eq!(results[2].changed, Some(0));
                assert!(results[2].error.is_none());

                let switched = Cell::new(false);
                calls.borrow_mut().clear();
                let cancelled = mark_unread_chats(
                    vec![chat("first", 1, false, false), chat("next", 1, false, true)],
                    || {
                        if switched.get() {
                            Err(CommandError::code("error.account_changed"))
                        } else {
                            Ok(())
                        }
                    },
                    |chat| {
                        let calls = &calls;
                        let switched = &switched;
                        async move {
                            calls.borrow_mut().push(chat);
                            switched.set(true);
                            Ok(1)
                        }
                    },
                )
                .await;
                assert_eq!(cancelled.unwrap_err().message.code, "error.account_changed");
                assert_eq!(*calls.borrow(), ["first"]);
                assert!(mark_unread_chats(
                    Vec::new(),
                    || Err(CommandError::code("error.account_changed")),
                    |_| async { Ok(0) }
                )
                .await
                .is_err());
            });
    }
}
