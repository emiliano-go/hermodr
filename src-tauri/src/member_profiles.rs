use crate::command_error::{CommandError, CommandResult};
use crate::AppState;
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
) -> CommandResult<MemberProfile> {
    let account = account_id
        .or_else(|| state.accounts.lock().unwrap().active.clone())
        .ok_or_else(|| CommandError::code("error.not_connected"))?;
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service
        .member_profile(
            &jid,
            group.as_deref(),
            live.unwrap_or(true),
            force.unwrap_or(false),
            || {
                account_current(&state, &account, &service)
                    .map_err(|error| error.message.into())
            },
        )
        .await;
    account_current(&state, &account, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn set_member_note(
    state: State<'_, AppState>,
    account_id: String,
    jid: String,
    group: Option<String>,
    text: String,
    warnings: u32,
) -> CommandResult<MemberNote> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service
        .set_member_note(&jid, group.as_deref(), &text, warnings, || {
            account_current(&state, &account_id, &service)
                .map_err(|error| error.message.into())
        })
        .await;
    account_current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

fn account_current(
    state: &AppState,
    account: &str,
    expected: &Arc<WhatsAppService>,
) -> CommandResult<()> {
    let current = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &current) {
        Ok(())
    } else {
        Err(CommandError::code("error.account_changed"))
    }
}
