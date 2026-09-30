use super::*;
use crate::store::pins::PinState;
use std::collections::{BTreeMap, BTreeSet};
use whatsapp_rust::wacore::types::events::{EventHandler, EventInterest, EventKind};

type PinMap = BTreeMap<String, PinState>;

#[derive(Default)]
struct Snapshot {
    pins: PinMap,
    live: PinMap,
    invalid: bool,
    start_sequence: i64,
}

#[derive(Default)]
struct State {
    pins: PinMap,
    snapshot: Option<Snapshot>,
    sequence: i64,
    confirmed_pins: Option<BTreeSet<String>>,
}

fn update(pins: &mut PinMap, pin: PinState) -> bool {
    if pins.get(&pin.chat).is_some_and(|previous| !pin.supersedes(previous)) { return false }
    pins.insert(pin.chat.clone(), pin);
    true
}

fn confirm_change(before: &[String], after: &[String], chat: &str, pinned: bool) -> Result<()> {
    anyhow::ensure!(after.iter().any(|pin| pin == chat) == pinned,
        "{}", if pinned {
            "WhatsApp did not keep this chat pinned. Your account may have reached its pin limit; the pin list was reloaded."
        } else {
            "WhatsApp did not confirm this chat's unpin. The pin list was reloaded; try again once connected."
        });
    anyhow::ensure!(!pinned || before.iter().all(|chat| after.contains(chat)),
        "WhatsApp removed an existing chat pin while applying this change. The pin list was reloaded to match your account.");
    Ok(())
}

fn snapshot_complete(synced: bool, bootstrapped: bool, version: u64, has_pins: bool) -> bool {
    synced && bootstrapped && (version > 0 || !has_pins)
}

impl State {
    fn begin_snapshot(&mut self) {
        self.snapshot = Some(Snapshot { start_sequence: self.sequence, ..Default::default() });
    }

    fn observe(&mut self, mut pin: PinState, full: bool) -> bool {
        if full && self.snapshot.is_none() { return false }
        let Some(sequence) = self.sequence.checked_add(1) else {
            if let Some(snapshot) = &mut self.snapshot { snapshot.invalid = true; }
            log::error!("chat pin sequence exhausted");
            return false;
        };
        self.sequence = sequence;
        pin.sequence = sequence;
        if let Some(snapshot) = &mut self.snapshot {
            if full { update(&mut snapshot.pins, pin); return false }
            update(&mut snapshot.live, pin.clone());
        }
        update(&mut self.pins, pin)
    }

    fn finish(&mut self, complete: bool) -> Result<()> {
        let snapshot = self.snapshot.take().ok_or_else(|| anyhow::anyhow!("chat pin snapshot was not started"))?;
        anyhow::ensure!(complete && !snapshot.invalid, "Chat pins could not be fully synchronized. Existing pins were kept; try again once connected.");
        let mut pins = snapshot.pins;
        for previous in self.pins.values() {
            pins.entry(previous.chat.clone()).or_insert_with(|| PinState {
                pinned: false, sequence: snapshot.start_sequence, ..previous.clone()
            });
        }
        for live in snapshot.live.into_values() { update(&mut pins, live); }
        self.pins = pins;
        Ok(())
    }

    fn reconcile(&mut self, store: &MessageStore, complete: bool) -> Result<()> {
        let normalize = |pins: &PinMap| -> Result<PinMap> {
            Ok(store.canonical_pin_state(&pins.values().cloned().collect::<Vec<_>>())?.into_iter()
                .map(|pin| (pin.chat.clone(), pin)).collect())
        };
        let result = (|| {
            self.pins = normalize(&self.pins)?;
            if let Some(snapshot) = &mut self.snapshot {
                snapshot.pins = normalize(&snapshot.pins)?;
                snapshot.live = normalize(&snapshot.live)?;
            }
            self.finish(complete)
        })();
        if result.is_err() { self.snapshot = None; }
        result
    }

    fn persist(&mut self, store: &MessageStore) -> Result<Vec<String>> {
        let pins = store.canonical_pin_state(&self.pins.values().cloned().collect::<Vec<_>>())?;
        store.replace_pin_state(&pins)?;
        self.pins = pins.into_iter().map(|pin| (pin.chat.clone(), pin)).collect();
        let removed: Vec<_> = self.confirmed_pins.iter().flatten()
            .filter(|chat| !self.pins.get(*chat).is_some_and(|pin| pin.pinned)).cloned().collect();
        if !removed.is_empty() { self.confirmed_pins = None; }
        Ok(removed)
    }
}

#[derive(Clone)]
pub(super) struct Pins {
    store: StoreWorker,
    state: Arc<Mutex<State>>,
    changed: tokio::sync::watch::Sender<()>,
    flushing: Arc<tokio::sync::Mutex<()>>,
    sending: Arc<tokio::sync::Mutex<()>>,
    events: broadcast::Sender<ServiceEvent>,
}

impl Pins {
    pub(super) async fn open(store: &StoreWorker, events: broadcast::Sender<ServiceEvent>) -> Result<Self> {
        let pins = store.run(|store| store.pin_state()).await?;
        let sequence = pins.iter().map(|pin| pin.sequence).max().unwrap_or(0);
        let (changed, mut changes) = tokio::sync::watch::channel(());
        let pins = Self {
            store: store.clone(),
            state: Arc::new(Mutex::new(State { pins: pins.into_iter().map(|pin| (pin.chat.clone(), pin)).collect(), sequence, ..Default::default() })),
            changed, flushing: Arc::default(), sending: Arc::default(), events,
        };
        let writer = (pins.store.clone(), pins.flushing.clone(), pins.events.clone(), pins.state.clone());
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let _guard = writer.1.lock().await;
                let state = writer.3.clone();
                match writer.0.run(move |store| state.lock().unwrap().persist(store)).await {
                    Ok(removed) => {
                        for chat in removed { let _ = writer.2.send(ServiceEvent::ChatPinRemoved { chat }); }
                        let _ = writer.2.send(ServiceEvent::StoreChanged);
                    }
                    Err(error) => log::error!("chat pins could not be saved: {error}"),
                }
            }
        });
        Ok(pins)
    }

    pub(super) fn handler(&self, enabled: bool) -> impl EventHandler { PinHandler(self.clone(), enabled) }

    async fn flush(&self) -> Result<()> {
        let _guard = self.flushing.lock().await;
        let state = self.state.clone();
        let removed = self.store.run(move |store| state.lock().unwrap().persist(store)).await?;
        for chat in removed { let _ = self.events.send(ServiceEvent::ChatPinRemoved { chat }); }
        let _ = self.events.send(ServiceEvent::StoreChanged);
        Ok(())
    }

    async fn refresh(&self, client: &Client) -> Result<()> {
        // Finish bootstrap before capturing our snapshot's replay.
        let ready = client.resync_app_state([WAPatchName::RegularLow], whatsapp_rust::AppStateResyncMode::Incremental).await
            .map_err(|error| anyhow::anyhow!("Chat pins could not be synchronized: {error}"))?;
        anyhow::ensure!(ready.all_synced() && ready.synced.contains(&WAPatchName::RegularLow), "Chat pins are still synchronizing; try again once connected.");
        self.state.lock().unwrap().begin_snapshot();
        let result = client.resync_app_state([WAPatchName::RegularLow], whatsapp_rust::AppStateResyncMode::Snapshot).await;
        let synced = result.as_ref().is_ok_and(|report| report.all_synced() && report.synced.contains(&WAPatchName::RegularLow));
        let baseline = client.persistence_manager().backend().get_version(WAPatchName::RegularLow.as_str()).await;
        let (bootstrapped, version) = baseline.as_ref().ok().and_then(|state| state.as_ref())
            .map_or((false, 0), |state| (state.bootstrapped, state.version));
        let has_pins = self.state.lock().unwrap().pins.values().any(|pin| pin.pinned);
        let complete = snapshot_complete(synced, bootstrapped, version, has_pins);
        let state = self.state.clone();
        let finish = self.store.run(move |store| state.lock().unwrap().reconcile(store, complete)).await;
        result.map_err(|error| anyhow::anyhow!("Chat pins could not be synchronized: {error}"))?;
        baseline.map_err(|error| anyhow::anyhow!("Chat pin synchronization could not be verified: {error}"))?;
        finish?;
        self.flush().await
    }

    pub(super) async fn synchronize(&self, client: &Client) -> Result<()> {
        let _send = self.sending.lock().await;
        self.refresh(client).await
    }
}

struct PinHandler(Pins, bool);

impl EventHandler for PinHandler {
    fn handle_event(&self, event: Arc<Event>) {
        let Event::PinUpdate(pin) = event.as_ref() else { return };
        let Some(pinned) = pin.action.pinned.filter(|_| pin.timestamp.timestamp_millis() >= 0) else {
            if let Some(snapshot) = &mut self.0.state.lock().unwrap().snapshot { snapshot.invalid = true; }
            log::warn!("chat pin update rejected: missing pin state or invalid timestamp");
            return;
        };
        let full = pin.from_full_sync;
        let pin = PinState { chat: pin.jid.to_non_ad().to_string(), pinned, timestamp: pin.timestamp.timestamp_millis(), sequence: 0 };
        if self.0.state.lock().unwrap().observe(pin, full) {
            self.0.changed.send_replace(());
        }
    }

    fn interest(&self) -> EventInterest {
        if self.1 { EventInterest::of(&[EventKind::PinUpdate]) } else { EventInterest::none() }
    }
}

impl WhatsAppService {
    pub async fn set_pinned(&self, chat: &str, pinned: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let key = resolve_chat(Some(&self.client), &self.store, &jid).await;
        let jid: Jid = key.parse()?;
        let _send = self.pins.sending.lock().await;
        self.pins.state.lock().unwrap().confirmed_pins = None;
        self.pins.refresh(&self.client).await?;
        let before = self.store.run(|store| store.pinned_chats()).await?;
        if before.contains(&key) == pinned { return Ok(()) }
        let actions = self.client.chat_actions();
        let result = if pinned { actions.pin_chat(&jid).await } else { actions.unpin_chat(&jid).await };
        result.map_err(|error| anyhow::anyhow!("WhatsApp rejected the chat pin change: {error}"))?;
        self.pins.refresh(&self.client).await?;
        let after = self.store.run(|store| store.pinned_chats()).await?;
        confirm_change(&before, &after, &key, pinned)?;
        if pinned {
            self.pins.state.lock().unwrap().confirmed_pins = Some(after.into_iter().collect());
            self.pins.flush().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(chat: &str, pinned: bool, timestamp: i64) -> PinState {
        PinState { chat: chat.into(), pinned, timestamp, sequence: 0 }
    }

    #[test]
    fn chat_pins_live_order_survives_clock_skew_and_same_millisecond_changes() {
        let mut state = State::default();
        assert!(state.observe(pin("1@g.us", true, 10), false));
        assert!(state.observe(pin("1@g.us", false, 20), false));
        assert!(state.observe(pin("1@g.us", true, 20), false));
        assert!(state.pins["1@g.us"].pinned);
        assert!(state.observe(pin("1@g.us", false, 10), false));
        assert!(!state.pins["1@g.us"].pinned);
        assert_eq!(state.pins["1@g.us"].timestamp, 10);
        assert_eq!(state.pins["1@g.us"].sequence, 4);
    }

    #[test]
    fn chat_pins_empty_and_complete_snapshots_remove_missing_pins() {
        let mut state = State::default();
        state.observe(pin("1@g.us", true, 1), false);
        state.observe(pin("2@g.us", true, 2), false);
        state.begin_snapshot();
        state.observe(pin("2@g.us", true, 2), true);
        state.observe(pin("3@g.us", true, 3), true);
        state.finish(true).unwrap();
        assert!(!state.pins["1@g.us"].pinned);
        assert!(state.pins["2@g.us"].pinned && state.pins["3@g.us"].pinned);
        state.begin_snapshot();
        state.finish(true).unwrap();
        assert!(state.pins.values().all(|pin| !pin.pinned));
        assert!(!state.observe(pin("3@g.us", true, 3), true));
    }

    #[test]
    fn chat_pins_partial_failed_and_invalid_snapshots_preserve_live_updates() {
        for (complete, invalid) in [(false, false), (true, true)] {
            let mut state = State::default();
            state.observe(pin("1@g.us", true, 1), false);
            state.observe(pin("2@g.us", true, 2), false);
            state.begin_snapshot();
            state.snapshot.as_mut().unwrap().invalid = invalid;
            state.observe(pin("3@g.us", true, 3), true);
            state.observe(pin("1@g.us", false, 4), false);
            assert!(state.finish(complete).is_err());
            assert!(!state.pins["1@g.us"].pinned);
            assert!(state.pins["2@g.us"].pinned);
            assert!(!state.pins.contains_key("3@g.us"));
        }
    }

    #[test]
    fn chat_pins_delivery_order_decides_live_snapshot_conflicts() {
        let mut state = State::default();
        state.observe(pin("1@g.us", true, 1), false);
        state.begin_snapshot();
        state.observe(pin("1@g.us", true, 1), true);
        state.observe(pin("1@g.us", false, 3), false);
        state.observe(pin("2@g.us", true, 2), true);
        state.finish(true).unwrap();
        assert!(!state.pins["1@g.us"].pinned);
        assert!(state.pins["2@g.us"].pinned);
        state.begin_snapshot();
        state.observe(pin("1@g.us", false, 3), false);
        state.observe(pin("1@g.us", true, 1), true);
        state.observe(pin("3@g.us", true, 3), false);
        state.finish(true).unwrap();
        assert!(state.pins["1@g.us"].pinned);
        assert!(state.pins["3@g.us"].pinned);
    }

    #[test]
    fn chat_pins_confirmation_reports_refusal_and_eviction_without_a_fixed_limit() {
        let before = vec!["1@g.us".into(), "2@g.us".into()];
        assert!(confirm_change(&before, &before, "3@g.us", true).unwrap_err().to_string().contains("pin limit"));
        let evicted = vec!["2@g.us".into(), "3@g.us".into()];
        assert!(confirm_change(&before, &evicted, "3@g.us", true).unwrap_err().to_string().contains("removed an existing"));
        let allowed: Vec<_> = (1..=10).map(|n| format!("{n}@g.us")).collect();
        confirm_change(&before, &allowed, "10@g.us", true).unwrap();
        confirm_change(&before, &[], "1@g.us", false).unwrap();
    }

    #[test]
    fn chat_pins_ambiguous_empty_response_cannot_erase_existing_pins() {
        assert!(!snapshot_complete(true, true, 0, true));
        assert!(!snapshot_complete(true, false, 7, true));
        assert!(!snapshot_complete(false, true, 7, true));
        assert!(snapshot_complete(true, true, 7, true));
        assert!(snapshot_complete(true, true, 0, false));
    }

    #[test]
    fn chat_pins_snapshot_address_change_keeps_the_same_confirmed_pin() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.set_lid_pn("123", "598123").unwrap();
        let mut state = State::default();
        state.observe(pin("598123@s.whatsapp.net", true, 1), false);
        state.begin_snapshot();
        state.observe(pin("123@lid", true, 1), true);
        state.reconcile(&store, true).unwrap();
        assert_eq!(state.pins.len(), 1);
        assert!(state.pins["598123@s.whatsapp.net"].pinned);
    }

    #[tokio::test]
    async fn chat_pins_raw_handler_updates_quiet_chats_and_rejects_incomplete_actions() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let (events, mut incoming) = broadcast::channel(16);
        let pins = Pins::open(&store, events).await.unwrap();
        let handler = pins.handler(true);
        let event = |pinned, full| Arc::new(Event::PinUpdate(whatsapp_rust::wacore::types::events::PinUpdate::builder()
            .jid("1@g.us".parse().unwrap())
            .timestamp((std::time::UNIX_EPOCH + Duration::from_millis(1)).into())
            .action(Box::new(wa::sync_action_value::PinAction { pinned }))
            .from_full_sync(full).build()));
        handler.handle_event(event(Some(true), false));
        pins.flush().await.unwrap();
        assert!(matches!(incoming.recv().await.unwrap(), ServiceEvent::StoreChanged));
        assert!(store.chats().await.unwrap()[0].pinned);
        handler.handle_event(event(Some(false), false));
        pins.flush().await.unwrap();
        assert!(!store.chats().await.unwrap()[0].pinned);
        pins.state.lock().unwrap().begin_snapshot();
        handler.handle_event(event(None, true));
        assert!(pins.state.lock().unwrap().finish(true).is_err());
        assert!(!pins.state.lock().unwrap().pins["1@g.us"].pinned);
    }

    #[test]
    fn chat_pins_notice_requires_a_confirmed_local_attempt_and_successful_save() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut state = State::default();
        state.observe(pin("1@g.us", true, 1), false);
        state.persist(&store).unwrap();
        state.observe(pin("1@g.us", false, 2), false);
        assert!(state.persist(&store).unwrap().is_empty());
        state.observe(pin("1@g.us", true, 3), false);
        state.observe(pin("2@g.us", true, 4), false);
        state.confirmed_pins = Some(["1@g.us".into(), "2@g.us".into()].into_iter().collect());
        state.persist(&store).unwrap();
        state.observe(pin("1@g.us", false, 5), false);
        state.pins.get_mut("2@g.us").unwrap().timestamp = -1;
        assert!(state.persist(&store).is_err());
        assert!(state.confirmed_pins.is_some());
        state.pins.get_mut("2@g.us").unwrap().timestamp = 4;
        assert_eq!(state.persist(&store).unwrap(), ["1@g.us"]);
        assert!(state.confirmed_pins.is_none());
        assert!(state.persist(&store).unwrap().is_empty());
        state.confirmed_pins = None;
        state.observe(pin("2@g.us", false, 6), false);
        assert!(state.persist(&store).unwrap().is_empty());
    }
}
