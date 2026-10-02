use crate::{connection::command_error, AppState};
use postal_core::{store::member_profiles::MemberNote, MemberProfile, WhatsAppService};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn user_profile(
    state: State<'_, AppState>,
    jid: String,
    account_id: Option<String>,
    group: Option<String>,
    live: Option<bool>,
    force: Option<bool>,
) -> Result<MemberProfile, String> {
    let account = account_id
        .or_else(|| state.accounts.lock().unwrap().active.clone())
        .ok_or("No active account.")?;
    let service = state.account_service(&account)?;
    let result = service
        .member_profile(
            &jid,
            group.as_deref(),
            live.unwrap_or(true),
            force.unwrap_or(false),
            || {
                account_current(&state, &account, &service)
                    .map_err(|error| std::io::Error::other(error).into())
            },
        )
        .await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn set_member_note(
    state: State<'_, AppState>,
    account_id: String,
    jid: String,
    group: Option<String>,
    text: String,
    warnings: u32,
) -> Result<MemberNote, String> {
    let service = state.account_service(&account_id)?;
    let result = service
        .set_member_note(&jid, group.as_deref(), &text, warnings, || {
            account_current(&state, &account_id, &service)
                .map_err(|error| std::io::Error::other(error).into())
        })
        .await;
    account_current(&state, &account_id, &service)?;
    result.map_err(|error| command_error(&service, error))
}

fn account_current(
    state: &AppState,
    account: &str,
    expected: &Arc<WhatsAppService>,
) -> Result<(), String> {
    let current = state.account_service(account)?;
    if Arc::ptr_eq(expected, &current) {
        Ok(())
    } else {
        Err("account changed during operation".into())
    }
}
