use tauri::{AppHandle, State};
use crate::{AppState, account_store::{Account, AccountsView, DEFAULT_ACCOUNT_LABEL, SESSION_POINTER, SESSION_POINTER_ANDROID, account_base, active_account, is_stale_session, now_millis, save_accounts}, connection::start_service};

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
) -> Result<(), String> {
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
) -> Result<(), String> {
    {
        let mut file = state.accounts.lock().unwrap();
        if !file.accounts.iter().any(|a| a.id == id) {
            return Err("unknown account".into());
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
) -> Result<(), String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Ok(());
    }
    {
        let mut file = state.accounts.lock().unwrap();
        match file.accounts.iter_mut().find(|a| a.id == id) {
            Some(account) => account.label = label,
            None => return Err("unknown account".into()),
        }
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    Ok(())
}

/// Removes an account and its data, switching to another if it was active.
#[tauri::command]
pub(crate) async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    log::info!("removing account {id}");
    let was_active = active_account(&state).as_deref() == Some(id.as_str());
    {
        let mut file = state.accounts.lock().unwrap();
        file.accounts.retain(|a| a.id != id);
        if was_active {
            file.active = file.accounts.first().map(|a| a.id.clone());
        }
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    crate::connection::stop_once(&app, &state).await?;
    let running = state.service.lock().unwrap().take();
    if let Some(existing) = running {
        // Only the running account can reach WhatsApp to unlink itself.
        if was_active {
            for path in existing.media_paths().await.unwrap_or_default() {
                let _ = std::fs::remove_file(path);
            }
            existing.logout().await;
        }
        existing.shutdown();
    }
    let base = account_base(&app, &id);
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
    if let Some(next) = active_account(&state) {
        start_service(&app, &state, &next).await?;
    }
    Ok(())
}

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
