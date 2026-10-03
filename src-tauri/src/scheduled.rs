use std::sync::Arc;
use postal_core::{WhatsAppService, store::scheduled::{decode_scheduled_failure, ScheduledMessage}};
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct ScheduledMessageView {
    #[serde(flatten)]
    pub message: ScheduledMessage,
    pub failure: Option<CommandError>,
}

impl From<ScheduledMessage> for ScheduledMessageView {
    fn from(message: ScheduledMessage) -> Self {
        let failure = message.error.as_ref().map(|raw| decode_scheduled_failure(raw, &message.status).into());
        Self { message, failure }
    }
}

impl AppState {
    pub(crate) fn account_service(&self, account: &str) -> CommandResult<Arc<WhatsAppService>> {
        let binding = self.account_service.lock().unwrap();
        let service = binding.as_ref().filter(|(id, _)| id == account)
            .and_then(|(_, service)| service.upgrade()).ok_or_else(|| CommandError::code("error.account_changed"))?;
        let current = self.service()?;
        if self.accounts.lock().unwrap().active.as_deref() != Some(account) || !Arc::ptr_eq(&service, &current) {
            return Err(CommandError::code("error.account_changed"));
        }
        Ok(service)
    }
}

#[tauri::command]
pub(crate) async fn schedule_message(state: State<'_, AppState>, account: String, chat: String,
    text: String, mentions: Option<Vec<String>>, due_at: i64) -> CommandResult<String> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .schedule_message(&chat, text, mentions.unwrap_or_default(), due_at).await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn scheduled_messages(state: State<'_, AppState>, account: String) -> CommandResult<Vec<ScheduledMessageView>> {
    let rows = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .scheduled_messages().await.map_err(CommandError::from)?;
    Ok(rows.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub(crate) async fn update_scheduled_message(state: State<'_, AppState>, account: String,
    id: String, text: String, due_at: i64) -> CommandResult<()> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .update_scheduled_message(id, text, due_at).await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn cancel_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> CommandResult<()> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .cancel_scheduled_message(id).await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn retry_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> CommandResult<()> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?
        .retry_scheduled_message(id).await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn send_scheduled_message(state: State<'_, AppState>, account: String, id: String) -> CommandResult<bool> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.send_scheduled_message(id).await.map_err(|error| { service.note_error(&error); error.into() })
}

#[cfg(test)]
mod localized_tests {
    use super::*;

    #[test]
    fn failure_view_keeps_legacy_fields_and_delivery_uncertainty_without_text_classification() {
        for (status, code) in [("failed", "error.scheduled_failure_legacy"), ("uncertain", "error.scheduled_failure_legacy_uncertain")] {
            let row = ScheduledMessage { id: "synthetic".into(), chat: "1@lid".into(), text: "body".into(), mentions: Vec::new(),
                due_at: 1, status: status.into(), error: Some("untranslated technical cause".into()), attempted: true };
            let view = ScheduledMessageView::from(row);
            let wire = serde_json::to_value(&view).unwrap();
            assert_eq!(wire["status"], status);
            assert_eq!(wire["error"], "untranslated technical cause");
            assert_eq!(wire["failure"]["code"], code);
            assert_eq!(wire["failure"]["diagnostic"], "untranslated technical cause");
            assert_eq!(wire["failure"]["kind"], "postal_error");
        }
        let row = ScheduledMessage { id: "synthetic".into(), chat: "1@lid".into(), text: "body".into(), mentions: Vec::new(),
            due_at: 1, status: "pending".into(), error: None, attempted: false };
        assert!(ScheduledMessageView::from(row).failure.is_none());
    }

    #[test]
    fn failure_view_decodes_version_one_without_replacing_the_recorded_reason() {
        let raw = r#"{"format":"postal_scheduled_failure_v1","failure":{"code":"error.scheduled_mention_invalid","params":{"item_index":2},"diagnostic":"synthetic parse detail"}}"#;
        let row = ScheduledMessage { id: "synthetic".into(), chat: "1@lid".into(), text: "body".into(), mentions: Vec::new(),
            due_at: 1, status: "uncertain".into(), error: Some(raw.into()), attempted: true };
        let wire = serde_json::to_value(ScheduledMessageView::from(row)).unwrap();
        assert_eq!(wire["error"], raw);
        assert_eq!(wire["status"], "uncertain");
        assert_eq!(wire["failure"]["code"], "error.scheduled_mention_invalid");
        assert_eq!(wire["failure"]["params"]["item_index"], 2);
        assert_eq!(wire["failure"]["diagnostic"], "synthetic parse detail");
        assert_eq!(wire["failure"]["kind"], "postal_error");
    }
}
