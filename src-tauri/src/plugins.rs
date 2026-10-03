use crate::AppState;
use crate::command_error::{CommandError, CommandResult};
use postal_core::ServiceEvent;
use postal_core::message_ref::{MessageFailure, MessageRef};
use postal_plugins::{PluginHost, PluginInfo};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

pub(crate) struct Plugins {
    pub host: Option<Arc<PluginHost>>,
    directory: String,
    error: Option<String>,
    failure: Option<MessageFailure>,
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct PluginRuntimeView {
    #[serde(flatten)]
    info: PluginInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    error_message: Option<MessageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    diagnostic: Option<String>,
}

impl From<PluginInfo> for PluginRuntimeView {
    fn from(info: PluginInfo) -> Self {
        let error_message = info.error.as_ref().map(|_| MessageRef::new("error.plugin_runtime_failed"));
        let diagnostic = info.error.clone();
        Self { info, error_message, diagnostic }
    }
}

pub(crate) fn failure(error: CommandError) -> MessageFailure {
    MessageFailure { message: error.message, diagnostic: error.diagnostic }
}

pub(crate) fn compatibility(error: &CommandError) -> String {
    if let Some(diagnostic) = &error.diagnostic { return diagnostic.clone(); }
    let mut text = crate::native_locale::english_text(&error.message.code);
    for (name, value) in &error.message.params {
        let value = match serde_json::to_value(value) {
            Ok(serde_json::Value::String(value)) => value,
            Ok(serde_json::Value::Null) | Err(_) => String::new(),
            Ok(value) => value.to_string(),
        };
        text = text.replace(&format!("{{{name}}}"), &value);
    }
    text
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct PluginsView {
    plugins: Vec<PluginRuntimeView>,
    directory: String,
    errors: Vec<String>,
    failures: Vec<MessageFailure>,
}

pub(crate) fn initialize(app: &AppHandle) -> Plugins {
    let mut directory = String::new();
    let result = (|| -> CommandResult<PluginHost> {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|e| CommandError::code("error.plugin_directory_unavailable").with_diagnostic(e))?
            .join("plugins");
        directory = root.to_string_lossy().into_owned();
        let grants = app
            .path()
            .app_config_dir()
            .map_err(|e| CommandError::code("error.plugin_directory_unavailable").with_diagnostic(e))?
            .join("plugin-grants.json");
        PluginHost::discover(&root, grants).map_err(|e| CommandError::code("error.plugin_discovery_failed").with_diagnostic(e))
    })();
    match result {
        Ok(host) => {
            let failure = tauri::async_runtime::block_on(host.start_enabled())
                .err()
                .map(|e| failure(CommandError::code("error.plugin_startup_failed").with_diagnostic(e)));
            let error = failure.as_ref().and_then(|error| error.diagnostic.clone());
            if let Some(error) = &error {
                log::error!("plugin startup: {error}");
            }
            Plugins {
                host: Some(Arc::new(host)),
                directory,
                error,
                failure,
            }
        }
        Err(error) => {
            log::error!("plugin discovery: {error}");
            let failure = failure(error);
            Plugins {
                host: None,
                directory,
                error: failure.diagnostic.clone(),
                failure: Some(failure),
            }
        }
    }
}

fn public_event(event: &ServiceEvent) -> Result<Option<serde_json::Value>, serde_json::Error> {
    if matches!(event, ServiceEvent::QrCode { .. }) {
        return Ok(None);
    }
    serde_json::to_value(event).map(Some)
}

pub(crate) fn publish(plugins: &Plugins, event: &ServiceEvent) {
    let Some(host) = &plugins.host else {
        return;
    };
    if !host.has_event_readers() {
        return;
    }
    let result = public_event(event)
        .map_err(|e| e.to_string())
        .and_then(|event| {
            event.map_or(Ok(()), |event| {
                host.publish(event).map_err(|e| format!("{e:#}"))
            })
        });
    if let Err(error) = result {
        log::warn!("plugin event delivery: {error}");
    }
}

#[tauri::command]
pub(crate) fn list_plugins(state: State<'_, AppState>) -> PluginsView {
    let plugins = &state.plugins;
    let mut errors: Vec<_> = plugins.error.iter().cloned().collect();
    let mut failures: Vec<_> = plugins.failure.iter().cloned().collect();
    let items = plugins.host.as_ref().map_or_else(Vec::new, |host| {
        errors.extend_from_slice(host.discovery_errors());
        failures.extend(host.discovery_errors().iter().map(|error| MessageFailure {
            message: MessageRef::new("error.plugin_discovery_failed"), diagnostic: Some(error.clone()),
        }));
        host.list()
    });
    PluginsView {
        plugins: items.into_iter().map(PluginRuntimeView::from).collect(),
        directory: plugins.directory.clone(),
        errors,
        failures,
    }
}

#[tauri::command]
pub(crate) async fn set_plugin_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    capabilities: Vec<String>,
) -> CommandResult<()> {
    let host = state
        .plugins
        .host
        .as_ref()
        .ok_or_else(|| CommandError::code("error.plugin_host_unavailable"))?;
    host.set_enabled(&id, enabled, capabilities)
        .await
        .map_err(CommandError::from)?;
    let _ = app.emit("transcription-settings-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_secrets_never_enter_plugin_protocol() {
        assert!(public_event(&ServiceEvent::QrCode {
            code: "synthetic-pairing-secret".into()
        })
        .unwrap()
        .is_none());
        let connected = public_event(&ServiceEvent::Connected).unwrap().unwrap();
        assert_eq!(connected["kind"], "connected");
    }

    #[test]
    fn runtime_plugin_errors_add_metadata_without_changing_legacy_fields() {
        let manifest = serde_json::from_value(serde_json::json!({
            "id":"synthetic.plugin", "name":"Synthetic", "version":"1", "api_version":1,
            "entrypoint":"synthetic", "idle_timeout_secs":null, "capabilities":["transcribe"],
        })).unwrap();
        let info = PluginInfo { manifest, enabled: false, state: "failed".into(), error: Some("synthetic diagnostic".into()) };
        let value = serde_json::to_value(PluginRuntimeView::from(info.clone())).unwrap();
        assert_eq!(value["id"], "synthetic.plugin");
        assert_eq!(value["error"], "synthetic diagnostic");
        assert_eq!(value["diagnostic"], "synthetic diagnostic");
        assert_eq!(value["error_message"]["code"], "error.plugin_runtime_failed");
        assert!(value.get("info").is_none());
        let clean = serde_json::to_value(PluginRuntimeView::from(PluginInfo { error: None, ..info })).unwrap();
        assert!(clean.get("error_message").is_none() && clean.get("diagnostic").is_none());
    }

    #[test]
    fn runtime_failure_refs_keep_parameters_and_diagnostics() {
        let value = serde_json::to_value(failure(CommandError::new(MessageRef::new("error.transcription_queue_full")
            .with_param("max", serde_json::Number::from(64))).with_diagnostic("synthetic diagnostic"))).unwrap();
        assert_eq!(value["code"], "error.transcription_queue_full");
        assert_eq!(value["params"]["max"], 64);
        assert_eq!(value["diagnostic"], "synthetic diagnostic");
        assert!(value.get("message").is_none());
    }
}
