use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use crate::{AppState, account_store::active_account};

pub(crate) const WINDOW_SHORTCUT: &str = "Control+Alt+P";

pub(crate) fn setup_shortcut(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    app.global_shortcut().on_shortcut(WINDOW_SHORTCUT, |app, _, event| {
        if event.state == ShortcutState::Pressed {
            if let Err(error) = toggle_main(app) {
                log::warn!("could not toggle Postal: {error}");
            }
        }
    }).map_err(|error| error.to_string())
}

fn hide_on_toggle(visible: bool, minimized: bool) -> bool { visible && !minimized }

fn toggle_main(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("Postal main window is missing")?;
    if hide_on_toggle(window.is_visible().map_err(|error| error.to_string())?, window.is_minimized().map_err(|error| error.to_string())?) {
        window.hide().map_err(|error| error.to_string())
    } else {
        window.unminimize().and_then(|_| window.show()).and_then(|_| window.set_focus()).map_err(|error| error.to_string())
    }
}

fn unread_tooltip(count: u32) -> String {
    match count {
        0 => "Postal".into(),
        1 => "Postal — 1 unread message".into(),
        count => format!("Postal — {count} unread messages"),
    }
}

#[tauri::command]
pub(crate) fn desktop_unread(app: AppHandle, state: State<'_, AppState>, account_id: Option<String>, count: u32) -> Result<(), String> {
    if active_account(&state) != account_id || (account_id.is_none() && count != 0) {
        return Err("account changed".into());
    }
    let tray = app.tray_by_id("main").ok_or("Postal tray icon is missing")?;
    tray.set_tooltip(Some(unread_tooltip(count))).map_err(|error| error.to_string())?;
    #[cfg(not(windows))]
    tray.set_title((count > 0).then(|| count.to_string())).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let tray = app.tray_by_id("main").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Postal tray icon is missing")
    })?;
    let window = app.get_webview_window("main").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Postal main window is missing",
        )
    })?;
    let show = MenuItem::with_id(app, "postal-tray-show", "Show Postal", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "postal-tray-quit", "Quit", true, None::<&str>)?;
    tray.set_menu(Some(Menu::with_items(app, &[&show, &quit])?))?;
    tray.set_tooltip(Some("Postal"))?;
    tray.set_show_menu_on_left_click(false)?;
    tray.on_menu_event(|app, event| match event.id().as_ref() {
        "postal-tray-show" => show_main(app),
        "postal-tray-quit" => app.exit(0),
        _ => {}
    });
    tray.on_tray_icon_event(|tray, event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            show_main(tray.app_handle());
        }
    });
    let main = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            match main.hide() {
                Ok(()) => api.prevent_close(),
                Err(error) => log::warn!("could not close Postal to tray: {error}"),
            }
        }
    });
    Ok(())
}

pub(crate) fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.unminimize() {
            log::warn!("could not restore Postal: {error}");
        }
        if let Err(error) = window.show().and_then(|_| window.set_focus()) {
            log::warn!("could not show Postal: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_toggle_restores_hidden_or_minimized_windows() {
        assert!(hide_on_toggle(true, false));
        assert!(!hide_on_toggle(false, false));
        assert!(!hide_on_toggle(true, true));
        assert!(!hide_on_toggle(false, true));
    }

    #[test]
    fn desktop_unread_labels_include_real_counts_and_reset() {
        assert_eq!(unread_tooltip(0), "Postal");
        assert_eq!(unread_tooltip(1), "Postal — 1 unread message");
        assert_eq!(unread_tooltip(42), "Postal — 42 unread messages");
        assert_eq!(unread_tooltip(u32::MAX), "Postal — 4294967295 unread messages");
    }
}
