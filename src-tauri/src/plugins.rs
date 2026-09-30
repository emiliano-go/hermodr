use crate::AppState;
use postal_core::ServiceEvent;
use postal_plugins::{PluginHost, PluginInfo};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub(crate) struct Plugins {
    pub host: Option<Arc<PluginHost>>,
    directory: String,
    error: Option<String>,
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct PluginsView {
    plugins: Vec<PluginInfo>,
    directory: String,
    errors: Vec<String>,
}

pub(crate) fn initialize(app: &AppHandle) -> Plugins {
    let mut directory = String::new();
    let result = (|| -> Result<PluginHost, String> {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("plugins");
        directory = root.to_string_lossy().into_owned();
        let grants = app
            .path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("plugin-grants.json");
        PluginHost::discover(&root, grants).map_err(|e| format!("{e:#}"))
    })();
    match result {
        Ok(host) => {
            let error = tauri::async_runtime::block_on(host.start_enabled())
                .err()
                .map(|e| format!("{e:#}"));
            if let Some(error) = &error {
                log::error!("plugin startup: {error}");
            }
            Plugins {
                host: Some(Arc::new(host)),
                directory,
                error,
            }
        }
        Err(error) => {
            log::error!("plugin discovery: {error}");
            Plugins {
                host: None,
                directory,
                error: Some(error),
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
    if !host.has_enabled() {
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
    let items = plugins.host.as_ref().map_or_else(Vec::new, |host| {
        errors.extend_from_slice(host.discovery_errors());
        host.list()
    });
    PluginsView {
        plugins: items,
        directory: plugins.directory.clone(),
        errors,
    }
}

#[tauri::command]
pub(crate) async fn set_plugin_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    capabilities: Vec<String>,
) -> Result<(), String> {
    let host = state
        .plugins
        .host
        .as_ref()
        .ok_or("plugin host unavailable")?;
    host.set_enabled(&id, enabled, capabilities)
        .await
        .map_err(|e| format!("{e:#}"))
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
}
