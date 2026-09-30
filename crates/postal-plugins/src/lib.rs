mod manifest;
mod process;
mod protocol;
pub mod transcription;

use anyhow::{ensure, Context, Result};
pub use manifest::{Activation, Manifest};
pub use transcription::{
    ProviderKind, Transcript, TranscriptionConfig, TranscriptionContribution,
    TranscriptionProvider, TranscriptionRequest,
};

#[cfg(feature = "wire-types")]
pub fn visit_wire_types(visitor: &mut impl ts_rs::TypeVisitor) {
    visitor.visit::<PluginInfo>();
    visitor.visit::<protocol::Reply>();
    visitor.visit::<protocol::HostMessage>();
}

#[cfg(feature = "wire-types")]
pub fn wire_fixture(event: Value) -> Value {
    serde_json::to_value(protocol::HostMessage::Event { seq: 7, event }).unwrap()
}
use process::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

const QUEUE_SIZE: usize = 64;
const MAX_FAILURES: u32 = 3;

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct PluginInfo {
    #[serde(flatten)]
    pub manifest: Manifest,
    pub enabled: bool,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Default)]
struct Runtime {
    state: String,
    error: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Grant {
    enabled: bool,
    capabilities: Vec<String>,
}

struct Entry {
    plugin: Arc<manifest::Plugin>,
    grant: Grant,
    runtime: Arc<Mutex<Runtime>>,
    events: Option<mpsc::Sender<Envelope>>,
    stop: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
}

struct Registry {
    entries: BTreeMap<String, Entry>,
    seq: u64,
}

struct Inner {
    registry: Mutex<Registry>,
    changes: tokio::sync::Mutex<()>,
    grants_path: PathBuf,
    errors: Vec<String>,
}

pub struct PluginHost(Arc<Inner>);

struct Envelope {
    seq: u64,
    bytes: Arc<zeroize::Zeroizing<Vec<u8>>>,
    done: Option<oneshot::Sender<std::result::Result<Value, String>>>,
    transcription: Option<(RequestKind, Duration, Option<u64>)>,
}

pub(crate) enum RequestKind {
    Transcribe(String),
    InstallModel(String),
}

fn status(runtime: &Mutex<Runtime>, state: &str, error: Option<String>) {
    let mut runtime = runtime.lock().unwrap();
    runtime.state = state.into();
    if error.is_some() {
        runtime.error = error;
    }
}

fn save_grants(inner: &Inner, registry: &Registry) -> Result<()> {
    let grants: BTreeMap<_, _> = registry
        .entries
        .iter()
        .map(|(id, entry)| (id, &entry.grant))
        .collect();
    let parent = inner
        .grants_path
        .parent()
        .context("grants path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = inner.grants_path.with_extension("json.tmp");
    use std::io::Write;
    let mut file = std::fs::File::create(&temporary)?;
    serde_json::to_writer(&mut file, &grants)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(temporary, &inner.grants_path)?;
    Ok(())
}

impl PluginHost {
    pub fn discover(directory: &Path, grants_path: PathBuf) -> Result<Self> {
        let grants: BTreeMap<String, Grant> =
            match manifest::read_bounded(&grants_path, protocol::MAX_LINE) {
                Ok(data) => serde_json::from_slice(&data).context("invalid plugin grants file")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => return Err(error.into()),
            };
        let mut entries = BTreeMap::new();
        let mut errors = Vec::new();
        if directory.exists() {
            let root = directory.canonicalize()?;
            for child in std::fs::read_dir(&root)? {
                let child = child?;
                if !child.file_type()?.is_dir() {
                    continue;
                }
                match manifest::Plugin::load(&child.path()) {
                    Ok(plugin) if plugin.directory.starts_with(&root) => {
                        let id = plugin.manifest.id.clone();
                        if entries.contains_key(&id) {
                            anyhow::bail!("duplicate plugin id: {id}");
                        }
                        let mut grant = grants.get(&id).cloned().unwrap_or_default();
                        if grant.capabilities != plugin.manifest.capabilities {
                            grant.enabled = false;
                        }
                        entries.insert(
                            id,
                            Entry {
                                plugin: Arc::new(plugin),
                                grant,
                                runtime: Arc::new(Mutex::new(Runtime {
                                    state: "disabled".into(),
                                    error: None,
                                })),
                                events: None,
                                stop: None,
                                task: None,
                            },
                        );
                    }
                    Ok(_) => errors.push("plugin directory escapes discovery root".into()),
                    Err(error) => errors.push(format!(
                        "{}: {error:#}",
                        child.file_name().to_string_lossy()
                    )),
                }
            }
        }
        Ok(Self(Arc::new(Inner {
            registry: Mutex::new(Registry { entries, seq: 0 }),
            changes: tokio::sync::Mutex::new(()),
            grants_path,
            errors,
        })))
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.0
            .registry
            .lock()
            .unwrap()
            .entries
            .values()
            .map(|entry| {
                let runtime = entry.runtime.lock().unwrap();
                PluginInfo {
                    manifest: entry.plugin.manifest.clone(),
                    enabled: entry.grant.enabled,
                    state: runtime.state.clone(),
                    error: runtime.error.clone(),
                }
            })
            .collect()
    }

    pub fn discovery_errors(&self) -> &[String] {
        &self.0.errors
    }

    pub fn has_enabled(&self) -> bool {
        self.0
            .registry
            .lock()
            .unwrap()
            .entries
            .values()
            .any(|entry| entry.grant.enabled)
    }

    pub fn has_event_readers(&self) -> bool {
        self.0
            .registry
            .lock()
            .unwrap()
            .entries
            .values()
            .any(|entry| entry.grant.enabled && entry.grant.capabilities == ["events:read"])
    }

    pub async fn start_enabled(&self) -> Result<()> {
        let enabled: Vec<_> = self
            .0
            .registry
            .lock()
            .unwrap()
            .entries
            .iter()
            .filter(|(_, entry)| entry.grant.enabled)
            .map(|(id, entry)| (id.clone(), entry.grant.capabilities.clone()))
            .collect();
        for (id, grants) in enabled {
            self.set_enabled(&id, true, grants).await?;
        }
        Ok(())
    }

    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        capabilities: Vec<String>,
    ) -> Result<()> {
        {
            let registry = self.0.registry.lock().unwrap();
            let entry = registry.entries.get(id).context("unknown plugin")?;
            ensure!(
                !enabled || capabilities == entry.plugin.manifest.capabilities,
                "explicit consent for every declared capability required"
            );
        }
        let _change = self.0.changes.lock().await;
        let task = {
            let mut registry = self.0.registry.lock().unwrap();
            let entry = registry.entries.get_mut(id).context("unknown plugin")?;
            if let Some(stop) = entry.stop.take() {
                let _ = stop.send(true);
            }
            entry.events = None;
            entry.task.take()
        };
        if let Some(task) = task {
            task.await.context("plugin supervisor failed")?;
        }
        let inner = self.0.clone();
        let id = id.to_owned();
        let persisted_id = id.clone();
        tokio::task::spawn_blocking(move || {
            let mut registry = inner.registry.lock().unwrap();
            let entry = registry
                .entries
                .get_mut(&persisted_id)
                .context("unknown plugin")?;
            entry.grant = Grant {
                enabled,
                capabilities,
            };
            if let Err(error) = save_grants(&inner, &registry) {
                let entry = registry.entries.get_mut(&persisted_id).unwrap();
                entry.grant.enabled = false;
                status(
                    &entry.runtime,
                    "disabled",
                    Some(format!("cannot save consent: {error:#}")),
                );
                return Err(error);
            }
            Ok(())
        })
        .await??;
        let mut registry = self.0.registry.lock().unwrap();
        let entry = registry.entries.get_mut(&id).unwrap();
        *entry.runtime.lock().unwrap() = Runtime {
            state: if enabled { "idle" } else { "disabled" }.into(),
            error: None,
        };
        if enabled {
            let (sender, events) = mpsc::channel(QUEUE_SIZE);
            let (stop, stopped) = watch::channel(false);
            entry.events = Some(sender);
            entry.stop = Some(stop);
            entry.task = Some(tokio::spawn(supervise(
                Arc::downgrade(&self.0),
                entry.plugin.clone(),
                entry.runtime.clone(),
                events,
                stopped,
            )));
        }
        Ok(())
    }

    /// Queues a read-only event without blocking the service event loop.
    pub fn publish(&self, event: Value) -> Result<()> {
        let mut registry = self.0.registry.lock().unwrap();
        if !registry
            .entries
            .values()
            .any(|entry| entry.grant.enabled && entry.grant.capabilities == ["events:read"])
        {
            return Ok(());
        }
        let (seq, bytes) = envelope(&mut registry, event)?;
        for entry in registry.entries.values() {
            if !entry.grant.enabled || entry.grant.capabilities != ["events:read"] {
                continue;
            }
            if let Some(sender) = &entry.events {
                if sender
                    .try_send(Envelope {
                        seq,
                        bytes: bytes.clone(),
                        done: None,
                        transcription: None,
                    })
                    .is_err()
                {
                    let error = "event queue full or stopped; event dropped";
                    entry.runtime.lock().unwrap().error = Some(error.into());
                    log::warn!("plugin {}: {error}", entry.plugin.manifest.id);
                }
            }
        }
        Ok(())
    }

    /// Ensures a plugin is active and waits for acknowledgement of this event.
    pub async fn send_event(&self, id: &str, event: Value) -> Result<()> {
        let (done, result) = oneshot::channel();
        {
            let mut registry = self.0.registry.lock().unwrap();
            let (seq, bytes) = envelope(&mut registry, event)?;
            let entry = registry.entries.get(id).context("unknown plugin")?;
            ensure!(entry.grant.enabled, "plugin disabled");
            ensure!(
                entry.grant.capabilities == ["events:read"],
                "events:read not granted"
            );
            entry
                .events
                .as_ref()
                .context("plugin stopped")?
                .try_send(Envelope {
                    seq,
                    bytes,
                    done: Some(done),
                    transcription: None,
                })
                .map_err(|_| anyhow::anyhow!("event queue full or stopped"))?;
        }
        result
            .await
            .context("plugin stopped before acknowledgement")?
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    }

    pub fn transcription_data_directory(&self, id: &str) -> Result<PathBuf> {
        let registry = self.0.registry.lock().unwrap();
        let entry = registry.entries.get(id).context("unknown plugin")?;
        ensure!(
            entry.plugin.manifest.capabilities == ["transcribe"],
            "not a transcription plugin"
        );
        let path = entry.plugin.directory.join("data");
        std::fs::create_dir_all(&path)?;
        let path = path.canonicalize()?;
        ensure!(
            path.starts_with(&entry.plugin.directory),
            "plugin data directory escapes installation"
        );
        Ok(path)
    }

    /// Activates an approved sidecar and waits for its matching transcript.
    pub async fn transcribe(
        &self,
        plugin_id: &str,
        request: TranscriptionRequest,
    ) -> Result<Transcript> {
        request.validate()?;
        let (done, result) = oneshot::channel();
        {
            let mut registry = self.0.registry.lock().unwrap();
            let entry = registry.entries.get(plugin_id).context("unknown plugin")?;
            ensure!(
                entry.grant.enabled && entry.grant.capabilities == ["transcribe"],
                "transcribe not granted"
            );
            let provider = entry
                .plugin
                .manifest
                .contributes
                .transcription
                .as_ref()
                .and_then(|c| c.providers.iter().find(|p| p.id == request.provider))
                .context("provider not declared by plugin")?;
            ensure!(
                !provider.transmits_audio || request.config.cloud_consent,
                "explicit cloud provider consent required"
            );
            ensure!(
                !provider.requires_key
                    || request
                        .config
                        .api_key
                        .as_ref()
                        .is_some_and(|k| !k.trim().is_empty()),
                "cloud provider key required"
            );
            registry.seq = registry
                .seq
                .checked_add(1)
                .context("plugin sequence exhausted")?;
            let seq = registry.seq;
            let options = (
                RequestKind::Transcribe(request.provider.clone()),
                Duration::from_secs(request.config.timeout_secs.unwrap_or(300)),
                request.config.idle_timeout_secs,
            );
            let mut bytes = serde_json::to_vec(&protocol::HostMessage::<Value>::Transcribe {
                id: seq,
                request,
            })?;
            bytes.push(b'\n');
            ensure!(
                bytes.len() <= protocol::MAX_LINE,
                "transcription request exceeds 1 MiB"
            );
            registry
                .entries
                .get(plugin_id)
                .unwrap()
                .events
                .as_ref()
                .context("plugin stopped")?
                .try_send(Envelope {
                    seq,
                    bytes: Arc::new(zeroize::Zeroizing::new(bytes)),
                    done: Some(done),
                    transcription: Some(options),
                })
                .map_err(|_| anyhow::anyhow!("plugin queue full or stopped"))?;
        }
        let value = result
            .await
            .context("plugin stopped before transcription")?
            .map_err(anyhow::Error::msg)?;
        serde_json::from_value(value).context("invalid transcript result")
    }

    pub async fn install_model(
        &self,
        plugin_id: &str,
        url: String,
        sha256: String,
        filename: String,
    ) -> Result<()> {
        let data_directory = self.transcription_data_directory(plugin_id)?;
        let request = transcription::ModelDownload {
            url,
            sha256,
            filename: filename.clone(),
            data_directory,
        };
        request.validate()?;
        let (done, result) = oneshot::channel();
        {
            let mut registry = self.0.registry.lock().unwrap();
            let entry = registry.entries.get(plugin_id).context("unknown plugin")?;
            ensure!(
                entry.grant.enabled && entry.grant.capabilities == ["transcribe"],
                "transcribe not granted"
            );
            registry.seq = registry
                .seq
                .checked_add(1)
                .context("plugin sequence exhausted")?;
            let seq = registry.seq;
            let mut bytes = serde_json::to_vec(&protocol::HostMessage::<Value>::InstallModel {
                id: seq,
                request,
            })?;
            bytes.push(b'\n');
            ensure!(
                bytes.len() <= protocol::MAX_LINE,
                "model request exceeds size limit"
            );
            registry
                .entries
                .get(plugin_id)
                .unwrap()
                .events
                .as_ref()
                .context("plugin stopped")?
                .try_send(Envelope {
                    seq,
                    bytes: Arc::new(zeroize::Zeroizing::new(bytes)),
                    done: Some(done),
                    transcription: Some((
                        RequestKind::InstallModel(filename),
                        Duration::from_secs(600),
                        None,
                    )),
                })
                .map_err(|_| anyhow::anyhow!("plugin queue full or stopped"))?;
        }
        result
            .await
            .context("plugin stopped before model installation")?
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    pub async fn shutdown(&self) {
        let _change = self.0.changes.lock().await;
        let tasks: Vec<_> = {
            let mut registry = self.0.registry.lock().unwrap();
            registry
                .entries
                .values_mut()
                .filter_map(|entry| {
                    if let Some(stop) = entry.stop.take() {
                        let _ = stop.send(true);
                    }
                    entry.events = None;
                    entry.task.take()
                })
                .collect()
        };
        for task in tasks {
            let _ = task.await;
        }
    }
}

fn envelope(
    registry: &mut Registry,
    event: Value,
) -> Result<(u64, Arc<zeroize::Zeroizing<Vec<u8>>>)> {
    registry.seq = registry
        .seq
        .checked_add(1)
        .context("plugin sequence exhausted")?;
    let seq = registry.seq;
    let mut bytes = serde_json::to_vec(&protocol::HostMessage::Event { seq, event })?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= protocol::MAX_LINE,
        "plugin event exceeds 1 MiB"
    );
    Ok((seq, Arc::new(zeroize::Zeroizing::new(bytes))))
}

async fn supervise(
    inner: Weak<Inner>,
    plugin: Arc<manifest::Plugin>,
    runtime: Arc<Mutex<Runtime>>,
    mut events: mpsc::Receiver<Envelope>,
    mut stop: watch::Receiver<bool>,
) {
    let lazy = plugin.manifest.activation == Activation::Lazy;
    let mut failures = 0;
    loop {
        if *stop.borrow() {
            break;
        }
        let pending = if lazy {
            status(&runtime, "idle", None);
            tokio::select! {
                biased;
                _ = stop.changed() => break,
                event = events.recv() => match event { Some(event) => Some(event), None => break },
            }
        } else {
            None
        };
        if pending.as_ref().is_some_and(|e| {
            e.transcription.is_some() && e.done.as_ref().is_some_and(|d| d.is_closed())
        }) {
            continue;
        }
        status(&runtime, "starting", None);
        let mut session = match Session::spawn(&plugin) {
            Ok(session) => session,
            Err(error) => {
                reject(pending, &format!("{error:#}"));
                if failure(&inner, &plugin, &runtime, &mut failures, &mut stop, &error).await {
                    break;
                }
                continue;
            }
        };
        let result = tokio::select! {
            biased;
            _ = stop.changed() => { reject(pending, "plugin stopping"); session.shutdown().await; break; }
            result = tokio::time::timeout(Duration::from_secs(5), session.handshake()) => result.context("plugin handshake timed out").and_then(|r| r),
        };
        if let Err(error) = result {
            reject(pending, &format!("{error:#}"));
            session.shutdown().await;
            if failure(&inner, &plugin, &runtime, &mut failures, &mut stop, &error).await {
                break;
            }
            continue;
        }
        status(&runtime, "running", None);
        let result = tokio::select! {
            biased;
            _ = stop.changed() => Ok(()),
            result = run_session(&mut session, pending, &mut events, &plugin) => result,
        };
        session.shutdown().await;
        if *stop.borrow() || stop.has_changed().is_err() {
            break;
        }
        if let Err(error) = result {
            if failure(&inner, &plugin, &runtime, &mut failures, &mut stop, &error).await {
                break;
            }
        }
    }
    status(&runtime, "stopped", None);
}

fn reject(event: Option<Envelope>, reason: &str) {
    if let Some(Envelope {
        done: Some(done), ..
    }) = event
    {
        let _ = done.send(Err(reason.into()));
    }
}

async fn run_session(
    session: &mut Session,
    mut pending: Option<Envelope>,
    events: &mut mpsc::Receiver<Envelope>,
    plugin: &manifest::Plugin,
) -> Result<()> {
    let mut linger = Duration::from_secs(plugin.manifest.idle_timeout_secs.unwrap_or(0));
    let mut idle_at = tokio::time::Instant::now() + linger;
    loop {
        let mut event = if let Some(event) = pending.take() {
            event
        } else {
            let idle = async {
                if plugin.manifest.activation == Activation::Lazy {
                    tokio::time::sleep_until(idle_at).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                biased;
                event = events.recv() => match event { Some(event) => event, None => return Ok(()) },
                _ = idle => return Ok(()),
                reply = session.next() => { reply?; continue; }
            }
        };
        let timeout = event
            .transcription
            .as_ref()
            .map_or(Duration::from_secs(10), |(_, t, _)| *t);
        if let Some((_, _, idle)) = &event.transcription {
            linger = Duration::from_secs(idle.unwrap_or(0));
        }
        let cancelled = async {
            if event.transcription.is_some() {
                if let Some(done) = &mut event.done {
                    done.closed().await;
                }
            } else {
                std::future::pending::<()>().await;
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancelled => { session.cancel(event.seq).await?; return Ok(()); }
            result = tokio::time::timeout(timeout, session.deliver(event.seq, &event.bytes, event.transcription.as_ref().map(|o| &o.0))) =>
                result.context("plugin request timed out").and_then(|r| r),
        };
        if let Some(done) = event.done {
            let _ = done.send(result.as_ref().cloned().map_err(|e| format!("{e:#}")));
        }
        result?;
        idle_at = tokio::time::Instant::now() + linger;
    }
}

async fn failure(
    inner: &Weak<Inner>,
    plugin: &manifest::Plugin,
    runtime: &Mutex<Runtime>,
    failures: &mut u32,
    stop: &mut watch::Receiver<bool>,
    error: &anyhow::Error,
) -> bool {
    let error = format!("{error:#}");
    log::warn!("plugin {}: {error}", plugin.manifest.id);
    status(runtime, "failed", Some(error));
    if plugin.manifest.activation == Activation::Lazy {
        return false;
    }
    *failures += 1;
    if *failures >= MAX_FAILURES {
        if let Some(inner) = inner.upgrade() {
            let id = plugin.manifest.id.clone();
            let saved = tokio::task::spawn_blocking(move || {
                let mut registry = inner.registry.lock().unwrap();
                registry.entries.get_mut(&id).unwrap().grant.enabled = false;
                save_grants(&inner, &registry)
            })
            .await;
            if !matches!(saved, Ok(Ok(()))) {
                status(
                    runtime,
                    "failed",
                    Some(format!(
                        "plugin disabled; could not persist disabled state: {saved:?}"
                    )),
                );
            }
        }
        return true;
    }
    status(runtime, "backoff", None);
    tokio::select! {
        _ = stop.changed() => true,
        _ = tokio::time::sleep(Duration::from_millis(250 * (1 << (*failures - 1)))) => false,
    }
}
