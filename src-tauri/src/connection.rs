use std::sync::atomic::Ordering;
use std::sync::Arc;
use postal_core::{WhatsAppService, ServiceEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use crate::command_error::{CommandError, CommandResult};
use crate::{AppState, ONCE_EVENT, SERVICE_EVENT, account_store::{Account, DEFAULT_ACCOUNT_LABEL, SESSION_POINTER, SESSION_POINTER_ANDROID, account_base, active_account, config_for, current_sessions, now_millis, once_config_for, remove_stale_sessions, save_accounts}};

/// Snapshot of the connection state, for the UI's initial render.
///
/// The pairing code is issued during startup, before the event listener is
/// attached, so a subscriber can miss it. Returning the current values lets the
/// UI recover instead of showing a blank pairing screen forever.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ConnectionState {
    pub started: bool,
    pub connected: bool,
    pub qr: Option<String>,
}

/// The optional Android instance's state, for the toggle and its pairing sheet.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
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
pub(crate) async fn boolean_props(state: State<'_, AppState>) -> CommandResult<Vec<postal_core::service::BooleanProp>> {
    Ok(state.service().map_err(|_| CommandError::code("error.not_connected"))?.boolean_props().await)
}

/// Connects the account, pairing by QR the first time.
///
/// Returns once the service is running; the QR code and connection state arrive
/// as [`SERVICE_EVENT`] messages so the UI can render them as they happen.
/// Starts the service for an account, replacing any running one.
pub(crate) async fn start_service(app: &AppHandle, state: &AppState, account: &str) -> CommandResult<()> {
    crate::floating::invalidate_all(app);
    log::info!("starting account {account}");
    app.state::<crate::transcription::TranscriptionState>().cancel_all();
    // The previous account's instance (and its session) goes first: only the
    // active account's instance may run, and neither link is ever unlinked.
    let _ = stop_once(app, state).await;
    let existing = state.service.lock().unwrap().take();
    if let Some(existing) = existing {
        existing.shutdown_and_disconnect().await;
    }

    let settings = state.settings.lock().unwrap().clone();
    let base = account_base(app, account);
    let database_key = app.state::<crate::database_encryption::DatabaseEncryption>().new_account_key(account, &account_base(app, "default"))
        .map_err(crate::database_encryption::command_failure)?;
    // A change of cold storage moves the archive before it is opened.
    crate::account_store::migrate_history(app, &settings, account);
    let mut config = config_for(app, &settings, account);
    config.database_key = database_key;
    if let Some(directory) = &config.media_dir {
        let directory = crate::media_access::validate_directory(app, directory)?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }

    remove_stale_sessions(&base, &current_sessions(&base));

    let (service, events) = WhatsAppService::start(config).await.map_err(|e| {
        log::error!("failed to start account {account}: {e:#}");
        CommandError::code("error.service_start_failed").with_diagnostic(e)
    })?;
    let service = Arc::new(service);
    *state.account_service.lock().unwrap() = Some((account.to_owned(), Arc::downgrade(&service)));
    *state.service.lock().unwrap() = Some(service.clone());
    spawn_main_events(app, &service, account, events);

    // The manager decides whether the companion is needed for this account.
    wake_once(app);
    Ok(())
}

/// Turns a command failure into the UI's message, telling the service first:
/// a timeout or socket error on a send is how a half-open link is noticed
/// without waiting for the watchdog.
pub(crate) fn command_error(service: &WhatsAppService, error: impl std::fmt::Display) -> String {
    service.note_error(&error);
    error.to_string()
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
/// link side detects a one-time message and wakes it; it fetches what it can
/// and goes dormant again. Enabling it needs an existing link, so pairing is
/// its own short session that ends at the scan.
pub(crate) fn spawn_once_manager(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut companion = CompanionState::new();
        loop {
            let state = app.state::<AppState>();
            let view = observe_companion(&state).await;
            let step = companion.step(&view, std::time::Instant::now());
            if step.cancel_pairing {
                state.once_pairing.store(false, Ordering::SeqCst);
            }
            match step.action {
                CompanionAction::Start => {
                    log::info!("waking the Android companion ({})", step.reason);
                    if let Err(e) = start_once(&app, &state).await {
                        log::warn!("could not wake the Android companion: {e}");
                    }
                }
                CompanionAction::Stop => {
                    log::info!("putting the Android companion to sleep ({})", step.reason);
                    if let Err(e) = stop_once(&app, &state).await {
                        log::warn!("could not put the Android companion to sleep: {e}");
                    }
                }
                CompanionAction::None => {}
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

/// What the companion manager needs to know this tick.
struct CompanionView {
    enabled: bool,
    pairing: bool,
    has_account: bool,
    paired: bool,
    running: bool,
    /// Pending one-time messages, before the state machine drops given-up ids.
    pending: Vec<(String, String)>,
}

/// The one thing to do with the instance after a tick.
#[derive(Debug, PartialEq, Eq)]
enum CompanionAction {
    None,
    Start,
    Stop,
}

struct CompanionStep {
    action: CompanionAction,
    /// Forget a pairing session that timed out.
    cancel_pairing: bool,
    /// Why, for the log line.
    reason: &'static str,
}

/// The companion manager's decision state: what it is still expecting to
/// fetch, what it already failed on this run, and the timers behind the
/// dormancy and give-up rules.
struct CompanionState {
    expecting: std::collections::HashSet<(String, String)>,
    ignored: std::collections::HashSet<(String, String)>,
    last_progress: std::time::Instant,
    pairing_since: Option<std::time::Instant>,
}

impl CompanionState {
    fn new() -> Self {
        Self {
            expecting: std::collections::HashSet::new(),
            ignored: std::collections::HashSet::new(),
            last_progress: std::time::Instant::now(),
            pairing_since: None,
        }
    }

    fn step(&mut self, view: &CompanionView, now: std::time::Instant) -> CompanionStep {
        if !view.has_account || (!view.enabled && !view.pairing) {
            // Off, or nothing to attach to: stop at once, no grace.
            self.expecting.clear();
            if !view.has_account {
                self.ignored.clear();
            }
            self.pairing_since = None;
            self.last_progress = now;
            let reason = if view.has_account { "disabled" } else { "no account" };
            return CompanionStep { action: stop_if_running(view), cancel_pairing: false, reason };
        }
        if view.pairing && !view.paired {
            // Pairing: stay up only long enough for the QR to be scanned.
            let started = *self.pairing_since.get_or_insert(now);
            if now.saturating_duration_since(started) >= ONCE_PAIR_TIMEOUT {
                self.pairing_since = None;
                return CompanionStep {
                    action: stop_if_running(view),
                    cancel_pairing: true,
                    reason: "pairing timed out",
                };
            }
            return CompanionStep { action: start_if_stopped(view), cancel_pairing: false, reason: "pairing" };
        }
        self.pairing_since = None;
        if !view.enabled || !view.paired {
            // Enabling requires a link; a revoked one stops the session.
            let reason = if view.pairing { "linking finished" } else { "link needed" };
            return CompanionStep { action: stop_if_running(view), cancel_pairing: view.pairing, reason };
        }
        // A scan that landed clears the pairing flag with the demand rules.
        let mut step = self.demand(view, now);
        step.cancel_pairing = view.pairing;
        step
    }

    /// Wake for pending one-time media, sleep once nothing is left, and give
    /// up on messages that never arrive.
    fn demand(&mut self, view: &CompanionView, now: std::time::Instant) -> CompanionStep {
        let pending: std::collections::HashSet<(String, String)> = view
            .pending
            .iter()
            .filter(|id| !self.ignored.contains(id))
            .cloned()
            .collect();
        if pending != self.expecting {
            self.last_progress = now;
            self.expecting = pending.clone();
        }
        if pending.is_empty() {
            if view.running && now.saturating_duration_since(self.last_progress) >= ONCE_STOP_GRACE {
                return CompanionStep { action: CompanionAction::Stop, cancel_pairing: false, reason: "done fetching" };
            }
            return CompanionStep { action: CompanionAction::None, cancel_pairing: false, reason: "dormant" };
        }
        if !view.running {
            return CompanionStep {
                action: CompanionAction::Start,
                cancel_pairing: false,
                reason: "one-time media waiting",
            };
        }
        if now.saturating_duration_since(self.last_progress) >= ONCE_GIVE_UP {
            log::warn!(
                "Android companion could not fetch {} one-time message(s); going dormant",
                pending.len()
            );
            self.ignored.extend(pending);
            self.expecting.clear();
            self.last_progress = now;
            return CompanionStep { action: CompanionAction::Stop, cancel_pairing: false, reason: "gave up" };
        }
        CompanionStep { action: CompanionAction::None, cancel_pairing: false, reason: "fetching" }
    }
}

fn start_if_stopped(view: &CompanionView) -> CompanionAction {
    if view.running { CompanionAction::None } else { CompanionAction::Start }
}

fn stop_if_running(view: &CompanionView) -> CompanionAction {
    if view.running { CompanionAction::Stop } else { CompanionAction::None }
}

/// Reads the current companion inputs, including the pending one-time media.
async fn observe_companion(state: &AppState) -> CompanionView {
    let enabled = state.settings.lock().unwrap().android_instance;
    let pairing = state.once_pairing.load(Ordering::SeqCst);
    let account = active_account(state);
    let paired = account.as_deref().is_some_and(|id| once_paired(state, id));
    let running = state.once_service.lock().unwrap().is_some();
    let service = state.service.lock().unwrap().clone();
    let pending = match service {
        Some(service) => service.pending_view_once(ONCE_RECOVERY_WINDOW).await,
        None => Vec::new(),
    };
    CompanionView { enabled, pairing, has_account: account.is_some(), paired, running, pending }
}


/// Forwards the main session's events to the UI. `events` was registered
/// before the connection attempt, so the pairing code cannot slip through the
/// gap between starting and subscribing. The service is held weakly: once the
/// active slot drops it, a swap lets the loop, and with it the old session
/// files, go.
fn spawn_main_events(
    app: &AppHandle,
    service: &Arc<WhatsAppService>,
    account: &str,
    mut events: tokio::sync::broadcast::Receiver<ServiceEvent>,
) {
    let emitter = app.clone();
    let service_for_events = Arc::downgrade(service);
    let account_id = account.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(service) = service_for_events.upgrade() else { break };
                    let state = emitter.state::<AppState>();
                    if !current_main_event(&state, &account_id, &service) { break; }
                    if matches!(event, ServiceEvent::LoggedOut) {
                        emitter.state::<crate::transcription::TranscriptionState>().cancel_all();
                        log::warn!("account {account_id} was logged out; its session is dropped");
                        forget_session(&emitter, &account_id, &service);
                        emit_service_event(&emitter, &event);
                        // Holding the service keeps its session database open.
                        break;
                    }
                    if matches!(event, ServiceEvent::Disconnected) {
                        emitter.state::<crate::transcription::TranscriptionState>().cancel_all();
                    }
                    crate::transcription::schedule_auto(&emitter, &account_id, &event);
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
                    let state = emitter.state::<AppState>();
                    if !current_main_event(&state, &account_id, &service) { break; }
                    emit_service_event(&emitter, &resync_event(&service));
                }
                Err(_) => break,
            }
        }
    });
}

fn event_owner_matches(active: Option<&str>, account: &str, slot_matches: bool, binding_matches: bool) -> bool {
    active == Some(account) && slot_matches && binding_matches
}

fn current_main_event(state: &AppState, account: &str, service: &Arc<WhatsAppService>) -> bool {
    let active = active_account(state);
    let bound = state.account_service.lock().unwrap().as_ref().is_some_and(|(id, weak)|
        id == account && weak.upgrade().is_some_and(|bound| Arc::ptr_eq(&bound, service)));
    let slot = state.service.lock().unwrap().as_ref().is_some_and(|current| Arc::ptr_eq(current, service));
    event_owner_matches(active.as_deref(), account, slot, bound)
}

fn current_once_event(state: &AppState, account: &str, service: &Arc<WhatsAppService>) -> bool {
    let active = active_account(state);
    let slot = state.once_service.lock().unwrap().as_ref().is_some_and(|current| Arc::ptr_eq(current, service));
    event_owner_matches(active.as_deref(), account, slot, true)
}

/// Store changes the Android instance makes that the main UI must reload
/// for. Its per-message traffic is catch-up replay the main link reports
/// itself; forwarding it floods the shared channel and starves these sparse
/// events, losing exactly the kept one-time media that needs announcing.
pub(crate) fn instance_store_event(event: &ServiceEvent) -> bool {
    matches!(
        event,
        ServiceEvent::Marks { .. }
            | ServiceEvent::ChatStateChanged { .. }
            | ServiceEvent::RetentionApplied { .. }
    )
}

/// Events the companion's own sheet reacts to: its link's state and the
/// store changes it caused.
pub(crate) fn instance_sheet_event(event: &ServiceEvent) -> bool {
    instance_store_event(event)
        || matches!(
            event,
            ServiceEvent::QrCode { .. }
                | ServiceEvent::PairingCode { .. }
                | ServiceEvent::PairingCodeRefresh { .. }
                | ServiceEvent::PairingCodeError { .. }
                | ServiceEvent::Connected
                | ServiceEvent::Disconnected
                | ServiceEvent::LoggedOut
        )
}

/// Its own event stream drives the pairing sheet and forwards the store
/// changes the main UI must reload for. Held weakly so a stop drops it.
fn spawn_instance_events(
    app: &AppHandle,
    service: &Arc<WhatsAppService>,
    account: &str,
    mut events: tokio::sync::broadcast::Receiver<ServiceEvent>,
) {
    let emitter = app.clone();
    let service_for_events = Arc::downgrade(service);
    let account_id = account.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let state = emitter.state::<AppState>();
                    let Some(service) = service_for_events.upgrade() else { break };
                    if !current_once_event(&state, &account_id, &service) { break; }
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
                            forget_once_session(&emitter, &account_id, &service);
                            let _ = emitter.emit_to("main", ONCE_EVENT, &event);
                            break;
                        }
                        // The shared store changed under the main session; let
                        // the main UI reload without duplicating the instance's
                        // catch-up replay.
                        _ if instance_store_event(&event) => {
                            log::debug!("Android companion store change: {event:?}");
                            emit_service_event(&emitter, &event);
                            // A kept one-time clears the demand; re-check soon.
                            emitter.state::<AppState>().once_wake.notify_one();
                        }
                        _ => {
                            emitter.state::<AppState>().once_wake.notify_one();
                        }
                    }
                    // The companion sheet only cares about its own link state
                    // and the store changes it caused; forwarding per-message
                    // catch-up would make it poll once_state thousands of
                    // times per wake.
                    if instance_sheet_event(&event) {
                        let _ = emitter.emit_to("main", ONCE_EVENT, &event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    // The instance's catch-up can outrun this loop, and the
                    // skipped events name no chat; ask for a blanket reload.
                    log::warn!("Android companion events fell behind, dropped {dropped} event(s)");
                    let Some(service) = service_for_events.upgrade() else { break };
                    let state = emitter.state::<AppState>();
                    if !current_once_event(&state, &account_id, &service) { break; }
                    emit_service_event(&emitter, &ServiceEvent::StoreChanged);
                }
                Err(_) => break,
            }
        }
        let state = emitter.state::<AppState>();
        if service_for_events.upgrade().is_some_and(|service| current_once_event(&state, &account_id, &service)) {
            state.once_connected.store(false, Ordering::SeqCst);
        }
    });
}

fn emit_service_event(app: &AppHandle, event: &ServiceEvent) {
    crate::plugins::publish(&app.state::<AppState>().plugins, event);
    crate::floating::notify(app, event);
    let mut rendered = event.clone();
    if let ServiceEvent::Message { message, .. } = &mut rendered { crate::media_access::prepare_message(app, message); }
    if let Err(error) = app.emit_to("main", SERVICE_EVENT, &rendered) {
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
pub(crate) async fn start_once(app: &AppHandle, state: &AppState) -> CommandResult<()> {
    if state.once_service.lock().unwrap().is_some() {
        return Ok(());
    }
    let Some(account) = active_account(state) else {
        return Err(CommandError::code("error.no_account"));
    };
    let settings = state.settings.lock().unwrap().clone();
    let database_key = app.state::<crate::database_encryption::DatabaseEncryption>().key(&account)
        .map_err(crate::database_encryption::command_failure)?;
    crate::account_store::migrate_history(app, &settings, &account);
    let mut config = once_config_for(app, &settings, &account)?;
    config.database_key = database_key;
    remove_stale_sessions(&account_base(app, &account), &current_sessions(&account_base(app, &account)));

    let (service, events) = WhatsAppService::start(config).await.map_err(|e| {
        log::error!("failed to start the Android instance: {e:#}");
        CommandError::code("error.companion_start_failed").with_diagnostic(e)
    })?;
    *state.once_qr.lock().unwrap() = service.current_qr();
    let service = Arc::new(service);
    *state.once_service.lock().unwrap() = Some(service.clone());
    spawn_instance_events(app, &service, &account, events);
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
    let _ = app.emit_to("main", ONCE_EVENT, &ServiceEvent::Disconnected);
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
pub(crate) fn set_pairing(app: AppHandle, state: State<'_, AppState>, pairing: bool) -> CommandResult<()> {
    if pairing {
        // Fail here, where the UI can show it, instead of leaving the pairing
        // screen without a QR until the session times out.
        if !state.settings.lock().unwrap().keep_history {
            return Err(CommandError::code("error.companion_history_required"));
        }
        if active_account(&state).is_none() {
            return Err(CommandError::code("error.no_account"));
        }
    }
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
    crate::floating::invalidate_all(app);
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
    state.once_pairing.store(false, Ordering::SeqCst);
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

/// Mints a phone-number pairing code for the main account or the Android
/// companion. The code arrives as a `pairingCode` event; this only reports
/// immediate failures.
#[tauri::command(async)]
pub(crate) async fn request_pair_code(state: State<'_, AppState>, phone: String, companion: bool) -> CommandResult<()> {
    let service = if companion {
        state.once_service.lock().unwrap().clone().ok_or_else(|| CommandError::code("error.companion_not_pairing"))?
    } else {
        state.service().map_err(|_| CommandError::code("error.not_connected"))?
    };
    service.request_pair_code(&phone).await.map_err(|error| CommandError::code("error.pair_code_failed").with_diagnostic(error))
}

/// Withdraws an outstanding pairing code so the QR (or a new code) takes over.
#[tauri::command(async)]
pub(crate) async fn cancel_pair_code(state: State<'_, AppState>, companion: bool) -> CommandResult<()> {
    let service = if companion {
        state.once_service.lock().unwrap().clone()
    } else {
        state.service.lock().unwrap().clone()
    };
    if let Some(service) = service {
        service.cancel_pair_code().await;
    }
    Ok(())
}

/// Connects the active account, pairing by QR the first time.
#[tauri::command]
pub(crate) async fn connect(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
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

#[cfg(test)]
mod companion_tests {
    use super::*;

    fn view(running: bool, pending: &[(&str, &str)]) -> CompanionView {
        CompanionView {
            enabled: true,
            pairing: false,
            has_account: true,
            paired: true,
            running,
            pending: pending.iter().map(|(chat, id)| (chat.to_string(), id.to_string())).collect(),
        }
    }

    fn secs(n: u64) -> std::time::Duration {
        std::time::Duration::from_secs(n)
    }

    #[test]
    fn stale_main_events_stop_after_account_switch_or_restart() {
        assert!(!event_owner_matches(Some("new"), "old", true, true));
        assert!(!event_owner_matches(Some("same"), "same", false, true));
    }

    #[test]
    fn old_companion_logout_cannot_mutate_new_account_state() {
        assert!(!event_owner_matches(Some("new"), "old", false, true));
    }

    #[test]
    fn sleeping_companion_wakes_for_one_time_media_and_dozes_after_grace() {
        let mut state = CompanionState::new();
        let base = std::time::Instant::now();
        assert_eq!(state.step(&view(false, &[("a", "1")]), base).action, CompanionAction::Start);

        let idle = state.step(&view(true, &[]), base + secs(5));
        assert_eq!(idle.action, CompanionAction::None);
        let stop = state.step(&view(true, &[]), base + secs(5) + ONCE_STOP_GRACE + secs(1));
        assert_eq!(stop.action, CompanionAction::Stop);
    }

    #[test]
    fn companion_gives_up_on_media_it_never_receives() {
        let mut state = CompanionState::new();
        let base = std::time::Instant::now();
        assert_eq!(state.step(&view(false, &[("a", "1")]), base).action, CompanionAction::Start);

        let waiting = state.step(&view(true, &[("a", "1")]), base + secs(10));
        assert_eq!(waiting.action, CompanionAction::None);
        let gave_up = state.step(&view(true, &[("a", "1")]), base + ONCE_GIVE_UP + secs(1));
        assert_eq!(gave_up.action, CompanionAction::Stop);
        assert_eq!(gave_up.reason, "gave up");

        // The same message is ignored for the rest of the run...
        let ignored = state.step(&view(false, &[("a", "1")]), base + ONCE_GIVE_UP + secs(2));
        assert_eq!(ignored.action, CompanionAction::None);
        // ...but a new one wakes it again.
        let fresh = state.step(&view(false, &[("a", "1"), ("b", "2")]), base + ONCE_GIVE_UP + secs(3));
        assert_eq!(fresh.action, CompanionAction::Start);
    }

    #[test]
    fn new_one_time_media_resets_the_give_up_timer() {
        let mut state = CompanionState::new();
        let base = std::time::Instant::now();
        assert_eq!(state.step(&view(false, &[("a", "1")]), base).action, CompanionAction::Start);
        assert_eq!(state.step(&view(true, &[("a", "1"), ("b", "2")]), base + secs(60)).action, CompanionAction::None);

        // 90s after the newest demand, not after the wake.
        assert_eq!(state.step(&view(true, &[("a", "1"), ("b", "2")]), base + secs(140)).action, CompanionAction::None);
        assert_eq!(state.step(&view(true, &[("a", "1"), ("b", "2")]), base + secs(152)).action, CompanionAction::Stop);
    }

    #[test]
    fn pairing_runs_until_scanned_or_timed_out() {
        let mut state = CompanionState::new();
        let base = std::time::Instant::now();
        let mut pairing = view(false, &[]);
        pairing.pairing = true;
        pairing.paired = false;
        assert_eq!(state.step(&pairing, base).action, CompanionAction::Start);

        pairing.running = true;
        assert_eq!(state.step(&pairing, base + secs(60)).action, CompanionAction::None);
        let timed_out = state.step(&pairing, base + ONCE_PAIR_TIMEOUT + secs(1));
        assert!(timed_out.cancel_pairing);
        assert_eq!(timed_out.action, CompanionAction::Stop);

        // A scan that landed clears the pairing flag and winds the session down.
        let mut paired = view(true, &[]);
        paired.pairing = true;
        let step = state.step(&paired, base + ONCE_PAIR_TIMEOUT + secs(2));
        assert!(step.cancel_pairing);
        assert_eq!(step.action, CompanionAction::Stop);
    }

    #[test]
    fn turning_the_companion_off_sleeps_it_immediately() {
        let mut state = CompanionState::new();
        let mut off = view(true, &[("a", "1")]);
        off.enabled = false;
        let step = state.step(&off, std::time::Instant::now());
        assert_eq!(step.action, CompanionAction::Stop);
        assert_eq!(step.reason, "disabled");
    }
}
