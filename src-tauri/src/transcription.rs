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
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

type RequestKey = (String, String, String);
pub(crate) struct TranscriptionState {
    path: PathBuf,
    config: Mutex<Configuration>,
    active: Mutex<BTreeMap<RequestKey, watch::Sender<bool>>>,
    error: Option<String>,
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
    plugins: Vec<PluginInfo>,
    cloud_consents: Vec<ProviderConsent>,
    key_configured: bool,
    data_directory: Option<String>,
    errors: Vec<String>,
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
}

impl TranscriptionState {
    pub(crate) fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        use std::io::Read;
        let path = app.path().app_config_dir()?.join("transcription.json");
        let loaded = (|| -> Result<Configuration, Box<dyn std::error::Error>> {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)?
                .take(65537)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 65536 {
                return Err("transcription settings exceed size limit".into());
            }
            Ok(serde_json::from_slice(&bytes)?)
        })();
        let (config, error) = match loaded {
            Ok(config) => (config, None),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                (Configuration::default(), None)
            }
            Err(error) => (
                Configuration::default(),
                Some(format!("cannot load transcription settings: {error}")),
            ),
        };
        Ok(Self {
            path,
            config: Mutex::new(config),
            active: Mutex::default(),
            error,
        })
    }

    fn save(&self, config: &Configuration) -> Result<(), String> {
        use std::io::Write;
        let parent = self.path.parent().ok_or("configuration path unavailable")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, config).map_err(|e| e.to_string())?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, &self.path).map_err(|e| e.to_string())
    }

    pub(crate) fn cancel_all(&self) {
        for cancel in self.active.lock().unwrap().values() {
            let _ = cancel.send(true);
        }
    }
}

fn provider(
    state: &AppState,
    plugin_id: &str,
    provider: &str,
) -> Result<(PluginInfo, TranscriptionProvider), String> {
    let plugin = state
        .plugins
        .host
        .as_ref()
        .ok_or("plugin host unavailable")?
        .list()
        .into_iter()
        .find(|p| p.manifest.id == plugin_id && p.manifest.capabilities == ["transcribe"])
        .ok_or("transcription plugin unavailable")?;
    let selected = plugin
        .manifest
        .contributes
        .transcription
        .as_ref()
        .and_then(|c| c.providers.iter().find(|p| p.id == provider))
        .cloned()
        .ok_or("provider not declared by plugin")?;
    Ok((plugin, selected))
}

fn bound_service(state: &AppState, account_id: &str) -> Result<Arc<WhatsAppService>, String> {
    if state.accounts.lock().unwrap().active.as_deref() != Some(account_id) {
        return Err("account changed".into());
    }
    let service = state.service()?;
    let matches = state
        .account_service
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|(id, weak)| {
            id == account_id && weak.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &service))
        });
    if !matches {
        return Err("account changed".into());
    }
    Ok(service)
}

fn current(
    state: &AppState,
    account_id: &str,
    service: &Arc<WhatsAppService>,
) -> Result<(), String> {
    if !Arc::ptr_eq(service, &bound_service(state, account_id)?) {
        return Err("account changed".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn transcription_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TranscriptionView, String> {
    let stt = app.state::<TranscriptionState>();
    let config = stt.config.lock().unwrap().clone();
    let plugins = state.plugins.host.as_ref().map_or_else(Vec::new, |h| {
        h.list()
            .into_iter()
            .filter(|p| p.manifest.capabilities == ["transcribe"])
            .collect()
    });
    let mut errors: Vec<_> = stt.error.iter().cloned().collect();
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
                    credentials::get(&id, &p).map(|k| k.is_some())
                })
                .await
                .map_err(|e| e.to_string())?
                {
                    Ok(value) => key_configured = value,
                    Err(error) => errors.push(error),
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
    })
}

#[tauri::command]
pub(crate) fn set_transcription_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: TranscriptionSettings,
) -> Result<(), String> {
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
) -> Result<(), String> {
    let (_, selected) = provider(&state, &plugin_id, &provider_id)?;
    if !selected.transmits_audio {
        return Err("local provider needs no cloud consent".into());
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
) -> Result<(), String> {
    let (_, selected) = provider(&state, &plugin_id, &provider_id)?;
    if !selected.requires_key {
        return Err("provider does not use an API key".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) {
        "postal-transcription-key.exe"
    } else {
        "postal-transcription-key"
    };
    let sibling = executable
        .parent()
        .ok_or("application directory unavailable")?
        .join(name);
    let helper = if sibling.is_file() {
        sibling
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join(name)
    };
    if !helper.is_file() {
        return Err("native credential helper missing from installation".into());
    }
    tokio::task::spawn_blocking(move || -> Result<(),String> {
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
                    Ok(status) if status.success()=>return if credentials::get(&plugin_id,&provider_id)?.is_some(){Ok(())}else{Err("API key was not saved".into())},
                    _=>return Err("credential entry cancelled or failed".into()),
                }
            }
            return Err("no supported terminal found; run postal-transcription-key PLUGIN PROVIDER in a terminal".into());
        }
        #[cfg(any(windows,target_os="macos"))] {
        command.arg(&plugin_id).arg(&provider_id);
        if !command.status().map_err(|_|"cannot open native credential prompt".to_string())?.success(){return Err("credential entry cancelled or failed".into());}
        if credentials::get(&plugin_id,&provider_id)?.is_none(){return Err("API key was not saved".into());}
        Ok(())
        }
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub(crate) async fn forget_transcription_key(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    provider_id: String,
) -> Result<(), String> {
    provider(&state, &plugin_id, &provider_id)?;
    app.state::<TranscriptionState>().cancel_all();
    tokio::task::spawn_blocking(move || credentials::delete(&plugin_id, &provider_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn install_transcription_model(
    state: State<'_, AppState>,
    plugin_id: String,
    url: String,
    sha256: String,
    filename: String,
) -> Result<(), String> {
    state
        .plugins
        .host
        .as_ref()
        .ok_or("plugin host unavailable")?
        .install_model(&plugin_id, url, sha256, filename)
        .await
        .map_err(|e| e.to_string())
}

struct Active<'a> {
    active: &'a Mutex<BTreeMap<RequestKey, watch::Sender<bool>>>,
    key: RequestKey,
}
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.active.lock().unwrap().remove(&self.key);
    }
}

fn notify(
    app: &AppHandle,
    key: &RequestKey,
    status: &str,
    transcript: Option<StoredTranscript>,
    error: Option<String>,
) {
    let _ = app.emit(
        "transcription-event",
        TranscriptionEvent {
            account_id: key.0.clone(),
            chat: key.1.clone(),
            id: key.2.clone(),
            status: status.into(),
            transcript,
            error,
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
) -> Result<(String, TranscriptionRequest), String> {
    let stt = app.state::<TranscriptionState>();
    let config = stt.config.lock().unwrap().clone();
    let plugin_id = config
        .settings
        .plugin_id
        .as_ref()
        .ok_or("no transcription plugin selected")?
        .clone();
    let (plugin, selected) = provider(state, &plugin_id, &config.settings.provider)?;
    if !plugin.enabled {
        return Err("transcription plugin disabled".into());
    }
    let global_auto = state.settings.lock().unwrap().auto_transcribe;
    if automatic
        && !effective_auto(
            global_auto,
            service
                .chat_auto_transcribe(chat)
                .await
                .map_err(|e| e.to_string())?,
        )
    {
        return Err("automatic transcription disabled".into());
    }
    let cloud_consent = config
        .cloud_consents
        .contains(&(plugin_id.clone(), selected.id.clone()));
    if selected.transmits_audio && !cloud_consent {
        return Err("explicit cloud provider consent required".into());
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
        .map_err(|e| e.to_string())?;
    current(state, account_id, service)?;
    let api_key = if selected.requires_key {
        let plugin = plugin_id.clone();
        let p = selected.id.clone();
        Some(
            tokio::task::spawn_blocking(move || credentials::get(&plugin, &p))
                .await
                .map_err(|e| e.to_string())??
                .ok_or("cloud provider key required")?,
        )
    } else {
        None
    };
    let data_directory = state
        .plugins
        .host
        .as_ref()
        .ok_or("plugin host unavailable")?
        .transcription_data_directory(&plugin_id)
        .map_err(|e| e.to_string())?;
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
    request.validate().map_err(|e| e.to_string())?;
    Ok((plugin_id, request))
}

async fn run(
    app: &AppHandle,
    account_id: String,
    chat: String,
    id: String,
    force: bool,
    automatic: bool,
) -> Result<StoredTranscript, String> {
    if account_id.is_empty()
        || account_id.len() > 200
        || chat.is_empty()
        || chat.len() > 300
        || id.is_empty()
        || id.len() > 300
    {
        return Err("invalid transcription target".into());
    }
    let state = app.state::<AppState>();
    let service = bound_service(&state, &account_id)?;
    if !force {
        if let Some(cached) = service
            .message_transcript(&chat, &id)
            .await
            .map_err(|e| e.to_string())?
        {
            current(&state, &account_id, &service)?;
            return Ok(cached);
        }
    }
    let stt = app.state::<TranscriptionState>();
    let key = (account_id.clone(), chat.clone(), id.clone());
    let (cancel, mut cancelled) = watch::channel(false);
    {
        let mut active = stt.active.lock().unwrap();
        if active.contains_key(&key) {
            return Err("transcription already running".into());
        }
        if active.len() >= 64 {
            return Err("transcription queue full".into());
        }
        active.insert(key.clone(), cancel);
    }
    let _active = Active {
        active: &stt.active,
        key: key.clone(),
    };
    let (plugin_id, request) =
        prepare(app, &state, &service, &account_id, &chat, &id, automatic).await?;
    if *cancelled.borrow() {
        return Err("transcription cancelled".into());
    }
    let canonical = request.chat.clone();
    notify(app, &key, "started", None, None);
    let result = tokio::select! {
        _=cancelled.changed()=>Err("transcription cancelled".into()),
        result=state.plugins.host.as_ref().ok_or("plugin host unavailable")?.transcribe(&plugin_id,request)=>result.map_err(|e|e.to_string()),
    };
    let result = match result {
        Ok(transcript) => {
            if let Err(error) = current(&state, &account_id, &service) {
                notify(app, &key, "cancelled", None, Some(error.clone()));
                return Err(error);
            }
            let transcript = StoredTranscript {
                chat: canonical,
                id: id.clone(),
                text: transcript.text,
                language: transcript.language,
                provider: transcript.provider,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64),
            };
            service
                .save_transcript(transcript.clone())
                .await
                .map(|_| transcript)
                .map_err(|e| e.to_string())
        }
        Err(error) => Err(error),
    };
    match &result {
        Ok(t) => notify(app, &key, "completed", Some(t.clone()), None),
        Err(error) => notify(
            app,
            &key,
            if *cancelled.borrow() {
                "cancelled"
            } else {
                "failed"
            },
            None,
            Some(error.clone()),
        ),
    };
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
) -> Result<StoredTranscript, String> {
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
) -> Result<Option<StoredTranscript>, String> {
    let service = bound_service(&state, &account_id)?;
    let result = service
        .message_transcript(&chat, &id)
        .await
        .map_err(|e| e.to_string())?;
    current(&state, &account_id, &service)?;
    Ok(result)
}

#[tauri::command]
pub(crate) async fn chat_auto_transcribe(
    state: State<'_, AppState>,
    account_id: String,
    chat: String,
) -> Result<Option<bool>, String> {
    bound_service(&state, &account_id)?
        .chat_auto_transcribe(&chat)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_chat_auto_transcribe(
    state: State<'_, AppState>,
    account_id: String,
    chat: String,
    enabled: Option<bool>,
) -> Result<(), String> {
    bound_service(&state, &account_id)?
        .set_chat_auto_transcribe(&chat, enabled)
        .await
        .map_err(|e| e.to_string())
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
}
