use std::path::PathBuf;
use postal_core::{Retention, WhatsAppService};
use tauri::{AppHandle, Manager, State};
use crate::AppState;

/// Settings the UI can change.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiSettings {
    pub retention: Retention,
    /// Whether to pull the account's entire history during pairing.
    pub accept_full_history: bool,
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
    /// How this device links: "android" makes WhatsApp send view-once media
    /// here, "external" links as an ordinary companion. Each mode keeps its own
    /// link; switching hot-swaps which session runs and never unlinks.
    #[serde(default = "default_pair_mode")]
    pub pair_mode: String,
    /// Whether an arriving view-once whose media can be fetched is downloaded
    /// and kept as an ordinary attachment instead of one-time.
    #[serde(default = "default_true")]
    pub keep_view_once: bool,
}

fn default_pair_mode() -> String {
    "android".to_string()
}

pub(crate) fn default_true() -> bool {
    true
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            retention: Retention::default(),
            accept_full_history: false,
            auto_download_media: true,
            media_dir: None,
            warn_missing_video_preview: true,
            send_typing: true,
            send_receipts: true,
            keep_history: true,
            skip_loading_screen: false,
            keep_archived: true,
            pair_mode: default_pair_mode(),
            keep_view_once: true,
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
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Whether a chat gets our (typing, read receipts): its overrides, else the global settings.
pub(crate) fn sends_privacy(state: &AppState, service: &WhatsAppService, chat: &str) -> (bool, bool) {
    let (typing, receipts) = match service.chat_privacy(chat) {
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

/// Updates and saves settings. Service settings take effect immediately.
///
/// Changing the device mode hot-swaps which linked session runs; neither mode
/// is unlinked, so the phone keeps both devices and switching back and forth
/// needs no new pairing (only the first time a mode is used).
#[tauri::command]
pub(crate) async fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: UiSettings) -> Result<(), String> {
    let path = settings_path(&app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    let mode_changed = state.settings.lock().unwrap().pair_mode != settings.pair_mode;
    if let Ok(service) = state.service() {
        service.set_retention(settings.retention, settings.accept_full_history);
        service.set_keep_archived(settings.keep_archived);
        service.set_keep_view_once(settings.keep_view_once);
    }
    *state.settings.lock().unwrap() = settings;
    if mode_changed {
        // Both modes stay linked; switching just restarts with the other
        // session file. An unpaired mode shows its QR code once.
        if let Some(account) = crate::account_store::active_account(&state) {
            crate::connection::start_service(&app, &state, &account).await?;
        }
    }
    Ok(())
}

/// How the current account was linked: `"android"` or `"external"`.
#[tauri::command]
pub(crate) fn paired_mode(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.service()?.paired_mode().to_string())
}
