use std::path::PathBuf;
use postal_core::{DiskRetention, WhatsAppService};
use tauri::{AppHandle, Manager, State};
use crate::AppState;

/// Settings the UI can change.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub retention: DiskRetention,
    pub message_window_size: u32,
    /// Requests deep history during pairing, independently of disk retention.
    #[serde(alias = "accept_full_history")]
    pub request_full_history: bool,
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
    /// Skip the initial-sync loading screen and show the chat UI immediately.
    /// Off holds the loading screen until the initial backlog is applied.
    #[serde(default)]
    pub skip_loading_screen: bool,
    /// Whether archived chats stay archived when a new message arrives. Off
    /// moves the chat back to the main list.
    #[serde(default = "default_true")]
    pub keep_archived: bool,
    /// Whether the optional Android instance runs: a second link that fetches
    /// one-time media the External companion never receives. Off stops it
    /// without unlinking; the link stays paired for next time.
    #[serde(default)]
    pub android_instance: bool,
    /// Global kill switch for desktop notifications. Muted chats never
    /// notify, whatever this is set to.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Whether the chat list keeps its order while the pointer is over it.
    /// Previews still update in place; the new order applies once the pointer
    /// leaves or a chat is opened. Off reorders immediately.
    #[serde(default = "default_true")]
    pub freeze_chat_list_on_hover: bool,
}

pub(crate) fn default_true() -> bool {
    true
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            retention: DiskRetention::default(),
            message_window_size: 500,
            request_full_history: false,
            auto_download_media: true,
            media_dir: None,
            warn_missing_video_preview: true,
            send_typing: true,
            send_receipts: true,
            keep_history: true,
            skip_loading_screen: false,
            keep_archived: true,
            android_instance: false,
            notifications_enabled: true,
            freeze_chat_list_on_hover: true,
        }
    }
}

pub(crate) fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

/// Saved settings, or the defaults when none were saved or they do not parse.
pub(crate) fn load_settings(app: &AppHandle) -> UiSettings {
    std::fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|json| parse_settings(&json).ok())
        .unwrap_or_default()
}

fn parse_settings(json: &str) -> serde_json::Result<UiSettings> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let legacy_unlimited = value.get("request_full_history").is_none()
        && value.get("accept_full_history").and_then(|v| v.as_bool()) == Some(true);
    let mut settings: UiSettings = serde_json::from_value(value)?;
    if legacy_unlimited { settings.retention = DiskRetention::unlimited(); }
    settings.message_window_size = settings.message_window_size.clamp(50, postal_core::store::MAX_MESSAGE_PAGE);
    Ok(settings)
}

/// Whether a chat gets our (typing, read receipts): its overrides, else the global settings.
pub(crate) async fn sends_privacy(state: &AppState, service: &WhatsAppService, chat: &str) -> (bool, bool) {
    let (typing, receipts) = match service.chat_privacy(chat).await {
        Ok(privacy) => privacy,
        Err(error) => {
            log::error!("could not read chat privacy settings: {error}");
            return (false, false);
        }
    };
    let settings = state.settings.lock().unwrap();
    (
        typing.unwrap_or(settings.send_typing),
        receipts.unwrap_or(settings.send_receipts) && !service.read_receipts_disabled(),
    )
}

/// Current settings.
#[tauri::command]
pub(crate) fn get_settings(state: State<'_, AppState>) -> UiSettings {
    state.settings.lock().unwrap().clone()
}

/// Updates and saves settings.
///
/// Toggling the Android companion only records the wish: the companion manager
/// wakes or puts the instance to sleep, without touching the main link.
#[tauri::command]
pub(crate) async fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: UiSettings) -> Result<(), String> {
    if !(50..=postal_core::store::MAX_MESSAGE_PAGE).contains(&settings.message_window_size) {
        return Err("The RAM window must contain 50–2,000 messages".into());
    }
    let instance_changed = state.settings.lock().unwrap().android_instance != settings.android_instance;
    // Both links share one store, so the instance cannot run without history;
    // and it is only useful once its own link exists, which pairing creates.
    if instance_changed && settings.android_instance {
        if !settings.keep_history {
            return Err("The Android companion needs \"Download and keep history\" turned on".into());
        }
        let account = crate::account_store::active_account(&state);
        if !account.is_some_and(|id| crate::connection::once_paired(&state, &id)) {
            return Err("Pair the Android companion before turning it on".into());
        }
    }
    let path = settings_path(&app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    if let Ok(service) = state.service() {
        service.set_retention(settings.retention);
        service.set_keep_archived(settings.keep_archived);
    }
    if let Some(service) = state.once_service.lock().unwrap().as_ref() {
        service.set_retention(settings.retention);
        service.set_keep_archived(settings.keep_archived);
    }
    let enabled = settings.android_instance;
    *state.settings.lock().unwrap() = settings;
    if instance_changed {
        crate::connection::wake_once(&app);
        log::info!(
            "Android companion {}",
            if enabled { "enabled; the manager will wake it when needed" } else { "disabled; the manager will put it to sleep" }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use postal_core::store::RetentionLimit;

    #[test]
    fn full_history_migration_preserves_effective_disk_policy() {
        let legacy = r#"{"accept_full_history":true,"retention":{"max_age_hours":24,"max_messages_per_chat":500}}"#;
        let migrated = parse_settings(legacy).unwrap();
        assert!(migrated.request_full_history);
        assert_eq!(migrated.retention, DiskRetention::unlimited());
        let saved = serde_json::to_string(&migrated).unwrap();
        assert!(!saved.contains("accept_full_history"));
        assert_eq!(parse_settings(&saved).unwrap().retention, DiskRetention::unlimited());
        let bounded = parse_settings(&legacy.replace("true", "false")).unwrap();
        assert_eq!(bounded.retention, DiskRetention { max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Limited(500) });
        let explicit = parse_settings(&legacy.replace("accept_full_history", "request_full_history")).unwrap();
        assert!(explicit.request_full_history);
        assert_eq!(explicit.retention.max_age_hours, RetentionLimit::Limited(24));
        assert!(!parse_settings("{}").unwrap().request_full_history);
        assert_eq!(parse_settings("{}").unwrap().retention, DiskRetention::unlimited());
        // Notifications default to on, including for settings saved before the toggle existed.
        assert!(parse_settings("{}").unwrap().notifications_enabled);
        assert!(parse_settings(legacy).unwrap().notifications_enabled);
        assert!(!parse_settings(r#"{"notifications_enabled":false}"#).unwrap().notifications_enabled);
        // The hover freeze defaults to on, including for settings saved before it existed.
        assert!(parse_settings("{}").unwrap().freeze_chat_list_on_hover);
        assert!(parse_settings(legacy).unwrap().freeze_chat_list_on_hover);
        assert!(!parse_settings(r#"{"freeze_chat_list_on_hover":false}"#).unwrap().freeze_chat_list_on_hover);
        assert_eq!(bounded.message_window_size, 500);
    }
}
