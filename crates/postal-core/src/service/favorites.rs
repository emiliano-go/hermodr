use super::*;
use crate::message_ref::MessageRef;
use anyhow::Context;
use std::path::Path;
use crate::store::favorites::FavoriteWorker;
use whatsapp_rust::{AppStateResyncMode, AppStateResyncReport};
use whatsapp_rust::wacore::appstate::hash::HashState;
use whatsapp_rust::wacore::types::events::{EventHandler, EventInterest, EventKind};

#[path = "favorites_recovery.rs"]
mod recovery;

#[derive(Clone)]
struct Snapshot { revision: u64, chats: Vec<String>, replay: Option<Replay> }

#[derive(Clone, Default)]
struct Replay { chats: Option<Vec<String>>, live: Option<Vec<String>>, invalid: bool }

fn requested_synced(report: &AppStateResyncReport) -> bool {
    report.all_synced() && report.synced.contains(&WAPatchName::RegularHigh)
}

#[derive(Clone)]
pub(super) struct Favorites {
    worker: FavoriteWorker,
    current: Arc<Mutex<Snapshot>>,
    changed: tokio::sync::watch::Sender<u64>,
    sending: Arc<tokio::sync::Mutex<()>>,
    synced: Arc<AtomicBool>,
    capture: recovery::Capture,
}

fn favorite_jid(chat: &str) -> Result<Jid> {
    let jid: Jid = chat.parse().with_context(|| MessageRef::new("error.favorite_address_invalid"))?;
    anyhow::ensure!(!jid.user.is_empty(), MessageRef::new("error.favorite_address_invalid"));
    Ok(jid)
}

fn normalize(chats: &[String]) -> Result<Vec<String>> {
    let mut found = std::collections::HashSet::new();
    chats.iter().map(|chat| {
        let jid = favorite_jid(chat)?;
        Ok(jid.to_non_ad().to_string())
    }).filter_map(|chat| match chat {
        Ok(chat) if found.insert(chat.clone()) => Some(Ok(chat)),
        Ok(_) => None,
        Err(error) => Some(Err(error)),
    }).collect()
}

impl Favorites {
    #[cfg(test)]
    pub(super) async fn open(path: &Path, events: broadcast::Sender<ServiceEvent>) -> Result<Self> {
        Self::open_with_key(path, events, None).await
    }

    pub(super) async fn open_with_key(path: &Path, events: broadcast::Sender<ServiceEvent>,
        key: Option<crate::database_crypto::DatabaseKey>) -> Result<Self> {
        let worker = FavoriteWorker::open_with_key(path, key).await?;
        let chats = worker.run(|db| db.list()).await?;
        let current = Arc::new(Mutex::new(Snapshot { revision: 0, chats, replay: None }));
        let (changed, mut changes) = tokio::sync::watch::channel(0);
        let saved = worker.clone();
        let latest = current.clone();
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let snapshot = latest.lock().unwrap().clone();
                match saved.run(move |db| db.replace(snapshot.revision, &snapshot.chats)).await {
                    Ok(_) => { let _ = events.send(ServiceEvent::FavoritesChanged); }
                    Err(error) => log::error!("favorite chats could not be saved: {error}"),
                }
            }
        });
        Ok(Self { worker, current, changed, sending: Arc::default(), synced: Arc::default(), capture: recovery::Capture::default() })
    }

    fn snapshot(&self) -> Snapshot { self.current.lock().unwrap().clone() }

    fn observe(&self, chats: &[String], expected: Option<u64>) -> Result<()> {
        let chats = normalize(chats)?;
        let mut current = self.current.lock().unwrap();
        self.apply(&mut current, chats, expected)
    }

    fn apply(&self, current: &mut Snapshot, chats: Vec<String>, expected: Option<u64>) -> Result<()> {
        if expected.is_some_and(|revision| revision != current.revision) { return Ok(()); }
        current.revision = current.revision.checked_add(1).ok_or_else(|| anyhow::Error::new(MessageRef::new("error.favorite_revision_exhausted")))?;
        current.chats = chats;
        self.changed.send_replace(current.revision);
        Ok(())
    }

    fn receive(&self, chats: &[String], full: bool) -> Result<()> {
        let chats = normalize(chats)?;
        let mut current = self.current.lock().unwrap();
        if let Some(replay) = &mut current.replay {
            if full { replay.chats = Some(chats); return Ok(()) }
            replay.live = Some(chats.clone());
        }
        self.apply(&mut current, chats, None)
    }

    fn begin_snapshot(&self) { self.current.lock().unwrap().replay = Some(Replay::default()); }

    async fn flush(&self) -> Result<()> {
        let snapshot = self.snapshot();
        self.worker.run(move |db| db.replace(snapshot.revision, &snapshot.chats)).await?;
        Ok(())
    }

    pub(super) fn handler(&self) -> impl EventHandler { FavoriteHandler(self.clone()) }

    pub(super) async fn synchronize(&self, client: &Arc<Client>) -> Result<()> {
        let _send = self.sending.lock().await;
        if self.synced.load(Ordering::Acquire) { return Ok(()); }
        self.ensure_synced(client).await
    }

    async fn ensure_synced(&self, client: &Arc<Client>) -> Result<()> {
        self.synced.store(false, Ordering::Release);
        let ready = client.resync_app_state([WAPatchName::RegularHigh], AppStateResyncMode::Incremental).await?;
        anyhow::ensure!(requested_synced(&ready), MessageRef::new("error.favorites_sync_pending"));
        self.begin_snapshot();
        let result = self.read_snapshot(client).await;
        let complete = self.complete_snapshot(result.is_ok(), result.as_ref().ok());
        result?;
        complete?;
        self.flush().await?;
        self.synced.store(true, Ordering::Release);
        Ok(())
    }

    async fn read_snapshot(&self, client: &Arc<Client>) -> Result<HashState> {
        let report = client.resync_app_state([WAPatchName::RegularHigh], AppStateResyncMode::Snapshot).await?;
        anyhow::ensure!(requested_synced(&report), MessageRef::new("error.favorites_sync_pending"));
        let baseline = recovery::baseline(client).await.with_context(|| MessageRef::new("error.favorites_recovery_unavailable"))?;
        let snapshot = self.snapshot();
        let replay = snapshot.replay.as_ref().ok_or_else(|| anyhow::Error::new(MessageRef::new("error.favorite_snapshot_missing")))?;
        anyhow::ensure!(!replay.invalid, MessageRef::new("error.favorite_update_invalid"));
        if replay.chats.is_some() { return Ok(baseline); }
        let (proof, baseline) = recovery::recover(client, &self.capture, &baseline).await
            .with_context(|| MessageRef::new("error.favorites_recovery_failed"))?;
        let mut current = self.current.lock().unwrap();
        anyhow::ensure!(current.revision == snapshot.revision, MessageRef::new("error.favorites_changed"));
        let replay = current.replay.as_mut().ok_or_else(|| anyhow::Error::new(MessageRef::new("error.favorite_snapshot_missing")))?;
        anyhow::ensure!(!replay.invalid, MessageRef::new("error.favorite_update_invalid"));
        if let Some(chats) = &replay.chats {
            anyhow::ensure!(*chats == proof.chats, MessageRef::new("error.favorites_changed"));
        }
        replay.chats = Some(proof.chats);
        Ok(baseline)
    }

    fn complete_snapshot(&self, synced: bool, baseline: Option<&HashState>) -> Result<()> {
        let mut current = self.current.lock().unwrap();
        let replay = current.replay.take().ok_or_else(|| anyhow::Error::new(MessageRef::new("error.favorite_snapshot_missing")))?;
        let verified = baseline.is_some_and(|state| state.has_baseline() && state.bootstrapped
            && !state.mac_mismatch_fatal && replay.chats.is_some());
        anyhow::ensure!(synced && verified && !replay.invalid,
            MessageRef::new("error.favorites_sync_incomplete"));
        self.apply(&mut current, replay.live.or(replay.chats).unwrap_or_default(), None)
    }
}

struct FavoriteHandler(Favorites);

impl EventHandler for FavoriteHandler {
    fn handle_event(&self, event: Arc<Event>) {
        if let Event::DecryptedPayload(raw) = event.as_ref() { self.0.capture.forward(raw); return; }
        let Event::FavoritesUpdate(update) = event.as_ref() else { return };
        let chats: Option<Vec<String>> = update.action.favorites.iter().map(|favorite| favorite.id.clone()).collect();
        if chats.as_deref().map_or(true, |chats| self.0.receive(chats, update.from_full_sync).is_err()) {
            self.0.synced.store(false, Ordering::Release);
            let mut current = self.0.current.lock().unwrap();
            if let Some(replay) = &mut current.replay { replay.invalid = true; }
            log::warn!("favorite update rejected: invalid or missing chat address");
        }
    }

    fn interest(&self) -> EventInterest {
        EventInterest::of(&[EventKind::FavoritesUpdate, EventKind::DecryptedPayload])
    }
}

fn action(chats: &[String], timestamp: i64) -> wa::SyncActionValue {
    wa::SyncActionValue {
        timestamp: Some(timestamp),
        favorites_action: MessageField::some(wa::sync_action_value::FavoritesAction {
            favorites: chats.iter().map(|chat| wa::sync_action_value::favorites_action::Favorite { id: Some(chat.clone()) }).collect(),
        }),
        ..Default::default()
    }
}

async fn resolved_chats(client: Option<&Client>, store: &StoreWorker, raw: &[String]) -> Result<Vec<String>> {
    let mut chats = Vec::new();
    let mut found = std::collections::HashSet::new();
    for raw in raw {
        let jid = favorite_jid(raw)?;
        let chat = resolve_chat(client, store, &jid).await;
        if found.insert(chat.clone()) { chats.push(chat); }
    }
    Ok(chats)
}

impl WhatsAppService {
    pub async fn favorite_chats(&self) -> Result<Vec<String>> {
        let snapshot = self.favorites.snapshot();
        resolved_chats(Some(&self.client), &self.store, &snapshot.chats).await
    }

    pub async fn set_favorite(&self, chat: &str, favorite: bool) -> Result<()> {
        let jid = favorite_jid(chat)?;
        let _send = self.favorites.sending.lock().await;
        self.favorites.ensure_synced(&self.client).await?;
        let target = resolve_chat(Some(&self.client), &self.store, &jid).await;
        let (snapshot, desired) = loop {
            let snapshot = self.favorites.snapshot();
            let mut desired = Vec::new();
            let mut found = false;
            for raw in &snapshot.chats {
                let old: Jid = raw.parse()?;
                let matches = resolve_chat(Some(&self.client), &self.store, &old).await == target;
                found |= matches;
                if !matches || favorite { desired.push(raw.clone()); }
            }
            if favorite && !found { desired.push(jid.to_non_ad().to_string()); }
            if self.favorites.snapshot().revision == snapshot.revision { break (snapshot, desired); }
        };
        if desired == snapshot.chats { return self.favorites.flush().await; }
        let timestamp = i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis())?;
        self.client.send_app_state_action(&whatsapp_rust::schemas::FAVORITES, &[], &action(&desired, timestamp)).await
            .map_err(anyhow::Error::new)?;
        self.favorites.observe(&desired, Some(snapshot.revision))?;
        self.favorites.flush().await?;
        let _ = self.events.send(ServiceEvent::FavoritesChanged);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::wacore::types::events::FavoritesUpdate;

    fn update(ids: &[Option<&str>], timestamp: i64, full: bool) -> Arc<Event> {
        Arc::new(Event::FavoritesUpdate(FavoritesUpdate::builder()
            .timestamp(whatsapp_rust::wacore::time::from_millis_or_now(timestamp))
            .action(Box::new(wa::sync_action_value::FavoritesAction {
                favorites: ids.iter().map(|id| wa::sync_action_value::favorites_action::Favorite {
                    id: id.map(str::to_owned),
                }).collect(),
            }))
            .from_full_sync(full).build()))
    }

    #[tokio::test]
    async fn favorite_aliases_reconcile_to_phone_in_original_order() {
        let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let raw = vec!["900@lid".into(), "100@g.us".into(), "200@s.whatsapp.net".into()];
        assert_eq!(resolved_chats(None, &store, &raw).await.unwrap(), raw);
        store.set_lid_pn("900", "200").await.unwrap();
        assert_eq!(resolved_chats(None, &store, &raw).await.unwrap(), vec!["200@s.whatsapp.net", "100@g.us"]);
        assert_eq!(raw, vec!["900@lid", "100@g.us", "200@s.whatsapp.net"]);
    }

    #[tokio::test]
    async fn favorite_events_replay_atomically_in_arrival_order_without_phone_clock_ordering() {
        let (events, mut notices) = broadcast::channel(8);
        let favorites = Favorites::open(Path::new(":memory:"), events).await.unwrap();
        let handler = favorites.handler();
        favorites.worker.run(|db| db.replace(1, &["200@s.whatsapp.net".into(), "100@g.us".into()])).await.unwrap();
        handler.handle_event(update(&[Some("200:7@s.whatsapp.net"), Some("100@g.us"), Some("200@s.whatsapp.net")], 1000, true));
        let first = favorites.snapshot();
        assert_eq!(first.chats, vec!["200@s.whatsapp.net", "100@g.us"]);
        let notice = tokio::time::timeout(std::time::Duration::from_secs(1), notices.recv()).await.unwrap().unwrap();
        assert!(matches!(notice, ServiceEvent::FavoritesChanged));
        favorites.synced.store(true, Ordering::Release);
        handler.handle_event(update(&[Some("new@g.us"), None], 2000, true));
        assert!(!favorites.synced.load(Ordering::Acquire));
        assert_eq!(favorites.snapshot().chats, first.chats);
        handler.handle_event(update(&[Some("s.whatsapp.net")], 2001, true));
        assert_eq!(favorites.snapshot().chats, first.chats);
        handler.handle_event(update(&[Some("100@g.us"), Some("200@s.whatsapp.net")], 0, true));
        favorites.observe(&first.chats, Some(first.revision)).unwrap();
        let expected = vec!["100@g.us".to_owned(), "200@s.whatsapp.net".to_owned()];
        assert_eq!(favorites.snapshot().chats, expected);
        favorites.flush().await.unwrap();
        assert_eq!(favorites.worker.run(|db| db.list()).await.unwrap(), expected);
        handler.handle_event(update(&[], 0, true));
        favorites.flush().await.unwrap();
        assert!(favorites.worker.run(|db| db.list()).await.unwrap().is_empty());

        let sent = action(&expected, 42);
        assert_eq!(sent.timestamp, Some(42));
        assert_eq!(sent.favorites_action.as_option().unwrap().favorites.iter()
            .map(|favorite| favorite.id.as_deref().unwrap()).collect::<Vec<_>>(), vec!["100@g.us", "200@s.whatsapp.net"]);
        assert!(action(&[], 43).favorites_action.as_option().unwrap().favorites.is_empty());
    }

    #[tokio::test]
    async fn favorite_snapshot_requires_requested_bucket_and_unambiguous_baseline() {
        let mut report = AppStateResyncReport::default();
        assert!(report.all_synced());
        assert!(!requested_synced(&report));
        report.synced.push(WAPatchName::RegularLow);
        assert!(!requested_synced(&report));
        report.synced.push(WAPatchName::RegularHigh);
        assert!(requested_synced(&report));
        report.retryable.push(WAPatchName::RegularHigh);
        assert!(!requested_synced(&report));

        let zero = HashState { bootstrapped: true, ..Default::default() };
        let trusted = HashState { version: 8, ..zero.clone() };
        let incomplete = HashState { bootstrapped: false, ..trusted.clone() };
        let mismatch = HashState { mac_mismatch_fatal: true, ..trusted.clone() };
        for old in [vec!["old@g.us".to_owned()], Vec::new()] {
            let (events, _) = broadcast::channel(8);
            let favorites = Favorites::open(Path::new(":memory:"), events).await.unwrap();
            let handler = favorites.handler();
            favorites.observe(&old, None).unwrap();
            favorites.flush().await.unwrap();

            for baseline in [&zero, &trusted] {
                favorites.begin_snapshot();
                assert!(favorites.complete_snapshot(true, Some(baseline)).is_err());
                assert_eq!(favorites.snapshot().chats, old);
                assert!(!favorites.synced.load(Ordering::Acquire));
            }

            for (synced, baseline) in [(false, Some(&trusted)), (true, None), (true, Some(&incomplete)), (true, Some(&mismatch))] {
                favorites.begin_snapshot();
                handler.handle_event(update(&[Some("new@g.us")], 0, true));
                assert_eq!(favorites.snapshot().chats, old);
                assert!(favorites.complete_snapshot(synced, baseline).is_err());
                assert_eq!(favorites.snapshot().chats, old);
                assert_eq!(favorites.worker.run(|db| db.list()).await.unwrap(), old);
                assert!(favorites.snapshot().replay.is_none());
            }

            favorites.begin_snapshot();
            handler.handle_event(update(&[Some("new@g.us")], 0, true));
            handler.handle_event(update(&[Some("bad@g.us"), None], 0, true));
            assert!(favorites.complete_snapshot(true, Some(&trusted)).is_err());
            assert_eq!(favorites.snapshot().chats, old);

            favorites.begin_snapshot();
            handler.handle_event(update(&[Some("new@g.us")], 0, true));
            handler.handle_event(update(&[Some("live@g.us")], 0, false));
            favorites.complete_snapshot(true, Some(&trusted)).unwrap();
            assert_eq!(favorites.snapshot().chats, vec!["live@g.us"]);

            favorites.begin_snapshot();
            assert!(favorites.complete_snapshot(true, Some(&trusted)).is_err());
            assert_eq!(favorites.snapshot().chats, vec!["live@g.us"]);

            favorites.observe(&old, None).unwrap();
            favorites.begin_snapshot();
            handler.handle_event(update(&[], 0, true));
            favorites.complete_snapshot(true, Some(&zero)).unwrap();
            assert!(favorites.snapshot().chats.is_empty());

            favorites.begin_snapshot();
            handler.handle_event(update(&[Some("new@g.us")], 0, true));
            favorites.complete_snapshot(true, Some(&zero)).unwrap();
            favorites.flush().await.unwrap();
            assert_eq!(favorites.worker.run(|db| db.list()).await.unwrap(), vec!["new@g.us"]);
        }
    }
}
