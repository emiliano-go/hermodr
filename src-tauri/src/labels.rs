use crate::{AppState, command_error::{CommandError, CommandResult}};
use postal_core::message_ref::MessageRef;
use postal_core::store::labels::LabelsView;
use postal_core::service::WhatsAppService;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn label_owner(app: AppHandle, account: String, service: Arc<WhatsAppService>) -> impl Fn() -> anyhow::Result<()> + Send + Sync {
    move || {
        let state = app.state::<AppState>();
        let current = state.account_service(&account).map_err(|error| anyhow::Error::new(MessageRef::new("error.account_changed")).context(error))?;
        anyhow::ensure!(Arc::ptr_eq(&service, &current), MessageRef::new("error.account_changed"));
        anyhow::ensure!(service.is_connected(), MessageRef::new("error.not_connected"));
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn labels_view(state: State<'_, AppState>, account_id: String) -> CommandResult<LabelsView> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let view = service.labels_view().await.map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    Ok(view)
}

#[tauri::command]
pub(crate) async fn labelled_messages(
    state: State<'_, AppState>, account_id: String, label_ids: Vec<String>, chat: Option<String>, query: String, limit: Option<u32>,
) -> CommandResult<Vec<postal_core::store::StoredMessage>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let rows = service.labelled_messages(&label_ids, chat.as_deref(), &query, limit.unwrap_or(500))
        .await.map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    Ok(rows)
}

#[tauri::command]
pub(crate) async fn save_label(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, name: String, color: i32, create: bool,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.save_label(&label_id, &name, color, create, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn delete_label(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.delete_label(&label_id, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn label_chat(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, chat: String, labeled: bool,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.label_chat(&label_id, &chat, labeled, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn label_message(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, chat: String, message_id: String, labeled: bool,
) -> CommandResult<()> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.label_message(&label_id, &chat, &message_id, labeled, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| { service.note_error(&error); error.into() })
}
