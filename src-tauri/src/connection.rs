use std::sync::atomic::Ordering;
use std::sync::Arc;
use postal_core::{WhatsAppService, ServiceEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use crate::{AppState, ONCE_EVENT, SERVICE_EVENT, account_store::{Account, DEFAULT_ACCOUNT_LABEL, SESSION_POINTER, SESSION_POINTER_ANDROID, account_base, active_account, config_for, current_sessions, now_millis, once_config_for, remove_stale_sessions, save_accounts}};

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

/// The optional Android instance's state, for the toggle and its pairing sheet.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OnceState {
    /// Whether a device was ever linked; survives the instance being stopped.
    pub paired: bool,
    /// Whether a pairing session was asked for and is waiting for the scan.
    pub pairing: bool,
    pub running: bool,
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

#[tauri::command]
pub(crate) async fn boolean_props(state: State<'_, AppState>) -> Result<Vec<postal_core::service::BooleanProp>, String> {
    Ok(state.service()?.boolean_props().await)
}

/// Connects the account, pairing by QR the first time.
///
/// Returns once the service is running; the QR code and connection state arrive
/// as [`SERVICE_EVENT`] messages so the UI can render them as they happen.
/// Starts the service for an account, replacing any running one.
pub(crate) async fn start_service(app: &AppHandle, state: &AppState, account: &str) -> Result<(), String> {
    log::info!("starting account {account}");
    // The previous account's instance (and its session) goes first: only the
    // active account's instance may run, and neither link is ever unlinked.
    let _ = stop_once(app, state).await;
    let existing = state.service.lock().unwrap().take();
    if let Some(existing) = existing {
        existing.shutdown_and_disconnect().await;
    }

    let settings = state.settings.lock().unwrap().clone();
    let config = config_for(app, &settings, account);

    let base = account_base(app, account);
    remove_stale_sessions(&base, &current_sessions(&base));

    let (service, mut events) = WhatsAppService::start(config).await.map_err(|e| {
        log::error!("failed to start account {account}: {e:#}");
        format!("failed to start service: {e}")
    })?;
    let service = Arc::new(service);

    // `events` was registered before the connection attempt, so the pairing code
    // cannot slip through the gap between starting and subscribing. The service
    // is held weakly: once the active slot drops it, a swap lets the loop, and
    // with it the old session files, go.
    let emitter = app.clone();
    let service_for_events = Arc::downgrade(&service);
    let account_id = account.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ServiceEvent::LoggedOut) => {
                    log::warn!("account {account_id} was logged out; its session is dropped");
                    if let Some(service) = service_for_events.upgrade() {
                        forget_session(&emitter, &account_id, &service);
                    }
                    emit_service_event(&emitter, &ServiceEvent::LoggedOut);
                    // Holding the service keeps its session database open.
                    break;
                }
                Ok(event) => {
                    if service_for_events.upgrade().is_none() {
                        break;
                    }
                    if matches!(
                        event,
                        ServiceEvent::Message { .. } | ServiceEvent::MessageHint { .. }
                    ) {
                        emitter.state::<AppState>().once_wake.notify_one();
                    }
                    emit_service_event(&emitter, &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    // A slow consumer missed some events, and that can include
                    // `Connected`: an offline-sync burst is larger than any
                    // buffer. Re-announce the state so the UI catches up. The
                    // messages themselves are in the store to be refetched.
                    log::warn!("UI fell behind, dropped {dropped} service event(s)");
                    let Some(service) = service_for_events.upgrade() else { break };
                    emit_service_event(&emitter, &resync_event(&service));
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

    // The manager decides whether the companion is needed for this account.
    wake_once(app);
    Ok(())
}

/// Pokes the companion manager to re-check whether the instance should run.
pub(crate) fn wake_once(app: &AppHandle) {
    app.state::<AppState>().once_wake.notify_one();
}

/// How recent a one-time message must be to wake the companion. Older stubs
/// were likely spent on the phone, so they stop being demand.
const ONCE_RECOVERY_WINDOW: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// How long the companion stays linked after its last fetch resolves.
const ONCE_STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(15);
/// A fetch that makes no progress for this long is given up on, so a message
/// the companion cannot receive does not keep the session up.
const ONCE_GIVE_UP: std::time::Duration = std::time::Duration::from_secs(90);
/// Poll pacing while the companion runs, and while it is dormant.
const ONCE_RUNNING_TICK: std::time::Duration = std::time::Duration::from_secs(3);
const ONCE_DORMANT_TICK: std::time::Duration = std::time::Duration::from_secs(60);
/// An abandoned pairing session stops itself, so the QR screen cannot hold a
/// link open forever.
const ONCE_PAIR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Runs the Android companion on demand instead of around the clock. The main
/// -link side detects a one-time message and wakes it; it fetches what it can
/// and goes dormant again. Enabling it needs an existing link, so pairing is
/// its own short session that ends at the scan.
pub(crate) fn spawn_once_manager(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // One-time messages the running companion is expected to fetch, and
        // the ones it already failed on this run.
        let mut expecting: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut ignored: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut last_progress = std::time::Instant::now();
        let mut pairing_since: Option<std::time::Instant> = None;

        loop {
            let state = app.state::<AppState>();
            let enabled = state.settings.lock().unwrap().android_instance;
            let pairing = state.once_pairing.load(Ordering::SeqCst);
            let account = active_account(&state);
            let paired = account.as_deref().is_some_and(|id| once_paired(&state, id));
            let running = state.once_service.lock().unwrap().is_some();

            if account.is_none() || (!enabled && !pairing) {
                // Off, or nothing to attach to: stop at once, no grace.
                if running {
                    if let Err(e) = stop_once(&app, &state).await {
                        log::warn!("could not put the Android companion to sleep: {e}");
                    }
                }
                expecting.clear();
                if account.is_none() {
                    ignored.clear();
                }
                pairing_since = None;
                last_progress = std::time::Instant::now();
            } else if pairing && !paired {
                // Pairing: stay up only long enough for the QR to be scanned.
                let started = *pairing_since.get_or_insert_with(std::time::Instant::now);
                if started.elapsed() >= ONCE_PAIR_TIMEOUT {
                    log::info!("Android companion pairing timed out; start it again to retry");
                    state.once_pairing.store(false, Ordering::SeqCst);
                    pairing_since = None;
                    if running {
                        let _ = stop_once(&app, &state).await;
                    }
                } else if !running {
                    if let Err(e) = start_once(&app, &state).await {
                        log::warn!("could not start the Android companion for pairing: {e}");
                    }
                }
            } else {
                if pairing && paired {
                    // The scan landed: the pairing session is done with.
                    state.once_pairing.store(false, Ordering::SeqCst);
                }
                pairing_since = None;
                if !enabled || !paired {
                    // Enabling requires a link; a revoked one stops the session.
                    if running {
                        if let Err(e) = stop_once(&app, &state).await {
                            log::warn!("could not put the Android companion to sleep: {e}");
                        }
                    }
                } else {
                    let pending: std::collections::HashSet<(String, String)> = state
                        .service
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|s| s.pending_view_once(ONCE_RECOVERY_WINDOW))
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|id| !ignored.contains(id))
                        .collect();
                    if pending != expecting {
                        last_progress = std::time::Instant::now();
                        expecting = pending.clone();
                    }
                    if !pending.is_empty() {
                        if !running {
                            log::info!(
                                "waking the Android companion for {} one-time message(s)",
                                pending.len()
                            );
                            if let Err(e) = start_once(&app, &state).await {
                                log::warn!("could not wake the Android companion: {e}");
                            }
                        } else if last_progress.elapsed() >= ONCE_GIVE_UP {
                            log::warn!(
                                "Android companion could not fetch {} one-time message(s); going dormant",
                                pending.len()
                            );
                            ignored.extend(pending.iter().cloned());
                            expecting.clear();
                            let _ = stop_once(&app, &state).await;
                            last_progress = std::time::Instant::now();
                        }
                    } else if running && last_progress.elapsed() >= ONCE_STOP_GRACE {
                        log::info!("Android companion is done fetching; going dormant");
                        let _ = stop_once(&app, &state).await;
                    }
                }
            }
            let running = state.once_service.lock().unwrap().is_some();
            drop(state);

            let tick = if running { ONCE_RUNNING_TICK } else { ONCE_DORMANT_TICK };
            tokio::time::timeout(tick, app.state::<AppState>().once_wake.notified())
                .await
                .ok();
        }
    });
}

fn emit_service_event(app: &AppHandle, event: &ServiceEvent) {
    if let Err(error) = app.emit(SERVICE_EVENT, event) {
        log::error!("could not emit service event to UI: {error}");
    }
}

/// Whether the account already linked the optional Android instance.
pub(crate) fn once_paired(state: &AppState, account: &str) -> bool {
    state
        .accounts
        .lock()
        .unwrap()
        .accounts
        .iter()
        .find(|a| a.id == account)
        .is_some_and(|a| a.once_paired)
}

fn set_once_paired(app: &AppHandle, account: &str, paired: bool) {
    let state = app.state::<AppState>();
    let mut file = state.accounts.lock().unwrap();
    if let Some(entry) = file.accounts.iter_mut().find(|a| a.id == account) {
        entry.once_paired = paired;
        save_accounts(app, &file);
    }
}

/// Starts the optional Android instance: a second link used only to fetch
/// one-time media into the shared store. No-op when it is already running.
pub(crate) async fn start_once(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if state.once_service.lock().unwrap().is_some() {
        return Ok(());
    }
    let Some(account) = active_account(state) else {
        return Err("no account yet".to_string());
    };
    let settings = state.settings.lock().unwrap().clone();
    let config = once_config_for(app, &settings, &account)?;
    remove_stale_sessions(&account_base(app, &account), &current_sessions(&account_base(app, &account)));

    let (service, mut events) = WhatsAppService::start(config).await.map_err(|e| {
        log::error!("failed to start the Android instance: {e:#}");
        format!("failed to start the Android instance: {e}")
    })?;
    *state.once_qr.lock().unwrap() = service.current_qr();
    let service = Arc::new(service);
    *state.once_service.lock().unwrap() = Some(service.clone());

    // Its own event stream drives the pairing sheet and forwards the store
    // changes the main UI must reload for. Held weakly so a stop drops it.
    let emitter = app.clone();
    let service_for_events = Arc::downgrade(&service);
    let account_id = account.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if service_for_events.upgrade().is_none() {
                        break;
                    }
                    let state = emitter.state::<AppState>();
                    match &event {
                        ServiceEvent::QrCode { code } => {
                            *state.once_qr.lock().unwrap() = Some(code.clone());
                        }
                        ServiceEvent::Connected => {
                            state.once_connected.store(true, Ordering::SeqCst);
                            *state.once_qr.lock().unwrap() = None;
                            set_once_paired(&emitter, &account_id, true);
                        }
                        ServiceEvent::Disconnected => {
                            state.once_connected.store(false, Ordering::SeqCst);
                        }
                        ServiceEvent::LoggedOut => {
                            state.once_connected.store(false, Ordering::SeqCst);
                            *state.once_qr.lock().unwrap() = None;
                            set_once_paired(&emitter, &account_id, false);
                            if let Some(service) = service_for_events.upgrade() {
                                forget_once_session(&emitter, &account_id, &service);
                            }
                            let _ = emitter.emit(ONCE_EVENT, &event);
                            break;
                        }
                        // The shared store changed under the main session; let
                        // the main UI reload without duplicating the instance's
                        // connection noise.
                        ServiceEvent::Message { .. }
                        | ServiceEvent::MessageHint { .. }
                        | ServiceEvent::Marks { .. }
                        | ServiceEvent::ChatStateChanged { .. }
                        | ServiceEvent::RetentionApplied { .. } => {
                            emit_service_event(&emitter, &event);
                            // A kept one-time clears the demand; re-check soon.
                            emitter.state::<AppState>().once_wake.notify_one();
                        }
                        _ => {}
                    }
                    let _ = emitter.emit(ONCE_EVENT, &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
        let state = emitter.state::<AppState>();
        state.once_connected.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Stops the optional Android instance without unlinking it: the link stays
/// paired on the phone for the next time the toggle is turned on.
pub(crate) async fn stop_once(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let existing = state.once_service.lock().unwrap().take();
    if let Some(existing) = existing {
        existing.shutdown_and_disconnect().await;
    }
    *state.once_qr.lock().unwrap() = None;
    state.once_connected.store(false, Ordering::SeqCst);
    state.once_pairing.store(false, Ordering::SeqCst);
    let _ = app.emit(ONCE_EVENT, &ServiceEvent::Disconnected);
    Ok(())
}

#[tauri::command]
pub(crate) fn once_state(state: State<'_, AppState>) -> OnceState {
    let account = active_account(&state);
    let paired = account
        .as_deref()
        .map(|id| once_paired(&state, id))
        .unwrap_or(false);
    OnceState {
        paired,
        pairing: state.once_pairing.load(Ordering::SeqCst),
        running: state.once_service.lock().unwrap().is_some(),
        connected: state.once_connected.load(Ordering::SeqCst),
        qr: state.once_qr.lock().unwrap().clone(),
    }
}

/// Starts or cancels the pairing session. Enabling the companion requires an
/// existing link, so pairing is its own step that runs the instance just long
/// enough for the QR to be scanned.
#[tauri::command]
pub(crate) fn set_pairing(app: AppHandle, state: State<'_, AppState>, pairing: bool) -> Result<(), String> {
    state.once_pairing.store(pairing, Ordering::SeqCst);
    wake_once(&app);
    Ok(())
}

/// Drops the whole account: the main session and the optional instance, whose
/// session file is rotated so a revoked link is never reused.
///
/// Neither file can be deleted yet: Windows keeps them locked until the library
/// releases its connection pools.
pub(crate) fn forget_session(app: &AppHandle, account: &str, service: &Arc<WhatsAppService>) {
    let state = app.state::<AppState>();
    {
        let mut slot = state.service.lock().unwrap();
        if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, service)) {
            *slot = None;
        }
    }
    service.shutdown();
    {
        let once = state.once_service.lock().unwrap().take();
        if let Some(once) = once {
            tauri::async_runtime::spawn(async move { once.shutdown_and_disconnect().await });
        }
    }
    *state.once_qr.lock().unwrap() = None;
    state.once_connected.store(false, Ordering::SeqCst);
    let mut file = state.accounts.lock().unwrap();
    if let Some(entry) = file.accounts.iter_mut().find(|a| a.id == account) {
        entry.jid = None;
        entry.once_paired = false;
        save_accounts(app, &file);
    }
    let base = account_base(app, account);
    for pointer in [SESSION_POINTER, SESSION_POINTER_ANDROID] {
        let _ = std::fs::write(base.join(pointer), format!("session-{}.db", now_millis()));
    }
}

/// Dropped instance link: rotate only its session file, keeping the main link.
fn forget_once_session(app: &AppHandle, account: &str, service: &Arc<WhatsAppService>) {
    let state = app.state::<AppState>();
    {
        let mut slot = state.once_service.lock().unwrap();
        if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, service)) {
            *slot = None;
        }
    }
    service.shutdown();
    let _ = std::fs::write(
        account_base(app, account).join(SESSION_POINTER_ANDROID),
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
                    once_paired: false,
                });
                file.active = Some(id.clone());
            }
            save_accounts(&app, &state.accounts.lock().unwrap());
            id
        }
    };
    start_service(&app, &state, &account).await
}
