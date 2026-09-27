use std::sync::Arc;
use postal_core::{WhatsAppService, ServiceEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use crate::{AppState, SERVICE_EVENT, account_store::{Account, DEFAULT_ACCOUNT_LABEL, SESSION_POINTER, account_base, active_account, config_for, now_millis, remove_stale_sessions, save_accounts}};

/// Snapshot of the connection state, for the UI's initial render.
///
/// The pairing code is issued during startup, before the event listener is
/// attached, so a subscriber can miss it. Returning the current values lets the
/// UI recover instead of showing a blank pairing screen forever.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionState {
    pub started: bool,
    pub connected: bool,
    pub qr: Option<String>,
}

/// The event that tells a fallen-behind UI what the current state is.
pub(crate) fn resync_event(service: &WhatsAppService) -> ServiceEvent {
    if service.is_connected() {
        ServiceEvent::Connected
    } else if let Some(code) = service.current_qr() {
        ServiceEvent::QrCode { code }
    } else {
        ServiceEvent::Disconnected
    }
}

#[tauri::command(async)]
pub(crate) fn connection_state(state: State<'_, AppState>) -> ConnectionState {
    let service = state.service.lock().unwrap().clone();
    match service {
        Some(service) => ConnectionState {
            started: true,
            connected: service.is_connected(),
            qr: service.current_qr(),
        },
        None => ConnectionState {
            started: false,
            connected: false,
            qr: None,
        },
    }
}

/// Connects the account, pairing by QR the first time.
///
/// Returns once the service is running; the QR code and connection state arrive
/// as [`SERVICE_EVENT`] messages so the UI can render them as they happen.
/// Starts the service for an account, replacing any running one.
pub(crate) async fn start_service(app: &AppHandle, state: &AppState, account: &str) -> Result<(), String> {
    log::info!("starting account {account}");
    // Stop whatever is running first, so the old account disconnects.
    if let Some(existing) = state.service.lock().unwrap().take() {
        existing.shutdown();
    }

    let settings = state.settings.lock().unwrap().clone();
    let config = config_for(app, &settings, account);

    remove_stale_sessions(&account_base(app, account), &config.session_path);

    let (service, mut events) = WhatsAppService::start(config).await.map_err(|e| {
        log::error!("failed to start account {account}: {e:#}");
        format!("failed to start service: {e}")
    })?;
    let service = Arc::new(service);

    // `events` was registered before the connection attempt, so the pairing code
    // cannot slip through the gap between starting and subscribing.
    let emitter = app.clone();
    let service_for_events = service.clone();
    let account_id = account.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ServiceEvent::LoggedOut) => {
                    log::warn!("account {account_id} was logged out; its session is dropped");
                    forget_session(&emitter, &account_id, &service_for_events);
                    let _ = emitter.emit(SERVICE_EVENT, &ServiceEvent::LoggedOut);
                    // Holding the service keeps its session database open.
                    break;
                }
                Ok(event) => {
                    let _ = emitter.emit(SERVICE_EVENT, &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    // A slow consumer missed some events, and that can include
                    // `Connected`: an offline-sync burst is larger than any
                    // buffer. Re-announce the state so the UI catches up. The
                    // messages themselves are in the store to be refetched.
                    log::warn!("UI fell behind, dropped {dropped} service event(s)");
                    let _ = emitter.emit(SERVICE_EVENT, &resync_event(&service_for_events));
                }
                Err(_) => break,
            }
        }
    });

    // A media folder outside the app data directory still has to be readable by
    // the UI, so the asset scope is widened to whatever was configured.
    if let Some(dir) = service.media_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = app.asset_protocol_scope().allow_directory(&dir, true);
    }

    *state.service.lock().unwrap() = Some(service);
    Ok(())
}

/// Stops a revoked account's service and points it at a fresh session file.
///
/// The old file cannot be deleted yet: Windows keeps it locked until the
/// library releases its connection pool.
pub(crate) fn forget_session(app: &AppHandle, account: &str, service: &Arc<WhatsAppService>) {
    let state = app.state::<AppState>();
    {
        let mut slot = state.service.lock().unwrap();
        if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, service)) {
            *slot = None;
        }
    }
    service.shutdown();
    let mut file = state.accounts.lock().unwrap();
    if let Some(entry) = file.accounts.iter_mut().find(|a| a.id == account) {
        entry.jid = None;
        save_accounts(app, &file);
    }
    let _ = std::fs::write(
        account_base(app, account).join(SESSION_POINTER),
        format!("session-{}.db", now_millis()),
    );
}

/// Connects the active account, pairing by QR the first time.
#[tauri::command]
pub(crate) async fn connect(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.service.lock().unwrap().is_some() {
        return Ok(());
    }
    let account = match active_account(&state) {
        Some(account) => account,
        // No account yet: a fresh one, so files a removed account left open cannot be picked up again.
        None => {
            let id = format!("acct-{}", now_millis());
            {
                let mut file = state.accounts.lock().unwrap();
                file.accounts.push(Account {
                    id: id.clone(),
                    label: DEFAULT_ACCOUNT_LABEL.into(),
                    jid: None,
                });
                file.active = Some(id.clone());
            }
            save_accounts(&app, &state.accounts.lock().unwrap());
            id
        }
    };
    start_service(&app, &state, &account).await
}
