use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use crate::{AppState, account_store::active_account};
use crate::command_error::{CommandError, CommandResult};
use crate::native_locale::{english_text, text};

struct TrayLabels {
    show: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

pub(crate) const WINDOW_SHORTCUT: &str = "Control+Alt+P";

pub(crate) fn setup_shortcut(app: &AppHandle) -> CommandResult<()> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    app.global_shortcut().on_shortcut(WINDOW_SHORTCUT, |app, _, event| {
        if event.state == ShortcutState::Pressed {
            if let Err(error) = toggle_main(app) {
                log::warn!("could not toggle Postal: {error}");
            }
        }
    }).map_err(CommandError::operation_failed)
}

fn hide_on_toggle(visible: bool, minimized: bool) -> bool { visible && !minimized }

fn toggle_main(app: &AppHandle) -> CommandResult<()> {
    let window = app.get_webview_window("main").ok_or_else(|| CommandError::code("error.native_main_window_unavailable"))?;
    if hide_on_toggle(window.is_visible().map_err(CommandError::operation_failed)?, window.is_minimized().map_err(CommandError::operation_failed)?) {
        window.hide().map_err(CommandError::operation_failed)
    } else {
        window.unminimize().and_then(|_| window.show()).and_then(|_| window.set_focus()).map_err(CommandError::operation_failed)
    }
}

fn unread_tooltip(count: u32) -> String {
    match count {
        0 => english_text("native.tray_name"),
        1 => english_text("native.tray_unread_one_fallback").replace("{count}", "1"),
        count => english_text("native.tray_unread_many_fallback").replace("{count}", &count.to_string()),
    }
}

fn validated_label(value: Option<String>) -> CommandResult<Option<String>> {
    if value.as_deref().is_some_and(|label| label.chars().count() > 160 || label.chars().any(char::is_control)) {
        return Err(CommandError::new(postal_core::message_ref::MessageRef::new("error.desktop_label_invalid")
            .with_param("max", serde_json::Number::from(160))));
    }
    Ok(value)
}

pub(crate) fn refresh_labels(app: &AppHandle) -> CommandResult<()> {
    let labels = app.try_state::<TrayLabels>().ok_or_else(|| CommandError::code("error.native_tray_unavailable"))?;
    labels.show.set_text(text(app, "native.tray_show")).map_err(CommandError::operation_failed)?;
    labels.quit.set_text(text(app, "native.tray_quit")).map_err(CommandError::operation_failed)
}

#[tauri::command]
pub(crate) fn desktop_unread(
    app: AppHandle, state: State<'_, AppState>, account_id: Option<String>, count: u32,
    tooltip: Option<String>, badge_label: Option<String>,
) -> CommandResult<()> {
    if active_account(&state) != account_id || (account_id.is_none() && count != 0) {
        return Err(CommandError::code("error.account_changed"));
    }
    let tooltip = validated_label(tooltip)?;
    let badge_label = validated_label(badge_label)?;
    let tray = app.tray_by_id("main").ok_or_else(|| CommandError::code("error.native_tray_unavailable"))?;
    tray.set_tooltip(Some(tooltip.unwrap_or_else(|| unread_tooltip(count)))).map_err(CommandError::operation_failed)?;
    #[cfg(not(windows))]
    tray.set_title((count > 0).then(|| badge_label.unwrap_or_else(|| count.to_string()))).map_err(CommandError::operation_failed)?;
    #[cfg(windows)]
    let _ = badge_label;
    Ok(())
}

pub(crate) fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let tray = app.tray_by_id("main").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, text(app, "error.native_tray_unavailable"))
    })?;
    let window = app.get_webview_window("main").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            text(app, "error.native_main_window_unavailable"),
        )
    })?;
    let show = MenuItem::with_id(app, "postal-tray-show", text(app, "native.tray_show"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "postal-tray-quit", text(app, "native.tray_quit"), true, None::<&str>)?;
    tray.set_menu(Some(Menu::with_items(app, &[&show, &quit])?))?;
    tray.set_tooltip(Some(text(app, "native.tray_name")))?;
    app.manage(TrayLabels { show, quit });
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
        assert_eq!(unread_tooltip(0), english_text("native.tray_name"));
        assert_eq!(unread_tooltip(1), english_text("native.tray_unread_one_fallback").replace("{count}", "1"));
        assert_eq!(unread_tooltip(42), english_text("native.tray_unread_many_fallback").replace("{count}", "42"));
        assert_eq!(unread_tooltip(u32::MAX), english_text("native.tray_unread_many_fallback").replace("{count}", "4294967295"));
        assert!(unread_tooltip(1).contains('1'));
        assert!(unread_tooltip(u32::MAX).contains("4294967295"));
    }

    #[test]
    fn desktop_labels_bound_unicode_and_keep_localized_content() {
        let label = "🙂".repeat(160);
        assert_eq!(validated_label(Some(label.clone())).unwrap(), Some(label));
        assert!(validated_label(Some("🙂".repeat(161))).is_err());
        assert!(validated_label(Some("bad\0label".into())).is_err());
        assert!(validated_label(Some("bad\nlabel".into())).is_err());
        assert_eq!(validated_label(None).unwrap(), None);
        assert_eq!(validated_label(Some("\u{2068}٤٢\u{2069}".into())).unwrap().as_deref(), Some("\u{2068}٤٢\u{2069}"));
        let error = validated_label(Some("x".repeat(161))).unwrap_err();
        assert_eq!(error.message.code, "error.desktop_label_invalid");
    }
}
