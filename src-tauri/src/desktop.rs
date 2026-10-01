use tauri::{AppHandle, State};
use crate::{AppState, account_store::{active_account, config_for}};

pub(crate) fn apply_start_on_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if manager.is_enabled().map_err(|error| error.to_string())? != enabled {
        if enabled { manager.enable() } else { manager.disable() }.map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct DesktopStatus {
    pub start_on_login: bool,
    pub shortcut_registered: bool,
}

#[tauri::command]
pub(crate) fn get_desktop_status(app: AppHandle) -> Result<DesktopStatus, String> {
    use tauri_plugin_autostart::ManagerExt;
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    Ok(DesktopStatus {
        start_on_login: app.autolaunch().is_enabled().map_err(|error| error.to_string())?,
        shortcut_registered: app.global_shortcut().is_registered(crate::tray::WINDOW_SHORTCUT),
    })
}

/// Opens a downloaded media file with the desktop's default application.
///
/// The path is restricted to the configured media folder. The webview is the
/// least trusted part of the app, and it must not be able to ask the shell to
/// open arbitrary files.
#[tauri::command(async)]
pub(crate) fn open_path(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let configured = state
        .service
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|service| service.media_dir())
        .or_else(|| {
            let account = active_account(&state)?;
            let settings = state.settings.lock().unwrap().clone();
            config_for(&app, &settings, &account).media_dir
        });

    let dir = configured
        .and_then(|dir| dunce::canonicalize(dir).ok())
        .ok_or("no media folder is configured")?;
    let target = dunce::canonicalize(&path).map_err(|e| e.to_string())?;
    if !target.starts_with(&dir) {
        return Err("refusing to open a file outside the media folder".into());
    }
    shell_open(target.as_os_str())
}

/// Opens a file or URL with the desktop's default handler.
pub(crate) fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(target).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(target).spawn().map_err(|e| e.to_string())?;
    // Not `cmd /C start`: cmd re-parses the target, so a `&` in a URL runs a
    // command, and it flashes a console window.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let file: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as usize;
        // Values above 32 mean success.
        if code <= 32 {
            return Err(format!("the shell could not open it (error {code})"));
        }
    }
    Ok(())
}

/// Opens an http(s) URL in the desktop's default browser.
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) links are opened".into());
    }
    shell_open(url.as_ref())
}

/// Renders a pairing code as SVG for the UI to display.
#[tauri::command]
pub(crate) fn qr_svg(value: String) -> Result<String, String> {
    postal_core::qr_svg(&value).map_err(|e| e.to_string())
}

pub(crate) fn is_hyprland() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
        || std::env::var("XDG_CURRENT_DESKTOP")
            .map(|v| v.eq_ignore_ascii_case("hyprland"))
            .unwrap_or(false)
}

pub(crate) fn is_sway() -> bool {
    std::env::var("SWAYSOCK").is_ok()
        || std::env::var("XDG_CURRENT_DESKTOP")
            .map(|v| v.eq_ignore_ascii_case("sway"))
            .unwrap_or(false)
}

pub(crate) fn is_tiling() -> bool {
    is_hyprland() || is_sway()
}
