//! Connection lifecycle: pairing, startup maintenance and the readiness gate.

use super::*;
use whatsapp_rust::wacore::iq::keepalive::KeepaliveSpec;
use whatsapp_rust::wacore::iq::spec::IqSpec;

/// What this device asks for when it links. `android` links as an Android
/// tablet, which is what makes WhatsApp send view-once media here; external
/// keeps the UWP companion identity, named Postal on the phone's linked
/// devices. Both are only read at pairing, so switching modes links a new
/// device; an existing link keeps what it was paired with. With full history,
/// a backfill of everything the phone has.
pub(super) fn pairing_props(
    full_history: bool,
    android: bool,
) -> whatsapp_rust::wacore::store::DevicePropsOverride {
    use wa::device_props::{AppVersion, HistorySyncConfig, PlatformType};
    let props = if android {
        whatsapp_rust::wacore::store::DevicePropsOverride::new()
            .with_os("Android")
            .with_platform_type(PlatformType::ANDROID_TABLET)
            .with_version(AppVersion {
                primary: Some(2),
                secondary: Some(26),
                tertiary: Some(32),
                quaternary: Some(84),
                ..Default::default()
            })
    } else {
        whatsapp_rust::wacore::store::DevicePropsOverride::new()
            .with_os("Postal")
            .with_platform_type(PlatformType::UWP)
    };
    if !full_history {
        return props;
    }
    props.with_require_full_sync(true).with_history_sync_config(HistorySyncConfig {
        // Older than WhatsApp itself, yet small enough that days in seconds fits an i32.
        full_sync_days_limit: Some(10_000),
        on_demand_ready: Some(true),
        complete_on_demand_ready: Some(true),
        // WhatsApp Web's own claims, which the library's default also makes.
        inline_initial_payload_in_e2_ee_msg: Some(true),
        support_bot_user_agent_chat_history: Some(true),
        support_cag_reactions_and_polls: Some(true),
        support_recent_sync_chunk_message_count_tuning: Some(true),
        support_hosted_group_msg: Some(true),
        support_biz_hosted_msg: Some(true),
        support_fbid_bot_chat_history: Some(true),
        support_message_association: Some(true),
        support_call_log_history: Some(true),
        support_group_history: Some(true),
        support_manus_history: Some(true),
        support_hatch_history: Some(true),
        ..Default::default()
    })
}

#[cfg(test)]
mod pairing_tests {
    use super::*;

    #[test]
    fn pairing_uses_android_tablet_identity_in_both_history_modes() {
        for full_history in [false, true] {
            let props = pairing_props(full_history, true);
            assert_eq!(props.os.as_deref(), Some("Android"));
            assert_eq!(props.platform_type, Some(wa::device_props::PlatformType::ANDROID_TABLET));
            let version = props.version.unwrap();
            assert_eq!((version.primary, version.secondary, version.tertiary, version.quaternary),
                (Some(2), Some(26), Some(32), Some(84)));
            assert_eq!(version.quinary, None);
            assert_eq!(props.require_full_sync, full_history.then_some(true));
            assert_eq!(props.history_sync_config.as_ref().and_then(|h| h.full_sync_days_limit),
                full_history.then_some(10_000));
        }
    }

    #[test]
    fn registration_and_reconnect_use_android_tablet_handshake_metadata() {
        use whatsapp_rust::wacore::store::Device;
        let mut device = Device::new();
        device.set_device_props(pairing_props(false, true));
        device.set_client_profile(android_tablet_profile());
        for jid in [None, Some("12345:2@s.whatsapp.net".parse().unwrap())] {
            device.pn = jid;
            let payload = device.get_client_payload();
            let ua = payload.user_agent.as_option().unwrap();
            assert_eq!(ua.platform, Some(wa::client_payload::user_agent::Platform::ANDROID));
            assert_eq!(ua.device.as_deref(), Some("Tablet"));
            assert_eq!(ua.os_version.as_deref(), Some("13"));
            assert_eq!(ua.os_build_number.as_deref(), Some("13"));
            assert!(payload.web_info.is_unset());
            assert_eq!(payload.passive, Some(false));
            assert_eq!(payload.device_pairing_data.is_set(), device.pn.is_none());
        }
    }

    #[test]
    fn pairing_keeps_the_external_identity_when_not_android() {
        let props = pairing_props(false, false);
        assert_eq!(props.os.as_deref(), Some("Postal"));
        assert_eq!(props.platform_type, Some(wa::device_props::PlatformType::UWP));
        assert_eq!(props.version, None);
        assert_eq!(props.require_full_sync, None);
    }
}

/// The Android handshake profile used on every connect while in Android mode.
fn android_tablet_profile() -> whatsapp_rust::wacore::client_profile::ClientProfile {
    let mut profile = whatsapp_rust::wacore::client_profile::ClientProfile::android("13");
    profile.device = "Tablet".into();
    profile
}

/// Builds the cache configuration for a given retention window.
///
/// `msg_secrets` are the decryption keys kept so edits, reactions and poll
/// votes can still be applied to their parent message. The library's default
/// horizon is 30 days for text and 90 for polls, which on an account with
/// hundreds of thousands of messages grows the session database into the
/// hundreds of megabytes. Since Postal only keeps messages for
/// [`DiskRetention::max_age_hours`], keeping keys far beyond that window protects
/// add-ons for messages that no longer exist. The horizon is therefore capped
/// at the message window, with a floor of an hour so edits arriving slightly
/// after their parent are never lost.
pub(super) fn cache_config_for(retention: &DiskRetention) -> CacheConfig {
    let horizon = retention
        .max_age_hours
        .value()
        .map(|hours| Duration::from_secs(u64::from(hours) * 3600))
        .unwrap_or_else(|| Duration::from_secs(30 * 86_400))
        .max(Duration::from_secs(3600));

    CacheConfig {
        msg_secret_retention: MsgSecretRetention {
            text: horizon,
            // Poll and bot secrets have no sender-side time window, but they
            // still only matter while the parent message is retained.
            poll_event: horizon,
            bot: horizon,
        },
        ..Default::default()
    }
}

/// Reclaims decryption secrets left behind by a longer retention horizon.
///
/// The library prunes `msg_secrets` by their stored `expires_at`, so rows
/// written before the horizon was shortened keep their original deadline and
/// the session database stays large. This drops secrets whose parent message is
/// already outside the retention window, and shortens the deadline on the rest
/// so they expire on our schedule.
///
/// Rows with no message timestamp, or with the "never expires" marker, are left
/// untouched: their age cannot be established, and discarding them could break
/// decryption for a message we still keep.
pub(super) fn reclaim_oversized_secrets(
    session_path: &Path,
    retention: &DiskRetention,
    store: &MessageStore,
) -> Result<usize> {
    let Some(hours) = retention.max_age_hours.value() else {
        return Ok(0);
    };
    if !session_path.exists() {
        return Ok(0);
    }

    let conn = rusqlite::Connection::open(session_path)?;
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'msg_secrets'",
        [],
        |row| row.get(0),
    )?;
    if has_table == 0 {
        return Ok(0);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let horizon = i64::from(hours) * 3600;
    let cutoff = now - horizon;

    let removed = conn.execute(
        "DELETE FROM msg_secrets WHERE message_ts > 0 AND message_ts < ?1",
        [cutoff],
    )?;
    conn.execute(
        "UPDATE msg_secrets SET expires_at = ?1
         WHERE message_ts > 0 AND expires_at > ?1",
        [now + horizon],
    )?;

    // SQLite reuses freed pages rather than returning them to the filesystem,
    // so the file stays large after a bulk delete and a later run would find
    // nothing left to remove. Compacting on the freelist rather than on this
    // run's deletions covers the profile an earlier build already pruned. This
    // runs before the bot starts, so there is no concurrent access to disturb.
    let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let last = store.meta(SESSION_VACUUM_KEY)?.unwrap_or(0);
    if should_vacuum(free_pages, pages, now - last) {
        log::info!("compacting the session database: {free_pages} of {pages} pages free");
        let started = std::time::Instant::now();
        conn.execute_batch("VACUUM")?;
        store.set_meta(SESSION_VACUUM_KEY, now)?;
        log::info!("session database compacted in {:?}", started.elapsed());
    }

    Ok(removed)
}

/// `meta` key holding when the session database was last compacted (unix seconds).
const SESSION_VACUUM_KEY: &str = "session_vacuum_at";

/// VACUUM rewrites the whole file, so it waits for at least 1000 free pages
/// making up a fifth of the file, and runs at most once a week.
pub(super) fn should_vacuum(free_pages: i64, pages: i64, since_last_secs: i64) -> bool {
    free_pages > 1_000 && free_pages * 5 >= pages && since_last_secs >= 7 * 86_400
}

/// Progress of the initial catch-up, shared between the event handler and the
/// readiness task so `InitialSyncComplete` fires only once the backlog is in.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SyncProgress {
    /// Messages the server announced for this offline drain.
    pub(super) pending: usize,
    /// Messages stored so far during the drain.
    pub(super) applied: usize,
    /// The drain reported complete (or interrupted by a dropped connection).
    pub(super) offline_done: bool,
    /// Conversations added by the initial history window.
    pub(super) history_chats: usize,
    /// When the last backlog message or history chunk was stored.
    pub(super) last_progress: Option<std::time::Instant>,
    /// Throttles `Syncing` emissions so a large drain is not one IPC per message.
    pub(super) last_emit: Option<std::time::Instant>,
}

/// How long to wait, after the last backlog event, before calling the initial
/// sync settled. Scales with how much was announced, so a large backlog has time
/// to finish flushing while a small one does not sit on the loading screen.
pub(super) fn adaptive_settle(pending: usize) -> std::time::Duration {
    let ms = (pending as u64 / 10).clamp(300, 2_000);
    std::time::Duration::from_millis(ms)
}

/// Whether the initial catch-up can be called done: the drain has ended (or
/// there was nothing to sync) and nothing new has landed for the settle window,
/// or the hard cap elapsed so a stuck sync never holds the UI forever.
pub(super) fn sync_ready(p: &SyncProgress, elapsed: std::time::Duration) -> bool {
    if elapsed >= std::time::Duration::from_secs(60) {
        return true;
    }
    let settle = adaptive_settle(p.pending);
    let quiescent = p
        .last_progress
        .map_or(elapsed >= std::time::Duration::from_secs(2), |t| t.elapsed() >= settle);
    let no_backlog = p.pending == 0 && elapsed >= std::time::Duration::from_secs(2);
    (p.offline_done || no_backlog) && quiescent
}

/// The paths and policies a session starts with, so a bug report has them.
fn log_start(config: &ServiceConfig) {
    log::info!(
        "starting: session {}, messages {}, aliases {}, media {:?}, retention {:?}, full history {}",
        config.session_path.display(),
        config.messages_path.display(),
        config.aliases_path.display(),
        config.media_dir,
        config.retention,
        config.request_full_history,
    );
}

/// Reclaims the stale decryption secrets an oversized secret store holds.
async fn reclaim_secrets(store: &StoreWorker, config: &ServiceConfig) {
    let session_path = config.session_path.clone();
    let retention = config.retention;
    match store.run(move |store| reclaim_oversized_secrets(&session_path, &retention, store)).await {
        Ok(0) => {}
        Ok(removed) => log::info!("reclaimed {removed} stale decryption secret(s)"),
        Err(e) => log::warn!("could not reclaim stale decryption secrets: {e}"),
    }
}

/// The event kinds the handler subscribes to. Unlisted kinds never reach it:
/// history ("load older" included), typing and picture changes need these.
const WATCHED_EVENTS: &[EventKind] = &[
    EventKind::Messages,
    EventKind::Disconnected,
    EventKind::LoggedOut,
    EventKind::Receipt,
    EventKind::ServerAck,
    EventKind::ContactUpdate,
    EventKind::ContactRemoved,
    EventKind::OfflineSyncPreview,
    EventKind::OfflineSyncCompleted,
    EventKind::OfflineSyncInterrupted,
    EventKind::PinUpdate,
    EventKind::ArchiveUpdate,
    EventKind::MuteUpdate,
    EventKind::MarkChatAsReadUpdate,
    EventKind::HistorySync,
    EventKind::ChatPresence,
    EventKind::PictureUpdate,
    EventKind::UndecryptableMessage,
    EventKind::IdentityChange,
    EventKind::DeviceListUpdate,
    EventKind::Presence,
    EventKind::GroupUpdate,
    EventKind::Notification,
    EventKind::MissedCall,
    EventKind::FavoriteStickerUpdate,
    EventKind::RemoveRecentStickerUpdate,
];

/// The state a session's tasks share, built once at startup.
struct SessionState {
    qr: Arc<Mutex<Option<String>>>,
    connected: Arc<AtomicBool>,
    keep_archived: Arc<AtomicBool>,
    keep_view_once: Arc<AtomicBool>,
    reconnecting: Arc<AtomicBool>,
    names_resynced: Arc<AtomicBool>,
    client_slot: Arc<std::sync::OnceLock<Arc<Client>>>,
    group_cache: Arc<Mutex<std::collections::HashMap<String, GroupInfo>>>,
    groups_cache: Arc<Mutex<Option<Vec<whatsapp_rust::GroupOverview>>>>,
    sync_progress: Arc<Mutex<SyncProgress>>,
    initial_gate_done: Arc<std::sync::atomic::AtomicBool>,
    older_waits: Arc<Mutex<OlderWaits>>,
}

impl SessionState {
    fn new(config: &ServiceConfig) -> Self {
        Self {
            qr: Arc::new(Mutex::new(None)),
            connected: Arc::new(AtomicBool::new(false)),
            // Shared with the event handler so the settings apply without a
            // reconnect.
            keep_archived: Arc::new(AtomicBool::new(config.keep_archived)),
            keep_view_once: Arc::new(AtomicBool::new(config.keep_view_once)),
            // Single-flight guard for a forced reconnect after a stall or a resume.
            reconnecting: Arc::new(AtomicBool::new(false)),
            // Whether the address book has already been replayed this run.
            names_resynced: Arc::new(AtomicBool::new(false)),
            // The client only exists once the bot is built, but the message
            // handler needs it to download media. A OnceLock bridges that.
            client_slot: Arc::new(std::sync::OnceLock::new()),
            group_cache: Arc::default(),
            groups_cache: Arc::default(),
            // Progress of the initial catch-up; shared with the readiness task
            // that decides when the UI may leave its loading screen.
            sync_progress: Arc::default(),
            // Readiness is announced once per run; a reconnect must not re-gate.
            initial_gate_done: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            older_waits: Arc::default(),
        }
    }

    /// The protocol event handler, sharing this session's state.
    fn inbound(
        &self,
        store: &StoreWorker,
        disk_retention: &Arc<DiskRetentionManager>,
        events: &broadcast::Sender<ServiceEvent>,
        config: &ServiceConfig,
    ) -> Inbound {
        Inbound {
            store: store.clone(),
            disk_retention: disk_retention.clone(),
            events: events.clone(),
            connected: self.connected.clone(),
            client_for_events: self.client_slot.clone(),
            media_dir: config.media_dir.clone(),
            group_cache: self.group_cache.clone(),
            groups_cache: self.groups_cache.clone(),
            older_waits: self.older_waits.clone(),
            // Auto-downloads run beside the event handler, so a long backlog of
            // media never holds up the messages behind it.
            downloads: Arc::new(tokio::sync::Semaphore::new(4)),
            sync_progress: self.sync_progress.clone(),
            auto_download_default: config.auto_download_media,
            keep_archived: self.keep_archived.clone(),
            keep_view_once: self.keep_view_once.clone(),
            one_time_only: config.one_time_only,
            tally: Arc::new(CompanionTally::default()),
        }
    }

    /// The client with its callbacks, wired to this session's state.
    async fn build_bot(
        &self,
        config: &ServiceConfig,
        store: &StoreWorker,
        events: &broadcast::Sender<ServiceEvent>,
        inbound: Inbound,
    ) -> Result<Bot> {
        // A one-time companion wants no history at all: chunks are
        // acknowledged and dropped, so a wake never stores them.
        let policy = if config.one_time_only {
            HistoryPolicy::reject_everything()
        } else if config.request_full_history {
            HistoryPolicy::accept_everything()
        } else {
            HistoryPolicy::default()
        };
        Ok(Bot::builder()
            .with_watched_ab_props(super::diagnostics::boolean_props())
            .with_backend(SqliteStore::new(config.session_path.to_string_lossy().as_ref()).await?)
            .with_history_sync_admission(policy)
            .with_device_props(pairing_props(config.request_full_history, config.android_pair))
            .with_cache_config(cache_config_for(&config.retention))
            .on_qr_code({
                let events = events.clone();
                let qr_state = self.qr.clone();
                move |code, _timeout| {
                    let events = events.clone();
                    let qr_state = qr_state.clone();
                    async move {
                        log::info!("pairing code issued");
                        *qr_state.lock().unwrap() = Some(code.clone());
                        let _ = events.send(ServiceEvent::QrCode { code });
                    }
                }
            })
            .on_connected({
                let events = events.clone();
                let qr = self.qr.clone();
                let connected = self.connected.clone();
                let names_resynced = self.names_resynced.clone();
                let store = store.clone();
                let session_path = config.session_path.clone();
                let sync_progress = self.sync_progress.clone();
                let initial_gate_done = self.initial_gate_done.clone();
                move |client| {
                    let events = events.clone();
                    let qr = qr.clone();
                    let connected = connected.clone();
                    let names_resynced = names_resynced.clone();
                    let store = store.clone();
                    let session_path = session_path.clone();
                    let sync_progress = sync_progress.clone();
                    let initial_gate_done = initial_gate_done.clone();
                    async move {
                        log::info!("connected");
                        connected.store(true, Ordering::SeqCst);
                        // The code is spent once paired.
                        *qr.lock().unwrap() = None;
                        let _ = events.send(ServiceEvent::Connected);
                        if !initial_gate_done.swap(true, Ordering::SeqCst) {
                            spawn_initial_gate(events.clone(), sync_progress.clone());
                        }
                        if !names_resynced.swap(true, Ordering::SeqCst) {
                            spawn_address_book_resync(client.clone(), events.clone(), store.clone(), session_path.clone());
                        }
                        spawn_regular_low_resync(client.clone());
                    }
                }
            })
            .on_event_for(WATCHED_EVENTS, move |event, _client| {
                let inbound = inbound.clone();
                async move { inbound.handle(event.as_ref()).await }
            })
            .build()
            .await?)
    }
}

/// Tells the UI once when the initial catch-up is applied, so it does not drop
/// its loading screen mid-burst. The drain completes first; the settle window
/// then covers the initial history window, which has no done event.
fn spawn_initial_gate(events: broadcast::Sender<ServiceEvent>, progress: Arc<Mutex<SyncProgress>>) {
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let elapsed = started.elapsed();
            let (ready, messages, chats) = {
                let p = progress.lock().unwrap();
                (sync_ready(&p, elapsed), p.applied, p.history_chats)
            };
            if ready {
                let _ = events.send(ServiceEvent::InitialSyncComplete { messages, chats });
                break;
            }
        }
    });
}

/// Replays the address book once per run, so saved contact names reach the
/// client even though an already-paired session has no app-state patches left
/// to deliver.
fn spawn_address_book_resync(
    client: Arc<Client>,
    events: broadcast::Sender<ServiceEvent>,
    store: StoreWorker,
    session_path: std::path::PathBuf,
) {
    tokio::spawn(async move {
        match client.resync_app_state_collection(WAPatchName::CriticalUnblockLow).await {
            Ok(_) => {
                store.run(move |store| { backfill_lid_names(&session_path, store); Ok(()) }).await.logged();
                if let Some(count) = store.saved_name_count().await.observed() {
                    log::info!("address book: {count} saved name(s)");
                    if count > 0 {
                        let _ = events.send(ServiceEvent::NamesUpdated { count });
                    }
                }
            }
            Err(e) => {
                log::warn!("contact resync failed: {e}");
            }
        }
    });
}

/// Pins, archive, mute and read marks all live in regular_low. Resync it on
/// every connection so the app adopts the account's state after a reconnect or
/// a conflict, the phone being the authority.
fn spawn_regular_low_resync(client: Arc<Client>) {
    tokio::spawn(async move {
        match client.resync_app_state_collection(WAPatchName::RegularLow).await {
            Ok(report) if report.all_synced() => {
                log::debug!("regular_low resync: all collections synced");
            }
            Ok(report) => {
                let stale: Vec<_> = report.unsynced().map(|n| n.as_str()).collect();
                log::warn!("regular_low resync left collections unsynced: {stale:?}");
            }
            Err(e) => log::warn!("regular_low resync failed: {e}"),
        }
    });
}

/// How often the link is sampled: wall clock, frame counters and silence.
const WATCHDOG_TICK: Duration = Duration::from_secs(30);
/// A wall-clock gap this large across one tick means the machine slept.
const RESUME_GAP: Duration = Duration::from_secs(120);
/// No transport frames for this long while "connected" means the link is
/// likely half-open: NAT mappings expire, the peer stops answering, and
/// nothing raises an error. The library's keepalive should catch it, but it
/// was observed silent after an automatic reconnect, so Postal probes too.
const LINK_SILENT: Duration = Duration::from_secs(180);
/// A probe that does not answer within this is a dead link.
const LINK_PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// A reconnect that has not finished within this is abandoned and retried on
/// the next check, instead of pinning the single-flight flag forever.
const RECONNECT_ABANDON: Duration = Duration::from_secs(60);

/// What one watchdog tick decided.
#[derive(Debug, PartialEq, Eq)]
enum LinkAction {
    None,
    /// The machine slept, or the link is stale enough to redo it.
    Reconnect,
    /// Nothing is moving; ask the server a question that must be answered.
    Probe,
}

/// Decide from one tick's observations. Pure, so the rules are testable.
fn link_action(connected: bool, wall_gap: Duration, frames_moved: bool, silent_for: Duration) -> LinkAction {
    if !connected {
        return LinkAction::None;
    }
    if wall_gap > RESUME_GAP {
        return LinkAction::Reconnect;
    }
    if frames_moved || silent_for < LINK_SILENT {
        return LinkAction::None;
    }
    LinkAction::Probe
}

/// Keeps the link honest: a wall-clock jump after a suspend forces a
/// reconnect, and a link that has moved no transport frames for minutes is
/// probed, so a half-open socket cannot leave the app silently offline.
fn spawn_resume_watchdog(client: &Arc<Client>, reconnecting: &Arc<AtomicBool>) {
    let client = client.clone();
    let reconnecting = reconnecting.clone();
    let shutdown = client.shutdown_signal();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(WATCHDOG_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_wall = std::time::SystemTime::now();
        let mut last_frames = client.stats().frames_received;
        let mut last_movement = std::time::Instant::now();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let now_wall = std::time::SystemTime::now();
                    let gap = now_wall.duration_since(last_wall).unwrap_or_default();
                    last_wall = now_wall;
                    let frames = client.stats().frames_received;
                    let moved = frames != last_frames;
                    if moved {
                        last_frames = frames;
                        last_movement = std::time::Instant::now();
                    }
                    match link_action(client.is_connected(), gap, moved, last_movement.elapsed()) {
                        LinkAction::None => {}
                        LinkAction::Reconnect => {
                            log::warn!(
                                "resumed after {:.1} min; forcing reconnect",
                                gap.as_secs_f64() / 60.0
                            );
                            last_movement = std::time::Instant::now();
                            force_reconnect(&client, &reconnecting);
                        }
                        LinkAction::Probe => {
                            log::warn!(
                                "link silent for {:.1} min; probing it",
                                last_movement.elapsed().as_secs_f64() / 60.0
                            );
                            let iq = KeepaliveSpec::with_timeout(LINK_PROBE_TIMEOUT).build_iq();
                            let outcome = client.send_iq(iq).await;
                            last_frames = client.stats().frames_received;
                            last_movement = std::time::Instant::now();
                            match outcome {
                                Ok(_) => log::info!("link probe answered"),
                                Err(e) => {
                                    log::warn!("link probe failed ({e}); forcing reconnect");
                                    force_reconnect(&client, &reconnecting);
                                }
                            }
                        }
                    }
                }
                _ = whatsapp_rust::wacore::runtime::wait_for_shutdown(&shutdown) => {
                    log::debug!("resume watchdog stopping");
                    break;
                }
            }
        }
    });
}


impl WhatsAppService {
    /// Connects an account, pairing first if it has no session yet.
    ///
    /// Returns the service along with an event receiver that was registered
    /// before the connection attempt began. The pairing code is emitted during
    /// startup, so a receiver created afterwards would miss it; the returned one
    /// is guaranteed to see every event from the beginning.
    pub async fn start(config: ServiceConfig) -> Result<(Self, broadcast::Receiver<ServiceEvent>)> {
        log_start(&config);
        let store = StoreWorker::open(&config.messages_path).await?;
        let disk_retention = Arc::new(DiskRetentionManager::new(config.retention));
        let aliases = AliasWorker::open(&config.aliases_path).await?;
        let (events, initial_rx) = broadcast::channel(256);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        reclaim_secrets(&store, &config).await;

        let states = SessionState::new(&config);
        let inbound = states.inbound(&store, &disk_retention, &events, &config);
        let bot = states.build_bot(&config, &store, &events, inbound).await?;
        let client = bot.client();
        // In-memory only, so it is set on every start. Android metadata is what
        // makes the server treat this companion as trusted and hand over
        // view-once media instead of a bare stub; external mode keeps the
        // default web identity and gets stubs.
        if config.android_pair {
            client.set_client_profile(android_tablet_profile()).await;
        }
        // Hand the client to the message handler, which needs it to download
        // media. Without this the slot stays empty and every attachment is
        // recorded with no file.
        states.client_slot.set(client.clone()).map_err(|_| anyhow::anyhow!("event client already initialized"))?;
        spawn_resume_watchdog(&client, &states.reconnecting);

        // `run()` only returns on logout or shutdown, so it lives in its own task.
        tokio::spawn(async move {
            tokio::select! {
                _ = bot.run() => {}
                _ = shutdown_rx => {}
            }
        });

        Ok((
            Self {
                client,
                store,
                disk_retention,
                aliases,
                events,
                shutdown: Mutex::new(Some(shutdown_tx)),
                media_dir: config.media_dir,
                qr: states.qr,
                connected: states.connected,
                keep_archived: states.keep_archived,
                reconnecting: states.reconnecting,
                subject_backoff: Mutex::default(),
                nameless: Mutex::default(),
                user_info_slots: tokio::sync::Semaphore::new(user_info::MAX_REQUESTS),
                resolving: AtomicBool::new(false),
                group_cache: states.group_cache,
                groups_cache: states.groups_cache,
                older_waits: states.older_waits,
            },
            initial_rx,
        ))
    }

    /// Subscribes to service events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    /// The current pairing code, if the account still needs pairing.
    ///
    /// Exposed separately from the event stream because the code is issued
    /// during startup, before a UI subscriber may have attached.
    pub fn current_qr(&self) -> Option<String> {
        self.qr.lock().unwrap().clone()
    }

    /// Whether the account is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Whether archived chats stay archived when a new message arrives.
    pub fn keep_archived(&self) -> bool {
        self.keep_archived.load(Ordering::SeqCst)
    }

    /// Changes the keep-archived behavior without a reconnect.
    pub fn set_keep_archived(&self, keep: bool) {
        self.keep_archived.store(keep, Ordering::SeqCst);
    }

    /// Drops the current transport so the client reconnects. For a stalled or
    /// post-sleep link; safe to call repeatedly and from anywhere.
    pub fn force_reconnect(&self) {
        force_reconnect(&self.client, &self.reconnecting);
    }

    /// Reacts to an operation error: a dead link is dropped so the client
    /// reconnects, instead of leaving every later call to time out.
    pub fn note_error(&self, error: &impl std::fmt::Display) {
        let text = error.to_string();
        let dead = text.contains("timed out")
            || text.contains("not connected")
            || text.contains("Socket")
            || text.contains("disconnected");
        if dead {
            log::warn!("operation failed on a dead link ({text}); forcing reconnect");
            self.force_reconnect();
        }
    }

    /// Stops the service and closes its connection without unlinking it: the
    /// phone keeps this device linked, and the session file stays usable for
    /// the next start. Used to hot-swap between linked sessions.
    pub async fn shutdown_and_disconnect(&self) {
        self.client.disconnect().await;
        if let Some(tx) = self.shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    /// Stops the background task.
    /// Unlinks this device from the account on WhatsApp's side.
    pub async fn logout(&self) {
        self.client.logout().await;
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

/// Forces one reconnect, single-flight: a stall can be seen by several callers
/// at once, and a resume watchdog tick lands while a previous drop is settling.
/// A reconnect that does not settle within [`RECONNECT_ABANDON`] is abandoned
/// and the flag cleared, so the next check retries instead of waiting forever.
fn force_reconnect(client: &Arc<Client>, reconnecting: &Arc<AtomicBool>) {
    if reconnecting.swap(true, Ordering::SeqCst) {
        return;
    }
    let client = client.clone();
    let reconnecting = reconnecting.clone();
    tokio::spawn(async move {
        if tokio::time::timeout(RECONNECT_ABANDON, client.reconnect_immediately()).await.is_err() {
            log::warn!(
                "reconnect did not finish within {}s; abandoning it for a retry",
                RECONNECT_ABANDON.as_secs()
            );
        }
        reconnecting.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn link_actions_cover_sleep_staleness_and_life() {
        let gap = RESUME_GAP + Duration::from_secs(1);
        let silent = LINK_SILENT + Duration::from_secs(1);
        assert_eq!(link_action(false, gap, false, silent), LinkAction::None);
        assert_eq!(link_action(true, gap, true, Duration::ZERO), LinkAction::Reconnect);
        assert_eq!(link_action(true, Duration::ZERO, true, silent), LinkAction::None);
        assert_eq!(link_action(true, Duration::ZERO, false, Duration::ZERO), LinkAction::None);
        assert_eq!(link_action(true, Duration::ZERO, false, silent), LinkAction::Probe);
    }
}
