use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

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

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.unminimize() {
            log::warn!("could not restore Postal: {error}");
        }
        if let Err(error) = window.show().and_then(|_| window.set_focus()) {
            log::warn!("could not show Postal: {error}");
        }
    }
}
