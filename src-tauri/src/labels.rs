use crate::{AppState, connection::command_error};
use postal_core::store::labels::LabelsView;
use postal_core::service::WhatsAppService;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn label_owner(app: AppHandle, account: String, service: Arc<WhatsAppService>) -> impl Fn() -> anyhow::Result<()> + Send + Sync {
    move || {
        let state = app.state::<AppState>();
        let current = state.account_service(&account).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(Arc::ptr_eq(&service, &current) && service.is_connected(), "account changed or disconnected during label operation");
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn labels_view(state: State<'_, AppState>, account_id: String) -> Result<LabelsView, String> {
    let service = state.account_service(&account_id)?;
    let view = service.labels_view().await.map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed while loading labels".into()); }
    Ok(view)
}

#[tauri::command]
pub(crate) async fn labelled_messages(
    state: State<'_, AppState>, account_id: String, label_ids: Vec<String>, chat: Option<String>, query: String, limit: Option<u32>,
) -> Result<Vec<postal_core::store::StoredMessage>, String> {
    let service = state.account_service(&account_id)?;
    let rows = service.labelled_messages(&label_ids, chat.as_deref(), &query, limit.unwrap_or(500))
        .await.map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed while finding labelled messages".into()); }
    Ok(rows)
}

#[tauri::command]
pub(crate) async fn save_label(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, name: String, color: i32, create: bool,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    service.save_label(&label_id, &name, color, create, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn delete_label(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    service.delete_label(&label_id, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn label_chat(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, chat: String, labeled: bool,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    service.label_chat(&label_id, &chat, labeled, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn label_message(
    app: AppHandle, state: State<'_, AppState>, account_id: String, label_id: String, chat: String, message_id: String, labeled: bool,
) -> Result<(), String> {
    let service = state.account_service(&account_id)?;
    service.label_message(&label_id, &chat, &message_id, labeled, label_owner(app, account_id, service.clone()))
        .await.map_err(|error| command_error(&service, error))
}
