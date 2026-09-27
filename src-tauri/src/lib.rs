//! Tauri shell composition and shared application state.

use std::sync::{Arc, Mutex};
use postal_core::WhatsAppService;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use account_store::{AccountsFile, load_accounts};
use desktop::is_hyprland;
use logging::{init_logging, log_path};
use migration::{migrate_bundle_id, migrate_media};
use settings::load_settings;
pub use settings::UiSettings;
pub use account_store::{Account, AccountsView};
pub use connection::ConnectionState;

mod account_store;
mod accounts;
mod connection;
mod settings;
mod chats;
mod messages;
mod media;
mod media_actions;
mod groups;
mod contacts;
mod polls;
mod desktop;
mod migration;
mod logging;
#[cfg(test)]
mod tests;

/// Event name the frontend listens on for service updates.
const SERVICE_EVENT: &str = "service-event";
/// Event name for the optional Android instance's own state (QR, connect).
const ONCE_EVENT: &str = "once-event";

struct AppState {
    service: Mutex<Option<Arc<WhatsAppService>>>,
    /// The optional Android instance, running beside the main service.
    once_service: Mutex<Option<Arc<WhatsAppService>>>,
    /// Its pairing code while it waits to be linked.
    once_qr: Mutex<Option<String>>,
    once_connected: std::sync::atomic::AtomicBool,
    settings: Mutex<UiSettings>,
    accounts: Mutex<AccountsFile>,
}

impl AppState {
    fn service(&self) -> Result<Arc<WhatsAppService>, String> {
        self.service
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "not connected yet".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before any app path resolves, adopt an install from before the rename.
    migrate_bundle_id();

    // WebKitGTK's DMA-BUF renderer fails to create GBM buffers under Wayland
    // (Hyprland), aborting with "Gdk Error 71". This affects our own UI webview
    // as much as it did the old one.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init());
    // A second launch hands its arguments to the running instance and exits,
    // so one process at a time owns the WhatsApp session. The dev server is
    // exempt so a dev instance can run beside the installed app.
    let builder = if tauri::is_dev() {
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
    };

    builder
        .setup(|app| {
            init_logging(&log_path(app.handle()));
            let accounts = load_accounts(app.handle());
            migrate_media(app.handle(), &accounts);

            app.manage(AppState {
                service: Mutex::new(None),
                once_service: Mutex::new(None),
                once_qr: Mutex::new(None),
                once_connected: std::sync::atomic::AtomicBool::new(false),
                settings: Mutex::new(load_settings(app.handle())),
                accounts: Mutex::new(accounts),
            });

            // Built here rather than from the config so clipboard access can be
            // turned on. WebKitGTK only hands pasted images to the page when
            // `javascript_can_access_clipboard` is set, and it does not deliver
            // them through the paste event's clipboardData.
            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Postal")
                .inner_size(1000.0, 720.0)
                .min_inner_size(480.0, 360.0)
                .decorations(!is_hyprland())
                .enable_clipboard_access();
            // WebView2 only delivers dropped files to the page's drop handler
            // when Tauri's own drag and drop handler is off.
            #[cfg(target_os = "windows")]
            let builder = builder.disable_drag_drop_handler();
            let _window = builder.build()?;
            // Only the dev server gets the inspector; scripts/install-dev.sh
            // installs debug builds, which are not "dev" runs.
            if tauri::is_dev() {
                // _window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection::connection_state,
            connection::boolean_props,
            connection::connect,
            accounts::accounts,
            accounts::add_account,
            accounts::switch_account,
            accounts::remove_account,
            accounts::rename_account,
            messages::messages,
            chats::chats,
            contacts::resolve_names,
            messages::mark_read,
            messages::mark_read_until,
            messages::send_reply,
            media::send_media,
            messages::send_text,
            messages::edit_message,
            desktop::open_path,
            media_actions::message_media_action,
            media::read_file,
            groups::participants,
            groups::group_info,
            groups::group_kinds,
            chats::set_pinned,
            chats::set_archived,
            chats::set_muted,
            chats::set_marked_unread,
            groups::leave_group,
            messages::unread_mentions,
            contacts::avatar,
            contacts::names,
            media::send_voice,
            media::open_view_once,
            messages::mark_played,
            messages::starred_messages,
            messages::pings,
            messages::search_messages,
            polls::edit_event,
            chats::set_chat_retention,
            chats::chat_settings,
            groups::admin_reports,
            groups::set_allow_admin_reports,
            media::save_sticker,
            contacts::user_profile,
            groups::invite_info,
            groups::join_invite,
            messages::message_info,
            accounts::own_jid,
            contacts::send_typing,
            contacts::set_online,
            contacts::watch_presence,
            contacts::profile,
            contacts::set_about,
            contacts::set_profile_picture,
            groups::set_member_label,
            messages::react,
            messages::star,
            messages::pin_message,
            messages::delete_message,
            messages::report_message,
            messages::forward_message,
            messages::marks,
            media::send_sticker,
            media::media_library,
            media::send_from_library,
            polls::create_poll,
            polls::vote_poll,
            polls::create_event,
            polls::respond_event,
            contacts::set_push_name,
            contacts::set_privacy,
            messages::load_older,
            messages::backfill_history,
            media::flush_media,
            chats::clear_history,
            chats::clear_chat,
            chats::delete_chat,
            logging::frontend_log,
            logging::open_log,
            media::download_media,
            media::recover_quote_media,
            chats::set_chat_auto_download,
            chats::set_chat_privacy,
            contacts::contact_aliases,
            contacts::add_contact_alias,
            contacts::remove_contact_alias,
            messages::chat_for_message,
            contacts::search,
            desktop::open_url,
            desktop::qr_svg,
            settings::get_settings,
            settings::set_settings,
            connection::once_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
