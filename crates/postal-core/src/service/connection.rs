//! Connection lifecycle: pairing, startup maintenance and the readiness gate.

use super::*;

/// What this device asks for when it links: named as Postal on the phone's
/// linked devices, and with full history a backfill of everything the phone
/// has. Only read at pairing; an existing link keeps what it was paired with.
pub(super) fn pairing_props(full_history: bool) -> whatsapp_rust::wacore::store::DevicePropsOverride {
    use wa::device_props::{HistorySyncConfig, PlatformType};
    let props = whatsapp_rust::wacore::store::DevicePropsOverride::new()
        .with_os("Postal")
        .with_platform_type(PlatformType::UWP);
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

/// The retention actually applied: keeping full history means nothing is pruned.
pub(super) fn kept_retention(retention: Retention, full_history: bool) -> Retention {
    if full_history { Retention::unlimited() } else { retention }
}

/// Builds the cache configuration for a given retention window.
///
/// `msg_secrets` are the decryption keys kept so edits, reactions and poll
/// votes can still be applied to their parent message. The library's default
/// horizon is 30 days for text and 90 for polls, which on an account with
/// hundreds of thousands of messages grows the session database into the
/// hundreds of megabytes. Since Postal only keeps messages for
/// [`Retention::max_age_hours`], keeping keys far beyond that window protects
/// add-ons for messages that no longer exist. The horizon is therefore capped
/// at the message window, with a floor of an hour so edits arriving slightly
/// after their parent are never lost.
pub(super) fn cache_config_for(retention: &Retention) -> CacheConfig {
    let horizon = retention
        .max_age_hours
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
    retention: &Retention,
    store: &MessageStore,
) -> Result<usize> {
    let Some(hours) = retention.max_age_hours else {
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

impl Service {
    /// Connects an account, pairing first if it has no session yet.
    ///
    /// Returns the service along with an event receiver that was registered
    /// before the connection attempt began. The pairing code is emitted during
    /// startup, so a receiver created afterwards would miss it; the returned one
    /// is guaranteed to see every event from the beginning.
    pub async fn start(mut config: ServiceConfig) -> Result<(Self, broadcast::Receiver<ServiceEvent>)> {
        config.retention = kept_retention(config.retention, config.accept_full_history);
        log::info!(
            "starting: session {}, messages {}, aliases {}, media {:?}, retention {:?}, full history {}",
            config.session_path.display(),
            config.messages_path.display(),
            config.aliases_path.display(),
            config.media_dir,
            config.retention,
            config.accept_full_history,
        );
        let store = Arc::new(MessageStore::open(
            &config.messages_path,
            config.retention,
        )?);
        let aliases = Arc::new(AliasStore::open(&config.aliases_path)?);
        let (events, initial_rx) = broadcast::channel(256);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        match reclaim_oversized_secrets(&config.session_path, &config.retention, &store) {
            Ok(0) => {}
            Ok(removed) => log::info!("reclaimed {removed} stale decryption secret(s)"),
            Err(e) => log::warn!("could not reclaim stale decryption secrets: {e}"),
        }

        let policy = if config.accept_full_history {
            HistoryPolicy::accept_everything()
        } else {
            HistoryPolicy::default()
        };

        let qr_state = Arc::new(Mutex::new(None));
        let connected_state = Arc::new(AtomicBool::new(false));
        // Whether a new message keeps an archived chat archived; shared with the
        // event handler so the setting applies without a reconnect.
        let keep_archived_state = Arc::new(AtomicBool::new(config.keep_archived));
        // Whether the address book has already been replayed this run.
        let names_resynced = Arc::new(AtomicBool::new(false));
        // The client only exists once the bot is built, but the message handler
        // needs it to download media. A OnceLock bridges that ordering.
        let client_slot: Arc<std::sync::OnceLock<Arc<Client>>> =
            Arc::new(std::sync::OnceLock::new());

        let media_dir = config.media_dir.clone();
        let group_cache: Arc<Mutex<std::collections::HashMap<String, GroupInfo>>> = Arc::default();
        let groups_cache: Arc<Mutex<Vec<whatsapp_rust::GroupOverview>>> = Arc::default();
        // Progress of the initial catch-up; shared with the readiness task that
        // decides when the UI may leave its loading screen.
        let sync_progress: Arc<Mutex<SyncProgress>> = Arc::default();
        // Readiness is announced once per run; a reconnect must not re-gate the UI.
        let initial_gate_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let older_waits: Arc<Mutex<OlderWaits>> = Arc::default();
        let inbound = Inbound {
            store: store.clone(),
            events: events.clone(),
            connected: connected_state.clone(),
            client_for_events: client_slot.clone(),
            media_dir: media_dir.clone(),
            group_cache: group_cache.clone(),
            groups_cache: groups_cache.clone(),
            older_waits: older_waits.clone(),
            // Auto-downloads run beside the event handler, so a long backlog of media
            // never holds up the messages behind it.
            downloads: Arc::new(tokio::sync::Semaphore::new(4)),
            sync_progress: sync_progress.clone(),
            auto_download_default: config.auto_download_media,
            keep_archived: keep_archived_state.clone(),
        };

        let bot = Bot::builder()
            .with_backend(SqliteStore::new(config.session_path.to_string_lossy().as_ref()).await?)
            .with_history_sync_admission(policy)
            .with_device_props(pairing_props(config.accept_full_history))
            .with_cache_config(cache_config_for(&config.retention))
            .on_qr_code({
                let events = events.clone();
                let qr_state = qr_state.clone();
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
                let qr_state = qr_state.clone();
                let connected_state = connected_state.clone();
                let names_resynced = names_resynced.clone();
                let store = store.clone();
                let session_path = config.session_path.clone();
                let sync_progress = sync_progress.clone();
                let initial_gate_done = initial_gate_done.clone();
                move |client| {
                    let events = events.clone();
                    let qr_state = qr_state.clone();
                    let connected_state = connected_state.clone();
                    let names_resynced = names_resynced.clone();
                    let store = store.clone();
                    let session_path = session_path.clone();
                    let sync_progress = sync_progress.clone();
                    let initial_gate_done = initial_gate_done.clone();
                    async move {
                        log::info!("connected");
                        connected_state.store(true, Ordering::SeqCst);
                        // The code is spent once paired.
                        *qr_state.lock().unwrap() = None;
                        let _ = events.send(ServiceEvent::Connected);

                        // Tell the UI once when the initial catch-up is applied,
                        // so it does not drop its loading screen mid-burst. The
                        // drain completes first; the settle window then covers
                        // the initial history window, which has no done event.
                        if !initial_gate_done.swap(true, Ordering::SeqCst) {
                            let progress = sync_progress.clone();
                            let events = events.clone();
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
                                        let _ = events.send(ServiceEvent::InitialSyncComplete {
                                            messages,
                                            chats,
                                        });
                                        break;
                                    }
                                }
                            });
                        }

                        // Saved contact names reach the client as app-state
                        // patches, and an already-paired session has none left
                        // to deliver. Replay the address book once per run so
                        // the names are learned.
                        if !names_resynced.swap(true, Ordering::SeqCst) {
                            let client = client.clone();
                            let events = events.clone();
                            let session_path = session_path.clone();
                            tokio::spawn(async move {
                                match client
                                    .resync_app_state_collection(WAPatchName::CriticalUnblockLow)
                                    .await
                                {
                                    Ok(_) => {
                                        backfill_lid_names(&session_path, &store);
                                        if let Ok(count) = store.saved_name_count() {
                                            log::info!("address book: {count} saved name(s)");
                                            if count > 0 {
                                                let _ =
                                                    events.send(ServiceEvent::NamesUpdated { count });
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("contact resync failed: {e}");
                                    }
                                }
                            });
                        }
                        // Pins, archive, mute and read marks all live in
                        // regular_low. Resync it on every connection so the app
                        // adopts the account's state after a reconnect or a
                        // conflict, the phone being the authority.
                        {
                            let client = client.clone();
                            tokio::spawn(async move {
                                match client
                                    .resync_app_state_collection(WAPatchName::RegularLow)
                                    .await
                                {
                                    Ok(report) if report.all_synced() => {
                                        log::debug!("regular_low resync: all collections synced");
                                    }
                                    Ok(report) => {
                                        let stale: Vec<_> =
                                            report.unsynced().map(|n| n.as_str()).collect();
                                        log::warn!(
                                            "regular_low resync left collections unsynced: {stale:?}"
                                        );
                                    }
                                    Err(e) => log::warn!("regular_low resync failed: {e}"),
                                }
                            });
                        }
                    }
                }
            })
            .on_event_for(
                &[
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
                    // Unlisted kinds never reach the handler: history ("load
                    // older" included), typing and picture changes need these.
                    EventKind::HistorySync,
                    EventKind::ChatPresence,
                    EventKind::PictureUpdate,
                    EventKind::UndecryptableMessage,
                    EventKind::IdentityChange,
                    EventKind::DeviceListUpdate,
                    EventKind::Presence,
                    EventKind::GroupUpdate,
                    EventKind::MissedCall,
                ],
                move |event, _client| {
                    let inbound = inbound.clone();
                    async move { inbound.handle(event.as_ref()).await }
                },
            )
            .build()
            .await?;

        let client = bot.client();

        // Hand the client to the message handler, which needs it to download
        // media. Without this the slot stays empty and every attachment is
        // recorded with no file.
        let _ = client_slot.set(client.clone());

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
                aliases,
                events,
                shutdown: Mutex::new(Some(shutdown_tx)),
                media_dir,
                qr: qr_state,
                connected: connected_state,
                keep_archived: keep_archived_state,
                subject_backoff: Mutex::default(),
                nameless: Mutex::default(),
                resolving: AtomicBool::new(false),
                group_cache,
                groups_cache,
                older_waits,
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
