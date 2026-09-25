//! Tauri shell for Hermóðr.
//!
//! The window only renders our own UI; WhatsApp is spoken natively by
//! [`hermodr_core`]. There is no webview pointed at a remote site, so none of
//! the history, memory, or compositing problems of that approach apply.

use std::{
    io::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use hermodr_core::{
    ChatSummary, Retention, SendOptions, Service, ServiceConfig, ServiceEvent, StoredMessage,
    VoiceNote,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Event name the frontend listens on for service updates.
const SERVICE_EVENT: &str = "service-event";

/// Settings the UI can change.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiSettings {
    pub retention: Retention,
    /// Whether to pull the account's entire history during pairing.
    pub accept_full_history: bool,
    /// Where downloaded media is stored. Empty disables downloads.
    pub media_dir: Option<String>,
    /// Whether to download incoming media automatically.
    #[serde(default = "default_true")]
    pub auto_download_media: bool,
    /// Whether to warn when a video goes out without a preview.
    #[serde(default = "default_true")]
    pub warn_missing_video_preview: bool,
    /// Whether others see "typing…" while we write.
    #[serde(default = "default_true")]
    pub send_typing: bool,
    /// Whether senders learn we read or played their messages. Off covers
    /// groups too, which WhatsApp's own read-receipt privacy does not.
    #[serde(default = "default_true")]
    pub send_receipts: bool,
    /// Whether messages are kept on disk. Off keeps them in memory for this run only.
    #[serde(default = "default_true")]
    pub keep_history: bool,
}

fn default_true() -> bool {
    true
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            retention: Retention::default(),
            accept_full_history: true,
            auto_download_media: true,
            media_dir: None,
            warn_missing_video_preview: true,
            send_typing: true,
            send_receipts: true,
            keep_history: true,
        }
    }
}

fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

/// Saved settings, or the defaults when none were saved or they do not parse.
fn load_settings(app: &AppHandle) -> UiSettings {
    std::fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

struct AppState {
    service: Mutex<Option<Arc<Service>>>,
    settings: Mutex<UiSettings>,
    accounts: Mutex<AccountsFile>,
}

/// One signed-in account.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    /// Learned once the account connects, so its picture shows while inactive.
    #[serde(default)]
    pub jid: Option<String>,
}

/// The account list, persisted next to the app config.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct AccountsFile {
    accounts: Vec<Account>,
    active: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AccountsView {
    pub accounts: Vec<Account>,
    pub active: Option<String>,
}

impl AppState {
    fn service(&self) -> Result<Arc<Service>, String> {
        self.service
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "not connected yet".to_string())
    }
}

/// Where this account's data lives.
fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Where an account's files live. The first account keeps the old layout so an
/// existing single-account install is adopted in place.
fn account_base(app: &AppHandle, id: &str) -> std::path::PathBuf {
    if id == "default" {
        data_dir(app)
    } else {
        data_dir(app).join("accounts").join(id)
    }
}

fn accounts_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("accounts.json")
}

fn save_accounts(app: &AppHandle, file: &AccountsFile) {
    let path = accounts_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(file) {
        let _ = std::fs::write(path, json);
    }
}

/// Loads the account list, adopting an existing single-account install the
/// first time.
fn load_accounts(app: &AppHandle) -> AccountsFile {
    if let Ok(json) = std::fs::read_to_string(accounts_path(app)) {
        if let Ok(file) = serde_json::from_str::<AccountsFile>(&json) {
            return file;
        }
    }
    let mut file = AccountsFile::default();
    if data_dir(app).join("session.db").exists() {
        file.accounts.push(Account {
            id: "default".into(),
            label: "WhatsApp".into(),
            jid: None,
        });
        file.active = Some("default".into());
        save_accounts(app, &file);
    }
    file
}

fn active_account(state: &AppState) -> Option<String> {
    state.accounts.lock().unwrap().active.clone()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Where downloads live by default. The cache, because media is regenerable and
/// should not be carried in a backup of the account.
fn media_cache_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("media"))
        .unwrap_or_else(|_| data_dir(app).join("media"))
}

/// Moves media out of the folders earlier versions used: next to the session,
/// then the shared cache root (`~/.cache/media`, `%LOCALAPPDATA%\media`).
fn migrate_media(app: &AppHandle, accounts: &AccountsFile) {
    let legacy: Vec<PathBuf> = [Ok(data_dir(app).join("media")), app.path().cache_dir().map(|d| d.join("media"))]
        .into_iter()
        .flatten()
        .filter(|dir| dir.is_dir())
        .collect();
    if legacy.is_empty() {
        return;
    }
    let from: Vec<&std::path::Path> = legacy.iter().map(PathBuf::as_path).collect();
    let to = media_cache_dir(app);
    for account in &accounts.accounts {
        let db = account_base(app, &account.id).join("messages.db");
        if !db.exists() {
            continue;
        }
        match hermodr_core::MessageStore::open(&db, Retention::default()) {
            Ok(store) => {
                if let Err(e) = store.relocate_media(&from, &to) {
                    log::warn!("media migration for {}: {e}", account.id);
                }
            }
            Err(e) => log::warn!("media migration for {}: {e}", account.id),
        }
    }
    for dir in &legacy {
        // Only succeeds once empty, so another program's files keep the folder.
        let _ = std::fs::remove_dir(dir);
    }
}

fn config_for(app: &AppHandle, settings: &UiSettings, account: &str) -> ServiceConfig {
    let base = account_base(app, account);
    let default_media = media_cache_dir(app);
    ServiceConfig {
        session_path: session_path(&base),
        messages_path: if settings.keep_history {
            base.join("messages.db")
        } else {
            PathBuf::from(":memory:")
        },
        retention: settings.retention,
        accept_full_history: settings.accept_full_history,
        auto_download_media: settings.auto_download_media,
        // An unset or empty setting falls back to the app data directory.
        media_dir: settings
            .media_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or(Some(default_media)),
    }
}

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
fn resync_event(service: &Service) -> ServiceEvent {
    if service.is_connected() {
        ServiceEvent::Connected
    } else if let Some(code) = service.current_qr() {
        ServiceEvent::QrCode { code }
    } else {
        ServiceEvent::Disconnected
    }
}

#[tauri::command(async)]
fn connection_state(state: State<'_, AppState>) -> ConnectionState {
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
async fn start_service(app: &AppHandle, state: &AppState, account: &str) -> Result<(), String> {
    log::info!("starting account {account}");
    // Stop whatever is running first, so the old account disconnects.
    if let Some(existing) = state.service.lock().unwrap().take() {
        existing.shutdown();
    }

    let settings = state.settings.lock().unwrap().clone();
    let config = config_for(app, &settings, account);

    remove_stale_sessions(&account_base(app, account), &config.session_path);

    let (service, mut events) = Service::start(config).await.map_err(|e| {
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

/// Names the account's current session file; absent means `session.db`.
const SESSION_POINTER: &str = "session_name";

fn session_path(base: &std::path::Path) -> PathBuf {
    let name = std::fs::read_to_string(base.join(SESSION_POINTER)).unwrap_or_default();
    base.join(match name.trim() {
        "" => "session.db",
        name => name,
    })
}

/// Deletes session files other than `current`; one still held open is retried on a later start.
fn remove_stale_sessions(base: &std::path::Path, current: &std::path::Path) {
    let Some(current) = current.file_name().and_then(|n| n.to_str()) else { return };
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_stale_session(name, current) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// True for an old `session*.db` file or its `-wal`/`-shm`/`-journal`; never
/// for the current database or its own sidecars.
fn is_stale_session(name: &str, current: &str) -> bool {
    let db = ["-wal", "-shm", "-journal"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    db.starts_with("session") && db.ends_with(".db") && db != current
}

#[cfg(test)]
mod tests {
    use super::is_stale_session;

    #[test]
    fn stale_sessions_spare_the_active_wal() {
        assert!(!is_stale_session("session.db", "session.db"));
        assert!(!is_stale_session("session.db-wal", "session.db"));
        assert!(!is_stale_session("session.db-shm", "session.db"));
        assert!(is_stale_session("session.db-wal", "session-1.db"));
        assert!(is_stale_session("session-1.db", "session-2.db"));
        assert!(is_stale_session("session-1.db-shm", "session-2.db"));
        assert!(!is_stale_session("session-2.db-wal", "session-2.db"));
        assert!(!is_stale_session("session.dbx", "session-2.db"));
        assert!(!is_stale_session("messages.db", "session.db"));
    }
}

/// Stops a revoked account's service and points it at a fresh session file.
///
/// The old file cannot be deleted yet: Windows keeps it locked until the
/// library releases its connection pool.
fn forget_session(app: &AppHandle, account: &str, service: &Arc<Service>) {
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
async fn connect(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.service.lock().unwrap().is_some() {
        return Ok(());
    }
    let account = match active_account(&state) {
        Some(account) => account,
        // First run: create the default account.
        None => {
            {
                let mut file = state.accounts.lock().unwrap();
                file.accounts.push(Account {
                    id: "default".into(),
                    label: "WhatsApp".into(),
                    jid: None,
                });
                file.active = Some("default".into());
            }
            save_accounts(&app, &state.accounts.lock().unwrap());
            "default".to_string()
        }
    };
    start_service(&app, &state, &account).await
}

/// The accounts and which one is active.
#[tauri::command(async)]
fn accounts(state: State<'_, AppState>) -> AccountsView {
    let file = state.accounts.lock().unwrap();
    AccountsView {
        accounts: file.accounts.clone(),
        active: file.active.clone(),
    }
}

/// Adds an account and switches to it, which starts pairing.
#[tauri::command]
async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    label: Option<String>,
) -> Result<(), String> {
    let id = format!("acct-{}", now_millis());
    {
        let mut file = state.accounts.lock().unwrap();
        file.accounts.push(Account {
            id: id.clone(),
            label: label.unwrap_or_else(|| "WhatsApp".into()),
            jid: None,
        });
        file.active = Some(id.clone());
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    start_service(&app, &state, &id).await
}

/// Switches to another account, disconnecting the current one.
#[tauri::command]
async fn switch_account(
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
fn rename_account(
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
async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    log::info!("removing account {id}");
    {
        let mut file = state.accounts.lock().unwrap();
        file.accounts.retain(|a| a.id != id);
        if file.active.as_deref() == Some(id.as_str()) {
            file.active = file.accounts.first().map(|a| a.id.clone());
        }
    }
    save_accounts(&app, &state.accounts.lock().unwrap());
    if let Some(existing) = state.service.lock().unwrap().take() {
        existing.shutdown();
    }
    if id != "default" {
        let _ = std::fs::remove_dir_all(account_base(&app, &id));
    }
    if let Some(next) = active_account(&state) {
        start_service(&app, &state, &next).await?;
    }
    Ok(())
}

/// Stored messages for a chat, newest first.
#[tauri::command(async)]
fn messages(
    state: State<'_, AppState>,
    chat: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .messages(&chat, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Chat summaries, most recently active first.
#[tauri::command(async)]
fn chats(state: State<'_, AppState>) -> Result<Vec<ChatSummary>, String> {
    state.service()?.chats().map_err(|e| e.to_string())
}

/// Resolves display names for chats that still show a raw number.
///
/// Returns how many were resolved; the UI refreshes when that is non-zero.
#[tauri::command]
async fn resolve_names(state: State<'_, AppState>) -> Result<usize, String> {
    let service = state.service()?;
    service.resolve_missing_names().await.map_err(|e| e.to_string())
}

/// Marks a chat as read. Returns how many messages were newly marked.
#[tauri::command]
async fn mark_read(state: State<'_, AppState>, chat: String) -> Result<usize, String> {
    let service = state.service()?;
    let receipts =
        state.settings.lock().unwrap().send_receipts && !service.read_receipts_disabled();
    service.mark_read(&chat, receipts).await.map_err(|e| e.to_string())
}

/// Sends a played receipt for a voice note or view-once media, unless receipts are off.
#[tauri::command]
async fn mark_played(state: State<'_, AppState>, chat: String, id: String, sender: String) -> Result<(), String> {
    let service = state.service()?;
    if !state.settings.lock().unwrap().send_receipts || service.read_receipts_disabled() {
        return Ok(());
    }
    service.mark_played(&chat, &id, &sender).await.map_err(|e| e.to_string())
}

/// Sends a text message quoting an earlier one.
#[tauri::command]
async fn send_reply(
    state: State<'_, AppState>,
    chat: String,
    text: String,
    reply_to_id: String,
    reply_to_sender: String,
    reply_to_text: String,
    mentions: Option<Vec<String>>,
    reply_to_chat: Option<String>,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .send_reply(
            &chat,
            text,
            &reply_to_id,
            &reply_to_sender,
            &reply_to_text,
            mentions.unwrap_or_default(),
            reply_to_chat.as_deref().filter(|c| *c != chat),
        )
        .await
        .map_err(|e| e.to_string())
}

/// The message a context-menu action applies to.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    chat: String,
    id: String,
    sender: String,
    from_me: bool,
}

#[tauri::command]
async fn react(state: State<'_, AppState>, target: Target, emoji: String) -> Result<(), String> {
    state
        .service()?
        .react(&target.chat, &target.id, &target.sender, target.from_me, &emoji)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn star(state: State<'_, AppState>, target: Target, starred: bool) -> Result<(), String> {
    state
        .service()?
        .star(&target.chat, &target.id, &target.sender, target.from_me, starred)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pin_message(state: State<'_, AppState>, target: Target, pinned: bool) -> Result<(), String> {
    state
        .service()?
        .pin_message(&target.chat, &target.id, &target.sender, target.from_me, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_message(
    state: State<'_, AppState>,
    target: Target,
    everyone: bool,
    timestamp: i64,
) -> Result<(), String> {
    let service = state.service()?;
    let done = if everyone {
        service
            .delete_for_everyone(&target.chat, &target.id, &target.sender, target.from_me)
            .await
    } else {
        service
            .delete_for_me(&target.chat, &target.id, &target.sender, target.from_me, timestamp)
            .await
    };
    done.map_err(|e| e.to_string())
}

#[tauri::command]
async fn report_message(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    state.service()?.report_to_admins(&chat, &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn forward_message(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    to: String,
) -> Result<(), String> {
    state.service()?.forward(&chat, &id, &to).await.map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn marks(state: State<'_, AppState>, chat: String) -> Result<hermodr_core::ChatMarks, String> {
    state.service()?.marks(&chat).map_err(|e| e.to_string())
}

/// Sends an attachment as an image or document.
///
/// The file arrives base64-encoded because the webview cannot hand out a real
/// filesystem path, and the plugin that could is not usable alongside the
/// pinned Tauri checkout.
#[tauri::command]
async fn send_media(
    state: State<'_, AppState>,
    chat: String,
    name: String,
    data: String,
    caption: Option<String>,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
    gif: Option<bool>,
    view_once: Option<bool>,
    mentions: Option<Vec<String>>,
    progress: Option<String>,
) -> Result<Option<String>, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        gif: gif.unwrap_or(false),
        view_once: view_once.unwrap_or(false),
        mentions: mentions.unwrap_or_default(),
        progress,
        ..Default::default()
    };
    let service = state.service()?;
    service
        .send_media(&chat, &name, bytes, caption, reply, options)
        .await
        .map_err(|e| e.to_string())
}

/// Sends a voice note recorded by the webview (WebM/Opus, base64).
#[tauri::command]
async fn send_voice(
    state: State<'_, AppState>,
    chat: String,
    data: String,
    seconds: u32,
    waveform: Vec<u8>,
    reply_to_id: Option<String>,
    reply_to_sender: Option<String>,
    reply_to_text: Option<String>,
    view_once: Option<bool>,
) -> Result<(), String> {
    let webm = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    let ogg = hermodr_core::ogg::webm_to_ogg(&webm).map_err(|e| e.to_string())?;
    let reply = match (reply_to_id, reply_to_sender, reply_to_text) {
        (Some(id), Some(sender), Some(text)) => Some((id, sender, text)),
        _ => None,
    };
    let options = SendOptions {
        view_once: view_once.unwrap_or(false),
        voice: Some(VoiceNote { seconds, waveform }),
        ..Default::default()
    };
    state
        .service()?
        .send_media(&chat, "voice.ogg", ogg, None, reply, options)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_poll(
    state: State<'_, AppState>,
    chat: String,
    question: String,
    options: Vec<String>,
    multi: bool,
) -> Result<(), String> {
    state
        .service()?
        .create_poll(&chat, question.trim(), options, multi)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn vote_poll(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    options: Vec<String>,
) -> Result<(), String> {
    state.service()?.vote_poll(&chat, &id, options).await.map_err(|e| e.to_string())
}

/// An event as the create dialog fills it; times are Unix seconds.
#[derive(serde::Deserialize)]
struct EventForm {
    name: String,
    description: Option<String>,
    start: Option<i64>,
    end: Option<i64>,
    location: Option<String>,
    link: Option<String>,
    #[serde(default)]
    canceled: bool,
}

impl From<EventForm> for hermodr_core::NewEvent {
    fn from(event: EventForm) -> Self {
        Self {
            name: event.name.trim().to_string(),
            description: event.description.filter(|s| !s.trim().is_empty()),
            start: event.start,
            end: event.end,
            location: event.location.filter(|s| !s.trim().is_empty()),
            link: event.link.filter(|s| !s.trim().is_empty()),
            canceled: event.canceled,
        }
    }
}

#[tauri::command]
async fn create_event(state: State<'_, AppState>, chat: String, event: EventForm) -> Result<(), String> {
    state.service()?.create_event(&chat, event.into()).await.map_err(|e| e.to_string())
}

/// Edits or cancels one of our events.
#[tauri::command]
async fn edit_event(state: State<'_, AppState>, chat: String, id: String, event: EventForm) -> Result<(), String> {
    state.service()?.edit_event(&chat, &id, event.into()).await.map_err(|e| e.to_string())
}

/// Who got, read and played one of our messages.
#[tauri::command(async)]
fn message_info(state: State<'_, AppState>, id: String) -> Result<Vec<hermodr_core::MessageReceipt>, String> {
    state.service()?.message_info(&id).map_err(|e| e.to_string())
}

/// Starred messages across every chat, newest first.
#[tauri::command(async)]
fn starred_messages(state: State<'_, AppState>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.starred_messages().map_err(|e| e.to_string())
}

/// Messages that mention us, in one chat or (without `chat`) all of them.
#[tauri::command(async)]
fn pings(state: State<'_, AppState>, chat: Option<String>) -> Result<Vec<StoredMessage>, String> {
    state.service()?.pings(chat.as_deref()).map_err(|e| e.to_string())
}

/// Up to `limit` (default 50) messages in one chat whose text contains `query`.
#[tauri::command(async)]
fn search_messages(
    state: State<'_, AppState>,
    chat: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<StoredMessage>, String> {
    state
        .service()?
        .search_messages(&chat, &query, limit.unwrap_or(50).clamp(1, 500))
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ChatSettings {
    /// The chat's auto download override, `None` when it follows the global one.
    auto_download: Option<bool>,
    retention: hermodr_core::ChatRetention,
}

#[tauri::command(async)]
fn chat_settings(state: State<'_, AppState>, chat: String) -> Result<ChatSettings, String> {
    let service = state.service()?;
    Ok(ChatSettings {
        auto_download: service.chat_auto_download(&chat).map_err(|e| e.to_string())?,
        retention: service.chat_retention(&chat).map_err(|e| e.to_string())?,
    })
}

#[tauri::command(async)]
fn set_chat_retention(
    state: State<'_, AppState>,
    chat: String,
    retention: hermodr_core::ChatRetention,
) -> Result<(), String> {
    state.service()?.set_chat_retention(&chat, &retention).map_err(|e| e.to_string())
}

/// Messages reported to a group's admins.
#[tauri::command]
async fn admin_reports(state: State<'_, AppState>, chat: String) -> Result<Vec<hermodr_core::AdminReport>, String> {
    state.service()?.admin_reports(&chat).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_allow_admin_reports(state: State<'_, AppState>, chat: String, allow: bool) -> Result<(), String> {
    state.service()?.set_allow_admin_reports(&chat, allow).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn respond_event(
    state: State<'_, AppState>,
    chat: String,
    id: String,
    response: String,
) -> Result<(), String> {
    state
        .service()?
        .respond_event(&chat, &id, &response)
        .await
        .map_err(|e| e.to_string())
}

/// Sends base64 image bytes as a sticker.
#[tauri::command]
async fn send_sticker(state: State<'_, AppState>, chat: String, data: String) -> Result<(), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    state.service()?.send_sticker(&chat, bytes).await.map_err(|e| e.to_string())
}

/// Saves base64 image bytes as a sticker without sending it; returns its path.
#[tauri::command(async)]
fn save_sticker(state: State<'_, AppState>, data: String) -> Result<String, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    state.service()?.save_sticker(&bytes).map_err(|e| e.to_string())
}

/// Stickers or GIFs already downloaded, newest first.
#[tauri::command(async)]
fn media_library(
    state: State<'_, AppState>,
    kind: String,
    prefer: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    state
        .service()?
        .media_library(&kind, &prefer.unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_from_library(
    state: State<'_, AppState>,
    chat: String,
    path: String,
    kind: String,
) -> Result<(), String> {
    state
        .service()?
        .send_from_library(&chat, &path, &kind)
        .await
        .map_err(|e| e.to_string())
}

/// Opens a downloaded media file with the desktop's default application.
///
/// The path is restricted to the configured media folder. The webview is the
/// least trusted part of the app, and it must not be able to ask the shell to
/// open arbitrary files.
#[tauri::command(async)]
fn open_path(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let configured = state
        .service
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|service| service.media_dir())
        .or_else(|| {
            let account = active_account(&state)?;
            let settings = state.settings.lock().unwrap().clone();
            config_for(&app, &settings, &account).media_dir
        });

    let dir = configured
        .and_then(|dir| dunce::canonicalize(dir).ok())
        .ok_or("no media folder is configured")?;
    let target = dunce::canonicalize(&path).map_err(|e| e.to_string())?;
    if !target.starts_with(&dir) {
        return Err("refusing to open a file outside the media folder".into());
    }
    shell_open(target.as_os_str())
}

/// Opens a file or URL with the desktop's default handler.
fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(target).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(target).spawn().map_err(|e| e.to_string())?;
    // Not `cmd /C start`: cmd re-parses the target, so a `&` in a URL runs a
    // command, and it flashes a console window.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let file: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as usize;
        // Values above 32 mean success.
        if code <= 32 {
            return Err(format!("the shell could not open it (error {code})"));
        }
    }
    Ok(())
}

/// Media extensions the renderer may read.
const READABLE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "mp4", "mov", "m4v", "webm", "mkv", "ogg", "opus", "mp3",
    "m4a", "aac", "wav",
];

/// Reads a media file and returns it base64-encoded.
///
/// WebKitGTK's media pipeline cannot load the custom asset scheme, so audio and
/// video have to arrive as bytes and be turned into a blob URL by the page. The
/// extension allowlist keeps this from becoming a general file-read primitive,
/// which matters because a pasted file can live anywhere on disk.
#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    let extension = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !READABLE_EXTENSIONS.contains(&extension.as_str()) {
        return Err("unsupported file type".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(BASE64.encode(bytes))
}

/// Sends a text message to a chat.
#[tauri::command]
async fn send_text(
    state: State<'_, AppState>,
    chat: String,
    text: String,
    mentions: Option<Vec<String>>,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .send_text(&chat, text, mentions.unwrap_or_default())
        .await
        .map_err(|e| e.to_string())
}

/// Group members for mention autocomplete.
#[tauri::command]
async fn participants(
    state: State<'_, AppState>,
    chat: String,
) -> Result<Vec<hermodr_core::Participant>, String> {
    state
        .service()?
        .participants(&chat)
        .await
        .map_err(|e| e.to_string())
}

/// Group subject, description and members, for the info sidebar.
#[tauri::command]
async fn group_info(
    state: State<'_, AppState>,
    chat: String,
) -> Result<hermodr_core::GroupInfo, String> {
    state
        .service()?
        .group_info(&chat)
        .await
        .map_err(|e| e.to_string())
}

/// Community parents and subgroups among the account's groups, by JID.
#[tauri::command]
async fn group_kinds(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, hermodr_core::GroupKind>, String> {
    Ok(state.service()?.group_kinds().await)
}

/// Opens an http(s) URL in the desktop's default browser.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) links are opened".into());
    }
    shell_open(url.as_ref())
}

/// Chats, contacts and groups matching a query.
#[tauri::command]
async fn search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<hermodr_core::SearchResult>, String> {
    state.service()?.search(&query).await.map_err(|e| e.to_string())
}

/// Pins or unpins a chat, mirroring it to the account.
#[tauri::command]
async fn set_pinned(state: State<'_, AppState>, chat: String, pinned: bool) -> Result<(), String> {
    state
        .service()?
        .set_pinned(&chat, pinned)
        .await
        .map_err(|e| e.to_string())
}

/// Deletes downloaded media, keeping the messages.
#[tauri::command(async)]
fn flush_media(state: State<'_, AppState>) -> Result<usize, String> {
    state.service()?.flush_media().map_err(|e| e.to_string())
}

/// Deletes every message stored on this device; the phone keeps its copy.
#[tauri::command(async)]
fn clear_history(state: State<'_, AppState>) -> Result<usize, String> {
    state.service()?.clear_history().map_err(|e| e.to_string())
}

/// The chat a stored message id belongs to.
#[tauri::command(async)]
fn chat_for_message(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    state
        .service()?
        .chat_for_message(&id)
        .map_err(|e| e.to_string())
}

/// Downloads a message's media on demand.
#[tauri::command]
async fn download_media(
    state: State<'_, AppState>,
    chat: String,
    id: String,
) -> Result<(), String> {
    state
        .service()?
        .download_media(&chat, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Sets a chat's auto download override.
#[tauri::command(async)]
fn set_chat_auto_download(
    state: State<'_, AppState>,
    chat: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .service()?
        .set_chat_auto_download(&chat, enabled)
        .map_err(|e| e.to_string())
}

/// Asks the phone for older messages in a chat.
#[tauri::command]
async fn load_older(
    state: State<'_, AppState>,
    chat: String,
    count: Option<i32>,
) -> Result<(), String> {
    state
        .service()?
        .load_older(&chat, count.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

/// The signed-in account's own JID, once connected. Also recorded on the
/// account, so the switcher can show every account's picture.
#[tauri::command]
fn own_jid(app: AppHandle, state: State<'_, AppState>) -> Option<String> {
    let jid = state.service().ok()?.own_jid();
    if jid.is_empty() {
        return None;
    }
    let active = active_account(&state);
    let mut file = state.accounts.lock().unwrap();
    if let Some(account) = file.accounts.iter_mut().find(|a| Some(&a.id) == active.as_ref()) {
        if account.jid.as_deref() != Some(jid.as_str()) {
            account.jid = Some(jid.clone());
            save_accounts(&app, &file);
        }
    }
    Some(jid)
}

#[tauri::command]
async fn send_typing(state: State<'_, AppState>, chat: String, typing: bool) -> Result<(), String> {
    state.service()?.send_typing(&chat, typing).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_online(state: State<'_, AppState>, online: bool) -> Result<(), String> {
    state.service()?.set_online(online).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn watch_presence(state: State<'_, AppState>, jid: String) -> Result<(), String> {
    state.service()?.watch_presence(&jid).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn profile(state: State<'_, AppState>) -> Result<hermodr_core::Profile, String> {
    state.service()?.profile().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_member_label(
    state: State<'_, AppState>,
    chat: String,
    label: String,
) -> Result<(), String> {
    state
        .service()?
        .set_member_label(&chat, label.trim())
        .await
        .map_err(|e| e.to_string())
}

/// Sets our profile picture from base64 image bytes; an empty string removes it.
#[tauri::command]
async fn set_profile_picture(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    state.service()?.set_own_picture(bytes).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_about(state: State<'_, AppState>, text: String) -> Result<(), String> {
    state.service()?.set_about(&text).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_push_name(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.service()?.set_push_name(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_privacy(
    state: State<'_, AppState>,
    category: String,
    value: String,
) -> Result<(), String> {
    state
        .service()?
        .set_privacy(&category, &value)
        .await
        .map_err(|e| e.to_string())
}

/// A chat's cached profile picture path, if it has one.
#[tauri::command]
async fn avatar(state: State<'_, AppState>, jid: String) -> Result<Option<String>, String> {
    state.service()?.avatar(&jid).await.map_err(|e| e.to_string())
}

/// Someone's profile card: names, number, username, about.
#[tauri::command]
async fn user_profile(state: State<'_, AppState>, jid: String) -> Result<hermodr_core::UserProfile, String> {
    state.service()?.user_profile(&jid).await.map_err(|e| e.to_string())
}

/// The group behind an invite link, without joining it.
#[tauri::command]
async fn invite_info(state: State<'_, AppState>, link: String) -> Result<hermodr_core::InviteInfo, String> {
    state.service()?.invite_info(&link).await.map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct Joined {
    jid: String,
    /// An admin still has to approve the request.
    pending: bool,
}

#[tauri::command]
async fn join_invite(state: State<'_, AppState>, link: String) -> Result<Joined, String> {
    let (jid, pending) = state.service()?.join_invite(&link).await.map_err(|e| e.to_string())?;
    Ok(Joined { jid, pending })
}

/// Marks a view-once message opened and deletes its file.
#[tauri::command(async)]
fn open_view_once(state: State<'_, AppState>, chat: String, id: String) -> Result<(), String> {
    state.service()?.open_view_once(&chat, &id).map_err(|e| e.to_string())
}

/// Best known names for JIDs, keyed by the JID as given.
#[tauri::command]
async fn names(state: State<'_, AppState>, jids: Vec<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    Ok(state.service()?.names_for(&jids).await)
}

/// Unread messages that mention us, oldest first.
#[tauri::command(async)]
fn unread_mentions(state: State<'_, AppState>, chat: String) -> Result<Vec<String>, String> {
    state
        .service()?
        .unread_mentions(&chat)
        .map_err(|e| e.to_string())
}

/// Renders a pairing code as SVG for the UI to display.
#[tauri::command]
fn qr_svg(value: String) -> Result<String, String> {
    hermodr_core::qr_svg(&value).map_err(|e| e.to_string())
}

/// Current settings.
#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> UiSettings {
    state.settings.lock().unwrap().clone()
}

/// Updates and saves settings. Service settings take effect on the next connection.
#[tauri::command]
fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: UiSettings) -> Result<(), String> {
    let path = settings_path(&app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

/// Copies log output to stderr and to the log file. The file is unbuffered so
/// a line written before an abort is on disk.
struct Tee(std::fs::File);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Where [`init_logging`] writes.
fn log_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("hermodr.log")
}

/// Sends logs and panics to `<app data>/hermodr.log` as well as stderr, which
/// is discarded when the app is launched from a desktop entry. Past 5 MB the
/// file moves to `hermodr.log.old`, so the run before a crash is still there.
fn init_logging(path: &std::path::Path) {
    // Our crates and the UI (`ui`) log at info, debug in dev builds; history
    // sync and peer requests fail silently otherwise. RUST_LOG overrides.
    let ours = if cfg!(debug_assertions) { "debug" } else { "info" };
    let filter = format!(
        "warn,hermodr_lib={ours},hermodr_core={ours},ui={ours},\
         whatsapp_rust::history_sync=info,whatsapp_rust::pdo=info"
    );
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter));
    builder.format_timestamp_millis();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 5 << 20) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    let opened = file.is_ok();
    if let Ok(file) = file {
        if let Ok(panics) = file.try_clone() {
            let default = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let thread = std::thread::current();
                let _ = writeln!(
                    &panics,
                    "[{} ms] panic in thread '{}': {info}\n{}",
                    now_millis(),
                    thread.name().unwrap_or("<unnamed>"),
                    std::backtrace::Backtrace::force_capture(),
                );
                default(info);
            }));
        }
        builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
    }
    builder.init();
    log::info!(
        "Hermóðr {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if opened {
        log::info!("logging to {}", path.display());
    } else {
        log::warn!("could not open {}; logging to stderr only", path.display());
    }
}

/// Writes a line from the UI into the log under the `ui` target.
#[tauri::command(async)]
fn frontend_log(level: String, message: String) {
    let level = match level.as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "debug" => log::Level::Debug,
        _ => log::Level::Info,
    };
    log::log!(target: "ui", level, "{message}");
}

/// Opens the log file with the desktop's default application.
#[tauri::command(async)]
fn open_log(app: AppHandle) -> Result<(), String> {
    shell_open(log_path(&app).as_os_str())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK's DMA-BUF renderer fails to create GBM buffers under Wayland
    // (Hyprland), aborting with "Gdk Error 71". This affects our own UI webview
    // as much as it did the old one.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .setup(|app| {
            init_logging(&log_path(app.handle()));
            let accounts = load_accounts(app.handle());
            migrate_media(app.handle(), &accounts);

            app.manage(AppState {
                service: Mutex::new(None),
                settings: Mutex::new(load_settings(app.handle())),
                accounts: Mutex::new(accounts),
            });

            // Built here rather than from the config so clipboard access can be
            // turned on. WebKitGTK only hands pasted images to the page when
            // `javascript_can_access_clipboard` is set, and it does not deliver
            // them through the paste event's clipboardData.
            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Hermóðr")
                .inner_size(1000.0, 720.0)
                .min_inner_size(480.0, 360.0)
                .enable_clipboard_access();
            // WebView2 only delivers dropped files to the page's drop handler
            // when Tauri's own drag and drop handler is off.
            #[cfg(target_os = "windows")]
            let builder = builder.disable_drag_drop_handler();
            builder.build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection_state,
            connect,
            accounts,
            add_account,
            switch_account,
            remove_account,
            rename_account,
            messages,
            chats,
            resolve_names,
            mark_read,
            send_reply,
            send_media,
            send_text,
            open_path,
            read_file,
            participants,
            group_info,
            group_kinds,
            set_pinned,
            unread_mentions,
            avatar,
            names,
            send_voice,
            open_view_once,
            mark_played,
            starred_messages,
            pings,
            search_messages,
            edit_event,
            set_chat_retention,
            chat_settings,
            admin_reports,
            set_allow_admin_reports,
            save_sticker,
            user_profile,
            invite_info,
            join_invite,
            message_info,
            own_jid,
            send_typing,
            set_online,
            watch_presence,
            profile,
            set_about,
            set_profile_picture,
            set_member_label,
            react,
            star,
            pin_message,
            delete_message,
            report_message,
            forward_message,
            marks,
            send_sticker,
            media_library,
            send_from_library,
            create_poll,
            vote_poll,
            create_event,
            respond_event,
            set_push_name,
            set_privacy,
            load_older,
            flush_media,
            clear_history,
            frontend_log,
            open_log,
            download_media,
            set_chat_auto_download,
            chat_for_message,
            search,
            open_url,
            qr_svg,
            get_settings,
            set_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
