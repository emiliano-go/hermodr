use tauri::{AppHandle, State};
use crate::command_error::{CommandError, CommandResult};
use crate::{AppState, account_store::{Account, AccountsFile, AccountsView, DEFAULT_ACCOUNT_LABEL, SESSION_POINTER, SESSION_POINTER_ANDROID, account_base, active_account, is_stale_session, now_millis, save_accounts}, connection::start_service};

/// The accounts and which one is active.
#[tauri::command(async)]
pub(crate) fn accounts(state: State<'_, AppState>) -> AccountsView {
    let file = state.accounts.lock().unwrap();
    AccountsView {
        accounts: file.accounts.clone(),
        active: file.active.clone(),
    }
}

/// Adds an account and switches to it, which starts pairing.
#[tauri::command]
pub(crate) async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    label: Option<String>,
) -> CommandResult<()> {
    let _transition = state.account_transition.lock().await;
    let id = format!("acct-{}", now_millis());
    {
        let mut file = state.accounts.lock().unwrap();
        file.accounts.push(Account {
            id: id.clone(),
            label: label.unwrap_or_else(|| DEFAULT_ACCOUNT_LABEL.into()),
            jid: None,
            once_paired: false,

        });
        file.active = Some(id.clone());
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    start_service(&app, &state, &id).await
}

/// Switches to another account, disconnecting the current one.
#[tauri::command]
pub(crate) async fn switch_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let _transition = state.account_transition.lock().await;
    {
        let mut file = state.accounts.lock().unwrap();
        if !file.accounts.iter().any(|a| a.id == id) {
            return Err(CommandError::code("error.unknown_account"));
        }
        file.active = Some(id.clone());
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    start_service(&app, &state, &id).await
}

/// Renames an account.
#[tauri::command]
pub(crate) fn rename_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    label: String,
) -> CommandResult<()> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Ok(());
    }
    {
        let mut file = state.accounts.lock().unwrap();
        match file.accounts.iter_mut().find(|a| a.id == id) {
            Some(account) => account.label = label,
            None => return Err(CommandError::code("error.unknown_account")),
        }
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    Ok(())
}

fn select_account_removal(file: &mut AccountsFile, id: &str) -> CommandResult<bool> {
    if id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\', ':', '\0']) || id.ends_with(['.', ' '])
        || !file.accounts.iter().any(|account| account.id == id) {
        return Err(CommandError::code("error.unknown_account"));
    }
    let was_active = file.active.as_deref() == Some(id);
    file.accounts.retain(|account| account.id != id);
    if was_active { file.active = file.accounts.first().map(|account| account.id.clone()); }
    Ok(was_active)
}

/// Removes an account and its data, switching to another if it was active.
#[tauri::command]
pub(crate) async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let _transition = state.account_transition.lock().await;
    let was_active = {
        let mut file = state.accounts.lock().unwrap();
        let was_active = select_account_removal(&mut file, &id)?;
        save_accounts(&app, &file);
        was_active
    };
    log::info!("removing account {id}");
    if was_active {
        crate::floating::invalidate_all(&app);
        crate::connection::stop_once(&app, &state).await?;
        let running = state.service.lock().unwrap().take();
        if let Some(existing) = running {
            for path in existing.media_paths().await.unwrap_or_default() {
                let _ = std::fs::remove_file(path);
            }
            existing.logout().await;
            existing.shutdown();
        }
    }
    let base = account_base(&app, &id);
    crate::account_store::remove_history(&app, &state.settings.lock().unwrap().clone(), &id);
    if id == "default" {
        // The default account lives in the data root beside the other accounts' folder.
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let db = ["-wal", "-shm", "-journal"].iter().find_map(|s| name.strip_suffix(s)).unwrap_or(&name);
            if db == SESSION_POINTER
                || db == SESSION_POINTER_ANDROID
                || is_stale_session(db, &[])
                || db == "messages.db"
                || db == "aliases.db"
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    } else {
        let _ = std::fs::remove_dir_all(base);
    }
    if was_active {
        if let Some(next) = active_account(&state) {
            start_service(&app, &state, &next).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "account_cleanup_tests.rs"]
mod account_cleanup_tests;

/// The signed-in account's own JID, once connected. Also recorded on the
/// account, so the switcher can show every account's picture, and used to name
/// an account that never got a name of its own.
#[tauri::command]
pub(crate) fn own_jid(app: AppHandle, state: State<'_, AppState>) -> Option<String> {
    let service = state.service().ok()?;
    let jid = service.own_jid();
    if jid.is_empty() {
        return None;
    }
    // Both facts are read before the lock: each is a cached lookup on the
    // client, and there is no reason to hold the accounts lock across one.
    let push_name = service.push_name();
    let Some(id) = active_account(&state) else {
        return Some(jid);
    };
    let mut file = state.accounts.lock().unwrap();
    let mut changed = false;
    if let Some(account) = file.accounts.iter_mut().find(|a| a.id == id) {
        if account.jid.as_deref() != Some(jid.as_str()) {
            account.jid = Some(jid.clone());
            changed = true;
        }
    }
    if file.seed_label(&id, &push_name) {
        changed = true;
    }
    if changed {
        save_accounts(&app, &file);
    }
    Some(jid)
}
