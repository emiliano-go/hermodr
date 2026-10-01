use crate::AppState;
use postal_core::WhatsAppService;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct DesktopChatTarget {
    pub account_id: String,
    pub chat: String,
}

fn opens_chat(response: &notify_rust::NotificationResponse) -> bool {
    matches!(response, notify_rust::NotificationResponse::Default)
        || matches!(response, notify_rust::NotificationResponse::Action(action) if action == "open-chat")
}

fn current(state: &AppState, account_id: &str, service: &Arc<WhatsAppService>) -> Result<(), String> {
    if !Arc::ptr_eq(service, &state.service_for_account(account_id)?) {
        return Err("account changed".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn chat_sound_muted(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> Result<Option<bool>, String> {
    let service = state.service_for_account(&account_id)?;
    let muted = service.chat_sound_muted(&chat).await.map_err(|e| e.to_string())?;
    current(&state, &account_id, &service)?;
    Ok(muted)
}

#[tauri::command]
pub(crate) async fn set_chat_sound_muted(
    state: State<'_, AppState>, account_id: String, chat: String, muted: Option<bool>,
) -> Result<(), String> {
    let service = state.service_for_account(&account_id)?;
    service.set_chat_sound_muted(&chat, muted).await.map_err(|e| e.to_string())?;
    current(&state, &account_id, &service)
}

fn notification(title: &str, body: &str, muted: bool) -> notify_rust::Notification {
    let mut note = notify_rust::Notification::new();
    note.summary(title).body(body).auto_icon().action("open-chat", "Open chat");
    #[cfg(all(unix, not(target_os = "macos")))]
    note.action("default", "Open chat");
    #[cfg(all(unix, not(target_os = "macos")))]
    if muted {
        note.hint(notify_rust::Hint::SuppressSound(true));
    }
    #[cfg(any(windows, target_os = "macos"))]
    let _ = muted;
    note
}

#[tauri::command]
pub(crate) async fn show_chat_notification(
    app: AppHandle, state: State<'_, AppState>, account_id: String, chat: String,
    title: String, body: String,
) -> Result<(), String> {
    let service = state.service_for_account(&account_id)?;
    let muted = service.chat_sound_muted(&chat).await.map_err(|e| e.to_string())?.unwrap_or(false);
    let note = notification(&title, &body, muted);
    #[cfg(windows)]
    let note = {
        let mut note = note;
        let exe = tauri::utils::platform::current_exe().map_err(|e| e.to_string())?;
        let directory = exe.parent().ok_or("executable directory unavailable")?;
        let directory = directory.display().to_string();
        let sep = std::path::MAIN_SEPARATOR;
        if !(directory.ends_with(format!("{sep}target{sep}debug").as_str())
            || directory.ends_with(format!("{sep}target{sep}release").as_str())) {
            note.app_id(&app.config().identifier);
        }
        note
    };
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application(if tauri::is_dev() { "com.apple.Terminal" } else { &app.config().identifier });
    current(&state, &account_id, &service)?;
    if !state.settings.lock().unwrap().notifications_enabled {
        return Ok(());
    }
    let service_ref = Arc::downgrade(&service);
    drop(service);
    let target = DesktopChatTarget { account_id, chat };
    let (shown, result) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(service) = service_ref.upgrade() else { let _ = shown.send(Err("account changed".into())); return; };
        if let Err(error) = current(&app.state::<AppState>(), &target.account_id, &service) {
            let _ = shown.send(Err(error)); return;
        }
        if !app.state::<AppState>().settings.lock().unwrap().notifications_enabled {
            let _ = shown.send(Ok(())); return;
        }
        let handle = match note.show() {
            Ok(handle) => handle,
            Err(error) => { let _ = shown.send(Err(error.to_string())); return; }
        };
        let _ = shown.send(Ok(()));
        let service_ref = Arc::downgrade(&service);
        drop(service);
        if let Err(error) = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            if !opens_chat(response) { return; }
            let Some(service) = service_ref.upgrade() else { return; };
            if current(&app.state::<AppState>(), &target.account_id, &service).is_err() { return; }
            crate::tray::show_main(&app);
            if let Err(error) = app.emit("desktop-open-chat", &target) {
                log::warn!("could not open notification chat: {error}");
            }
        }) {
            log::warn!("could not listen for notification action: {error}");
        }
    });
    result.await.map_err(|_| "notification worker stopped".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_muting_keeps_visual_content_and_normal_defaults() {
        let normal = notification("Synthetic title", "Synthetic body", false);
        let muted = notification("Synthetic title", "Synthetic body", true);
        assert_eq!(normal.summary, "Synthetic title");
        assert_eq!(normal.body, "Synthetic body");
        assert_eq!(normal.summary, muted.summary);
        assert_eq!(normal.body, muted.body);
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(!normal.hints.contains(&notify_rust::Hint::SuppressSound(true)));
            assert!(muted.hints.contains(&notify_rust::Hint::SuppressSound(true)));
        }
        #[cfg(any(windows, target_os = "macos"))]
        assert_eq!(format!("{normal:?}"), format!("{muted:?}"));
    }

    #[test]
    fn desktop_notification_clicks_only_open_matching_actions() {
        use notify_rust::{CloseReason, NotificationResponse};
        assert!(opens_chat(&NotificationResponse::Default));
        assert!(opens_chat(&NotificationResponse::Action("open-chat".into())));
        assert!(!opens_chat(&NotificationResponse::Action("unrelated".into())));
        assert!(!opens_chat(&NotificationResponse::Closed(CloseReason::Dismissed)));
        assert!(!opens_chat(&NotificationResponse::Reply("open-chat".into())));
        let target = DesktopChatTarget { account_id: "synthetic-account".into(), chat: "synthetic-chat".into() };
        assert_eq!(serde_json::to_value(target).unwrap(), serde_json::json!({"account_id":"synthetic-account", "chat":"synthetic-chat"}));
    }
}
