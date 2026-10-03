use crate::{
    transcription_config::{effective_auto, Configuration, TranscriptionSettings},
    transcription_credentials as credentials, AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use postal_core::{store::transcription::StoredTranscript, WhatsAppService};
use postal_plugins::{
    PluginInfo, TranscriptionConfig, TranscriptionProvider, TranscriptionRequest,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;
use crate::command_error::{CommandError, CommandResult};
use crate::plugins::{compatibility, failure, PluginRuntimeView};
use postal_core::message_ref::{MessageFailure, MessageRef};

type RequestKey = (String, String, String);
const TRANSCRIPTION_CONCURRENCY: usize = 2;
const MAX_AUTO_WAITING: usize = 24;
const MAX_WAITING: usize = 32;

pub(crate) struct TranscriptionState {
    path: PathBuf,
    config: Mutex<Configuration>,
    active: Mutex<BTreeMap<RequestKey, watch::Sender<bool>>>,
    gate: Arc<PriorityGate>,
    error: Option<String>,
    failure: Option<MessageFailure>,
}

#[derive(Default)]
struct GateState {
    running: usize,
    next_ticket: u64,
    manual: VecDeque<u64>,
    automatic: VecDeque<u64>,
}

struct PriorityGate {
    state: Mutex<GateState>,
    changed: watch::Sender<u64>,
}

struct GatePermit(Arc<PriorityGate>);

struct GateWaiter {
    gate: Arc<PriorityGate>,
    ticket: u64,
    automatic: bool,
    queued: bool,
}

#[derive(Clone, Copy, Debug)]
enum GateError {
    Full,
    Cancelled,
}

impl Default for PriorityGate {
    fn default() -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            state: Mutex::default(),
            changed,
        }
    }
}

impl PriorityGate {
    async fn acquire(
        self: &Arc<Self>,
        automatic: bool,
        cancelled: &mut watch::Receiver<bool>,
        on_queued: impl FnOnce(),
    ) -> Result<GatePermit, GateError> {
        let mut changed = self.changed.subscribe();
        if *cancelled.borrow() {
            return Err(GateError::Cancelled);
        }

        let ticket = {
            let mut state = self.state.lock().unwrap();
            let waiters = state.manual.len() + state.automatic.len();
            if waiters >= MAX_WAITING || (automatic && state.automatic.len() >= MAX_AUTO_WAITING) {
                return Err(GateError::Full);
            }
            let can_start = state.running < TRANSCRIPTION_CONCURRENCY
                && state.manual.is_empty()
                && (!automatic || state.automatic.is_empty());
            if can_start {
                state.running += 1;
                return Ok(GatePermit(self.clone()));
            }
            let ticket = state.next_ticket;
            state.next_ticket = state.next_ticket.wrapping_add(1);
            if automatic {
                state.automatic.push_back(ticket);
            } else {
                state.manual.push_back(ticket);
            }
            ticket
        };
        let mut waiter = GateWaiter { gate: self.clone(), ticket, automatic, queued: true };
        self.wake();
        on_queued();

        loop {
            if *cancelled.borrow() {
                return Err(GateError::Cancelled);
            }
            let acquired = {
                let mut state = self.state.lock().unwrap();
                let is_next = if let Some(manual) = state.manual.front() {
                    !automatic && *manual == ticket
                } else {
                    automatic && state.automatic.front() == Some(&ticket)
                };
                if state.running < TRANSCRIPTION_CONCURRENCY && is_next {
                    if automatic {
                        state.automatic.pop_front();
                    } else {
                        state.manual.pop_front();
                    }
                    state.running += 1;
                    true
                } else {
                    false
                }
            };
            if acquired {
                waiter.queued = false;
                self.wake();
                return Ok(GatePermit(self.clone()));
            }
            tokio::select! {
                _ = changed.changed() => {},
                result = cancelled.changed() => {
                    if result.is_err() || *cancelled.borrow() {
                        return Err(GateError::Cancelled);
                    }
                }
            }
        }
    }

    fn remove(&self, ticket: u64, automatic: bool) {
        let mut state = self.state.lock().unwrap();
        let queue = if automatic {
            &mut state.automatic
        } else {
            &mut state.manual
        };
        queue.retain(|queued| *queued != ticket);
        drop(state);
        self.wake();
    }

    fn wake(&self) {
        self.changed.send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

impl Drop for GateWaiter {
    fn drop(&mut self) {
        if self.queued {
            self.gate.remove(self.ticket, self.automatic);
        }
    }
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().running -= 1;
        self.0.wake();
    }
}

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct ProviderConsent {
    pub plugin_id: String,
    pub provider: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct TranscriptionView {
    settings: TranscriptionSettings,
    plugins: Vec<PluginRuntimeView>,
    cloud_consents: Vec<ProviderConsent>,
    key_configured: bool,
    data_directory: Option<String>,
    errors: Vec<String>,
    failures: Vec<MessageFailure>,
}

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct TranscriptionEvent {
    account_id: String,
    chat: String,
    id: String,
    status: String,
    transcript: Option<StoredTranscript>,
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    error_message: Option<MessageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    diagnostic: Option<String>,
}

impl TranscriptionState {
    pub(crate) fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        use std::io::Read;
        let path = app.path().app_config_dir()?.join("transcription.json");
        let _ = remove_staging_file(&path);
        let loaded = (|| -> Result<Configuration, Box<dyn std::error::Error>> {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)?
                .take(65537)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 65536 {
                return Err(MessageRef::new("error.transcription_settings_size_limit")
                    .with_param("max_bytes", serde_json::Number::from(65536)).into());
            }
            Ok(serde_json::from_slice(&bytes)?)
        })();
        let (config, error, failure) = match loaded {
            Ok(config) => (config, None, None),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                (Configuration::default(), None, None)
            }
            Err(error) => {
                let message = error.downcast_ref::<MessageRef>().cloned()
                    .unwrap_or_else(|| MessageRef::new("error.transcription_settings_load_failed"));
                let diagnostic = format!("cannot load transcription settings: {error}");
                (Configuration::default(), Some(diagnostic.clone()), Some(MessageFailure { message, diagnostic: Some(diagnostic) }))
            },
        };
        Ok(Self {
            path,
            config: Mutex::new(config),
            active: Mutex::default(),
            gate: Arc::default(),
            error,
            failure,
        })
    }

    fn save(&self, config: &Configuration) -> CommandResult<()> {
        use std::io::Write;
        let parent = self.path.parent().ok_or_else(|| CommandError::code("error.transcription_config_path_unavailable"))?;
        std::fs::create_dir_all(parent).map_err(CommandError::from)?;
        remove_staging_file(&self.path).map_err(CommandError::from)?;
        let temporary = self.path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&temporary).map_err(CommandError::from)?;
        serde_json::to_writer(&mut file, config).map_err(CommandError::operation_failed)?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(CommandError::from)?;
        std::fs::rename(temporary, &self.path).map_err(CommandError::from)
    }

    pub(crate) fn cancel_all(&self) {
        for cancel in self.active.lock().unwrap().values() {
            let _ = cancel.send(true);
        }
    }
}

fn remove_staging_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path.with_extension("json.tmp")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn provider(
    state: &AppState,
    plugin_id: &str,
    provider: &str,
) -> CommandResult<(PluginInfo, TranscriptionProvider)> {
    let plugin = state
        .plugins
        .host
        .as_ref()
        .ok_or_else(|| CommandError::code("error.plugin_host_unavailable"))?
        .list()
        .into_iter()
        .find(|p| p.manifest.id == plugin_id && p.manifest.capabilities == ["transcribe"])
        .ok_or_else(|| CommandError::code("error.transcription_plugin_unavailable"))?;
    let selected = plugin
        .manifest
        .contributes
        .transcription
        .as_ref()
        .and_then(|c| c.providers.iter().find(|p| p.id == provider))
        .cloned()
        .ok_or_else(|| CommandError::code("error.transcription_provider_undeclared"))?;
    Ok((plugin, selected))
}

fn bound_service(state: &AppState, account_id: &str) -> CommandResult<Arc<WhatsAppService>> {
    state.service_for_account(account_id)
}

fn current(
    state: &AppState,
    account_id: &str,
    service: &Arc<WhatsAppService>,
) -> CommandResult<()> {
    if !Arc::ptr_eq(service, &bound_service(state, account_id)?) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn transcription_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<TranscriptionView> {
    let stt = app.state::<TranscriptionState>();
    let config = stt.config.lock().unwrap().clone();
    let plugins = state.plugins.host.as_ref().map_or_else(Vec::new, |h| {
        h.list()
            .into_iter()
            .filter(|p| p.manifest.capabilities == ["transcribe"])
            .map(PluginRuntimeView::from)
            .collect()
    });
    let mut errors: Vec<_> = stt.error.iter().cloned().collect();
    let mut failures: Vec<_> = stt.failure.iter().cloned().collect();
    let mut key_configured = false;
    let mut data_directory = None;
    if let Some(id) = &config.settings.plugin_id {
        if let Some(host) = &state.plugins.host {
            data_directory = host
                .transcription_data_directory(id)
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
        }
        if let Ok((_, selected)) = provider(&state, id, &config.settings.provider) {
            if selected.requires_key {
                let id = id.clone();
                let p = selected.id;
                match tokio::task::spawn_blocking(move || {
                    credentials::get_typed(&id, &p).map(|k| k.is_some())
                })
                .await
                .map_err(CommandError::operation_failed)?
                {
                    Ok(value) => key_configured = value,
                    Err(error) => {
                        let error = CommandError::from(error);
                        errors.push(compatibility(&error)); failures.push(failure(error));
                    },
                }
            }
        }
    }
    Ok(TranscriptionView {
        settings: config.settings,
        plugins,
        cloud_consents: config
            .cloud_consents
            .into_iter()
            .map(|(plugin_id, provider)| ProviderConsent {
                plugin_id,
                provider,
            })
            .collect(),
        key_configured,
        data_directory,
        errors,
        failures,
    })
}

#[tauri::command]
pub(crate) fn set_transcription_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: TranscriptionSettings,
) -> CommandResult<()> {
    settings.validate()?;
    if let Some(id) = &settings.plugin_id {
        provider(&state, id, &settings.provider)?;
    }
    let stt = app.state::<TranscriptionState>();
    let mut config = stt.config.lock().unwrap();
    let mut changed = config.clone();
    changed.settings = settings;
    stt.save(&changed)?;
    *config = changed;
    stt.cancel_all();
    let _ = app.emit("transcription-settings-changed", ());
    Ok(())
}

#[tauri::command]
pub(crate) fn grant_transcription_cloud_consent(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    provider_id: String,
    approved: bool,
) -> CommandResult<()> {
    let (_, selected) = provider(&state, &plugin_id, &provider_id)?;
    if !selected.transmits_audio {
        return Err(CommandError::code("error.transcription_local_consent_unnecessary"));
    }
    let stt = app.state::<TranscriptionState>();
    let mut config = stt.config.lock().unwrap();
    let mut changed = config.clone();
    if approved {
        changed.cloud_consents.insert((plugin_id, provider_id));
    } else {
        changed.cloud_consents.remove(&(plugin_id, provider_id));
    }
    stt.save(&changed)?;
    *config = changed;
    stt.cancel_all();
    let _ = app.emit("transcription-settings-changed", ());
    Ok(())
}

#[tauri::command]
pub(crate) async fn configure_transcription_key(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    provider_id: String,
) -> CommandResult<()> {
    let (_, selected) = provider(&state, &plugin_id, &provider_id)?;
    if !selected.requires_key {
        return Err(CommandError::code("error.transcription_key_unused"));
    }
    let executable = std::env::current_exe().map_err(CommandError::from)?;
    let name = if cfg!(windows) {
        "postal-transcription-key.exe"
    } else {
        "postal-transcription-key"
    };
    let sibling = executable
        .parent()
        .ok_or_else(|| CommandError::code("error.transcription_application_directory_unavailable"))?
        .join(name);
    let helper = if sibling.is_file() {
        sibling
    } else {
        app.path()
            .resource_dir()
            .map_err(CommandError::operation_failed)?
            .join(name)
    };
    if !helper.is_file() {
        return Err(CommandError::code("error.transcription_credential_helper_missing"));
    }
    tokio::task::spawn_blocking(move || -> CommandResult<()> {
        #[cfg(windows)] let mut command={
            use std::os::windows::process::CommandExt;
            let mut c=std::process::Command::new(&helper); c.creation_flags(0x00000010); c
        };
        #[cfg(target_os="macos")] let mut command={
            let mut c=std::process::Command::new("osascript");
            c.args(["-e","on run argv\n tell application \"Terminal\"\n set taskCommand to quoted form of item 1 of argv & \" \" & quoted form of item 2 of argv & \" \" & quoted form of item 3 of argv\n set taskWindow to do script taskCommand\n activate\n repeat while busy of taskWindow\n delay 0.2\n end repeat\n end tell\nend run"]).arg(&helper); c
        };
        #[cfg(not(any(windows,target_os="macos")))] {
            for (terminal,args) in [("x-terminal-emulator",&["-e"][..]),("gnome-terminal",&["--wait","--"][..]),("konsole",&["-e"][..]),("xterm",&["-e"][..])] {
                match std::process::Command::new(terminal).args(args).arg(&helper).arg(&plugin_id).arg(&provider_id).status() {
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue,
                    Ok(status) if status.success()=>return if credentials::get_typed(&plugin_id,&provider_id)?.is_some(){Ok(())}else{Err(CommandError::code("error.transcription_key_not_saved"))},
                    _=>return Err(CommandError::code("error.transcription_credential_entry_failed")),
                }
            }
            return Err(CommandError::code("error.transcription_terminal_unavailable"));
        }
        #[cfg(any(windows,target_os="macos"))] {
        command.arg(&plugin_id).arg(&provider_id);
        if !command.status().map_err(|error| CommandError::code("error.transcription_credential_prompt_unavailable").with_diagnostic(error))?.success(){return Err(CommandError::code("error.transcription_credential_entry_failed"));}
        if credentials::get_typed(&plugin_id,&provider_id)?.is_none(){return Err(CommandError::code("error.transcription_key_not_saved"));}
        Ok(())
        }
    }).await.map_err(CommandError::operation_failed)?
}

#[tauri::command]
pub(crate) async fn forget_transcription_key(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    provider_id: String,
) -> CommandResult<()> {
    provider(&state, &plugin_id, &provider_id)?;
    app.state::<TranscriptionState>().cancel_all();
    tokio::task::spawn_blocking(move || credentials::delete_typed(&plugin_id, &provider_id))
        .await
        .map_err(CommandError::operation_failed)?
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn install_transcription_model(
    state: State<'_, AppState>,
    plugin_id: String,
    url: String,
    sha256: String,
    filename: String,
) -> CommandResult<()> {
    state
        .plugins
        .host
        .as_ref()
        .ok_or_else(|| CommandError::code("error.plugin_host_unavailable"))?
        .install_model(&plugin_id, url, sha256, filename)
        .await
        .map_err(CommandError::from)
}

struct Active {
    app: AppHandle,
    key: RequestKey,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.app.state::<TranscriptionState>().active.lock().unwrap().remove(&self.key);
    }
}

fn notify(
    app: &AppHandle,
    key: &RequestKey,
    status: &str,
    transcript: Option<StoredTranscript>,
    error: Option<CommandError>,
) {
    let error_message = error.as_ref().map(|error| error.message.clone());
    let diagnostic = error.as_ref().and_then(|error| error.diagnostic.clone());
    let error = error.as_ref().map(compatibility);
    let _ = app.emit_to(
        "main",
        "transcription-event",
        TranscriptionEvent {
            account_id: key.0.clone(),
            chat: key.1.clone(),
            id: key.2.clone(),
            status: status.into(),
            transcript,
            error,
            error_message,
            diagnostic,
        },
    );
}

async fn prepare(
    app: &AppHandle,
    state: &AppState,
    service: &Arc<WhatsAppService>,
    account_id: &str,
    chat: &str,
    id: &str,
    automatic: bool,
) -> CommandResult<(String, TranscriptionRequest)> {
    let stt = app.state::<TranscriptionState>();
    let config = stt.config.lock().unwrap().clone();
    let plugin_id = config
        .settings
        .plugin_id
        .as_ref()
        .ok_or_else(|| CommandError::code("error.transcription_plugin_required"))?
        .clone();
    let (plugin, selected) = provider(state, &plugin_id, &config.settings.provider)?;
    if !plugin.enabled {
        return Err(CommandError::code("error.transcription_plugin_disabled"));
    }
    let global_auto = state.settings.lock().unwrap().auto_transcribe;
    let audio_download = if automatic {
        service.effective_media_auto_download(chat, "audio", service.media_auto_download())
            .await.map_err(CommandError::from)?
    } else { true };
    if automatic
        && (!audio_download || !effective_auto(
            global_auto,
            service
                .chat_auto_transcribe(chat)
                .await
                .map_err(CommandError::from)?,
        ))
    {
        return Err(CommandError::code("error.transcription_auto_disabled"));
    }
    let cloud_consent = config
        .cloud_consents
        .contains(&(plugin_id.clone(), selected.id.clone()));
    if selected.transmits_audio && !cloud_consent {
        return Err(CommandError::code("error.transcription_cloud_consent_required"));
    }
    current(state, account_id, service)?;
    let audio = service
        .transcription_audio(
            chat,
            id,
            automatic,
            postal_plugins::transcription::MAX_AUDIO_BYTES,
        )
        .await
        .map_err(CommandError::from)?;
    current(state, account_id, service)?;
    let api_key = if selected.requires_key {
        let plugin = plugin_id.clone();
        let p = selected.id.clone();
        Some(
            tokio::task::spawn_blocking(move || credentials::get_typed(&plugin, &p))
                .await
                .map_err(CommandError::operation_failed)??
                .ok_or_else(|| CommandError::code("error.transcription_cloud_key_required"))?,
        )
    } else {
        None
    };
    let data_directory = state
        .plugins
        .host
        .as_ref()
        .ok_or_else(|| CommandError::code("error.plugin_host_unavailable"))?
        .transcription_data_directory(&plugin_id)
        .map_err(CommandError::from)?;
    let mut provider_config = TranscriptionConfig::default();
    provider_config.data_directory = Some(data_directory);
    provider_config.whisper_executable = config.settings.whisper_executable;
    provider_config.decoder_executable = config.settings.decoder_executable;
    provider_config.model = config.settings.model.map(PathBuf::from);
    provider_config.model_sha256 = config.settings.model_sha256;
    provider_config.language = config.settings.language;
    provider_config.idle_timeout_secs = config.settings.idle_timeout_secs;
    provider_config.cloud_consent = cloud_consent;
    provider_config.api_key = api_key.as_ref().map(|k| k.to_string());
    let request = TranscriptionRequest {
        provider: selected.id,
        chat: audio.chat,
        message_id: id.into(),
        mime: audio.mime,
        duration_ms: audio.duration_ms,
        audio: STANDARD.encode(audio.bytes),
        config: provider_config,
    };
    request.validate().map_err(CommandError::from)?;
    Ok((plugin_id, request))
}

async fn cancellable_prepare<T>(
    cancelled: &mut watch::Receiver<bool>,
    prepare: impl std::future::Future<Output = CommandResult<T>>,
) -> CommandResult<T> {
    if *cancelled.borrow() {
        return Err(CommandError::code("error.transcription_cancelled"));
    }
    tokio::select! {
        result = prepare => result,
        _ = cancelled.changed() => Err(CommandError::code("error.transcription_cancelled")),
    }
}

async fn run(
    app: &AppHandle,
    account_id: String,
    chat: String,
    id: String,
    force: bool,
    automatic: bool,
) -> CommandResult<StoredTranscript> {
    let key = (account_id.clone(), chat.clone(), id.clone());
    let valid_target = !account_id.is_empty()
        && account_id.len() <= 200
        && !chat.is_empty()
        && chat.len() <= 300
        && !id.is_empty()
        && id.len() <= 300;
    let mut notify_terminal = false;
    let mut active_guard = None;
    let result = async {
        if !valid_target {
            return Err(CommandError::code("error.transcription_target_invalid"));
        }
        let state = app.state::<AppState>();
        let service = bound_service(&state, &account_id)?;
        if !force {
            if let Some(cached) = service
                .message_transcript(&chat, &id)
                .await
                .map_err(CommandError::from)?
            {
                current(&state, &account_id, &service)?;
                return Ok(cached);
            }
        }
        let stt = app.state::<TranscriptionState>();
        let (cancel, mut cancelled) = watch::channel(false);
        {
            let mut active = stt.active.lock().unwrap();
            if active.contains_key(&key) {
                return Err(CommandError::code("error.transcription_already_running"));
            }
            active.insert(key.clone(), cancel);
        }
        notify_terminal = true;
        active_guard = Some(Active { app: app.clone(), key: key.clone() });
        let permit = match stt.gate.acquire(automatic, &mut cancelled, || {
            notify(app, &key, "queued", None, None)
        }).await {
            Ok(permit) => permit,
            Err(GateError::Full) => {
                return Err(CommandError::new(
                    MessageRef::new("error.transcription_queue_full")
                        .with_param(
                            "max",
                            serde_json::Number::from(if automatic { MAX_AUTO_WAITING } else { MAX_WAITING }),
                        ),
                ));
            }
            Err(GateError::Cancelled) => {
                return Err(CommandError::code("error.transcription_cancelled"));
            }
        };
        if *cancelled.borrow() {
            return Err(CommandError::code("error.transcription_cancelled"));
        }
        notify(app, &key, "started", None, None);
        let result = async {
            let (plugin_id, request) = cancellable_prepare(
                &mut cancelled,
                prepare(app, &state, &service, &account_id, &chat, &id, automatic),
            ).await?;
            if *cancelled.borrow() {
                return Err(CommandError::code("error.transcription_cancelled"));
            }
            let canonical = request.chat.clone();
            let host = state
                .plugins
                .host
                .as_ref()
                .ok_or_else(|| CommandError::code("error.plugin_host_unavailable"))?;
            let transcript = tokio::select! {
                _ = cancelled.changed() => return Err(CommandError::code("error.transcription_cancelled")),
                result = host.transcribe(&plugin_id, request) => result.map_err(CommandError::from)?,
            };
            current(&state, &account_id, &service)?;
            let transcript = StoredTranscript {
                chat: canonical,
                id: id.clone(),
                text: transcript.text,
                language: transcript.language,
                provider: transcript.provider,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_secs() as i64),
            };
            service
                .save_transcript(transcript.clone())
                .await
                .map(|_| transcript)
                .map_err(CommandError::from)
        }
        .await;
        drop(permit);
        result
    }
    .await;
    if valid_target
        && notify_terminal
        && result
            .as_ref()
            .err()
            .is_none_or(|error| error.message.code != "error.transcription_already_running")
    {
        match &result {
            Ok(transcript) => notify(app, &key, "completed", Some(transcript.clone()), None),
            Err(error) => notify(
                app,
                &key,
                if matches!(
                    error.message.code.as_str(),
                    "error.transcription_cancelled" | "error.account_changed"
                ) {
                    "cancelled"
                } else {
                    "failed"
                },
                None,
                Some(error.clone()),
            ),
        }
    }
    drop(active_guard);
    result
}

#[tauri::command]
pub(crate) async fn transcribe_message(
    app: AppHandle,
    account_id: String,
    chat: String,
    id: String,
    force: bool,
    automatic: bool,
) -> CommandResult<StoredTranscript> {
    run(&app, account_id, chat, id, force, automatic).await
}

#[tauri::command]
pub(crate) fn cancel_transcription(app: AppHandle, account_id: String, chat: String, id: String) {
    if let Some(cancel) = app
        .state::<TranscriptionState>()
        .active
        .lock()
        .unwrap()
        .get(&(account_id, chat, id))
    {
        let _ = cancel.send(true);
    }
}

#[tauri::command]
pub(crate) async fn message_transcript(
    state: State<'_, AppState>,
    account_id: String,
    chat: String,
    id: String,
) -> CommandResult<Option<StoredTranscript>> {
    let service = bound_service(&state, &account_id)?;
    let result = service
        .message_transcript(&chat, &id)
        .await
        .map_err(CommandError::from)?;
    current(&state, &account_id, &service)?;
    Ok(result)
}

#[tauri::command]
pub(crate) async fn chat_auto_transcribe(
    state: State<'_, AppState>,
    account_id: String,
    chat: String,
) -> CommandResult<Option<bool>> {
    bound_service(&state, &account_id)?
        .chat_auto_transcribe(&chat)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_chat_auto_transcribe(
    state: State<'_, AppState>,
    account_id: String,
    chat: String,
    enabled: Option<bool>,
) -> CommandResult<()> {
    bound_service(&state, &account_id)?
        .set_chat_auto_transcribe(&chat, enabled)
        .await
        .map_err(CommandError::from)
}

pub(crate) fn schedule_auto(app: &AppHandle, account_id: &str, event: &postal_core::ServiceEvent) {
    let Some((chat, id)) = transcription_target(event) else { return };
    if app
        .state::<TranscriptionState>()
        .config
        .lock()
        .unwrap()
        .settings
        .plugin_id
        .is_none()
    {
        return;
    }
    let (app, account_id, chat, id) =
        (app.clone(), account_id.to_owned(), chat.clone(), id.clone());
    tauri::async_runtime::spawn(async move {
        let _ = run(&app, account_id, chat, id, false, true).await;
    });
}

fn transcription_target(event: &postal_core::ServiceEvent) -> Option<(&String, &String)> {
    Some(match event {
        postal_core::ServiceEvent::Message { message } => (&message.header.chat, &message.header.id),
        postal_core::ServiceEvent::MessageHint { chat, id, change, .. } if *change != postal_core::HintChange::Status => (chat, id),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn removes_only_its_interrupted_settings_write() {
        let dir = std::env::temp_dir().join(format!(
            "postal-transcription-staging-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("transcription.json");
        let temporary = path.with_extension("json.tmp");
        let unrelated = dir.join("other.json.tmp");
        std::fs::write(&temporary, b"interrupted").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();

        remove_staging_file(&path).unwrap();

        assert!(!temporary.exists());
        assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
        std::fs::remove_dir_all(dir).unwrap();
    }

    async fn wait_for_waiters(gate: &PriorityGate, manual: usize, automatic: usize) {
        for _ in 0..1000 {
            let state = gate.state.lock().unwrap();
            if state.manual.len() == manual && state.automatic.len() == automatic {
                return;
            }
            drop(state);
            tokio::task::yield_now().await;
        }
        panic!("transcription waiters did not reach expected counts");
    }

    #[tokio::test]
    async fn aborting_gate_acquire_removes_its_waiter() {
        let gate = Arc::new(PriorityGate::default());
        let (_cancel1, mut cancelled1) = watch::channel(false);
        let (_cancel2, mut cancelled2) = watch::channel(false);
        let first = gate.acquire(false, &mut cancelled1, || {}).await.unwrap();
        let second = gate.acquire(false, &mut cancelled2, || {}).await.unwrap();
        let queued_gate = gate.clone();
        let queued = tokio::spawn(async move {
            let (_cancel, mut cancelled) = watch::channel(false);
            queued_gate.acquire(true, &mut cancelled, || {}).await
        });
        wait_for_waiters(&gate, 0, 1).await;
        queued.abort();
        assert!(matches!(queued.await, Err(error) if error.is_cancelled()));
        assert!(gate.state.lock().unwrap().automatic.is_empty());
        drop((first, second));

        let (_cancel, mut cancelled) = watch::channel(false);
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            gate.acquire(true, &mut cancelled, || {}),
        ).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancellation_during_stalled_prepare_releases_gate_slot() {
        let gate = Arc::new(PriorityGate::default());
        let (_keep_first, mut first_cancelled) = watch::channel(false);
        let first = gate.acquire(false, &mut first_cancelled, || {}).await.unwrap();
        let (cancel, mut cancelled) = watch::channel(false);
        let (started, reached_prepare) = tokio::sync::oneshot::channel();
        let queued_gate = gate.clone();
        let task = tokio::spawn(async move {
            let permit = queued_gate.acquire(false, &mut cancelled, || {}).await.unwrap();
            let result = cancellable_prepare(&mut cancelled, async move {
                started.send(()).unwrap();
                std::future::pending::<CommandResult<()>>().await
            }).await;
            drop(permit);
            result
        });
        reached_prepare.await.unwrap();
        cancel.send(true).unwrap();
        assert_eq!(task.await.unwrap().unwrap_err().message.code, "error.transcription_cancelled");
        drop(first);

        let (_cancel, mut cancelled) = watch::channel(false);
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            gate.acquire(false, &mut cancelled, || {}),
        ).await.unwrap().unwrap();
    }

    async fn fake_plugin_call(active: &AtomicUsize, peak: &AtomicUsize) {
        let running = active.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(running, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        active.fetch_sub(1, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn auto_transcription_burst_never_exceeds_two_fake_plugin_calls() {
        let gate = Arc::new(PriorityGate::default());
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut jobs = Vec::new();
        for _ in 0..20 {
            let gate = gate.clone();
            let active = active.clone();
            let peak = peak.clone();
            jobs.push(tokio::spawn(async move {
                let (_cancel, mut cancelled) = watch::channel(false);
                let _permit = gate
                    .acquire(true, &mut cancelled, || {})
                    .await
                    .unwrap();
                fake_plugin_call(&active, &peak).await;
            }));
        }
        for job in jobs {
            job.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), TRANSCRIPTION_CONCURRENCY);
    }

    #[tokio::test]
    async fn manual_work_uses_reserved_capacity_and_precedes_auto_waiters() {
        let gate = Arc::new(PriorityGate::default());
        let (_cancel1, mut cancelled1) = watch::channel(false);
        let (_cancel2, mut cancelled2) = watch::channel(false);
        let first = gate.acquire(true, &mut cancelled1, || {}).await.unwrap();
        let second = gate.acquire(true, &mut cancelled2, || {}).await.unwrap();
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut autos = Vec::new();
        for index in 0..MAX_AUTO_WAITING {
            let gate = gate.clone();
            let started = started_tx.clone();
            autos.push(tokio::spawn(async move {
                let (_cancel, mut cancelled) = watch::channel(false);
                let _permit = gate.acquire(true, &mut cancelled, || {}).await.unwrap();
                started.send(format!("auto-{index}")).unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }));
        }
        wait_for_waiters(&gate, 0, MAX_AUTO_WAITING).await;

        let mut manuals = Vec::new();
        for index in 0..(MAX_WAITING - MAX_AUTO_WAITING) {
            let gate = gate.clone();
            let started = started_tx.clone();
            manuals.push(tokio::spawn(async move {
                let (_cancel, mut cancelled) = watch::channel(false);
                let _permit = gate.acquire(false, &mut cancelled, || {}).await.unwrap();
                started.send(format!("manual-{index}")).unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }));
        }
        wait_for_waiters(&gate, MAX_WAITING - MAX_AUTO_WAITING, MAX_AUTO_WAITING).await;
        let (_cancel, mut cancelled) = watch::channel(false);
        assert!(matches!(
            gate.acquire(false, &mut cancelled, || {}).await,
            Err(GateError::Full)
        ));
        let (_cancel, mut cancelled) = watch::channel(false);
        assert!(matches!(
            gate.acquire(true, &mut cancelled, || {}).await,
            Err(GateError::Full)
        ));

        drop(first);
        drop(second);
        for index in 0..(MAX_WAITING - MAX_AUTO_WAITING) {
            let expected = format!("manual-{index}");
            assert_eq!(started_rx.recv().await.as_deref(), Some(expected.as_str()));
        }
        for manual in manuals {
            manual.await.unwrap();
        }
        assert_eq!(started_rx.recv().await.as_deref(), Some("auto-0"));
        for job in autos {
            job.await.unwrap();
        }
    }

    #[test]
    fn auto_transcription_targets_live_rows_and_content_hints_not_receipts() {
        let mut message = postal_core::StoredMessage::default();
        message.header.chat = "synthetic@invalid".into();
        message.header.id = "note".into();
        let row = postal_core::ServiceEvent::Message { message: Box::new(message) };
        let expected = Some(("synthetic@invalid", "note"));
        assert_eq!(transcription_target(&row).map(|(chat,id)| (chat.as_str(),id.as_str())), expected);
        let mut hint = postal_core::ServiceEvent::MessageHint {
            chat: "synthetic@invalid".into(), id: "note".into(), sender: "synthetic@invalid".into(),
            from_me: false, fresh: false, change: postal_core::HintChange::Content, status: None,
        };
        assert_eq!(transcription_target(&hint).map(|(chat,id)| (chat.as_str(),id.as_str())), expected);
        if let postal_core::ServiceEvent::MessageHint { change, .. } = &mut hint { *change = postal_core::HintChange::Status; }
        assert!(transcription_target(&hint).is_none());
    }

    #[test]
    fn runtime_transcription_event_keeps_status_target_and_additive_failure_fields() {
        let event = TranscriptionEvent { account_id: "synthetic-account".into(), chat: "synthetic-chat".into(), id: "synthetic-id".into(),
            status: "failed".into(), transcript: None, error: Some("synthetic raw error".into()),
            error_message: Some(MessageRef::new("error.transcription_cancelled")), diagnostic: Some("synthetic diagnostic".into()) };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["account_id"], "synthetic-account");
        assert_eq!(value["status"], "failed");
        assert_eq!(value["error"], "synthetic raw error");
        assert_eq!(value["error_message"]["code"], "error.transcription_cancelled");
        assert_eq!(value["diagnostic"], "synthetic diagnostic");
        let clean = serde_json::to_value(TranscriptionEvent { status: "completed".into(), error: None, error_message: None, diagnostic: None, ..event }).unwrap();
        assert!(clean.get("error_message").is_none() && clean.get("diagnostic").is_none());
    }
}
