use std::sync::Arc;
use tauri::State;
use crate::AppState;

#[tauri::command]
pub(crate) async fn lookup_username(
    state: State<'_, AppState>, account_id: String, username: String, username_key: Option<String>,
) -> Result<postal_core::UsernameLookupResult, String> {
    let service = state.account_service(&account_id)?;
    let current = || crate::account_store::active_account(&state).as_deref() == Some(account_id.as_str())
        && state.account_service(&account_id).is_ok_and(|live| Arc::ptr_eq(&service, &live));
    if !current() { return Err("Account changed before username lookup.".into()); }
    let result = service.lookup_username(&username, username_key.as_deref(), current).await;
    if !current() { return Err("Account changed during username lookup.".into()); }
    result.map_err(|error| error.to_string())
}
