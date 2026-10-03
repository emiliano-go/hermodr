//! Tauri shell composition and shared application state.

use std::sync::{Arc, Mutex};
use postal_core::WhatsAppService;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use account_store::{AccountsFile, load_accounts};
use desktop::is_tiling;
use logging::{init_logging, log_path};
use migration::{migrate_bundle_id, migrate_media};
use settings::load_settings;
pub use settings::UiSettings;
pub use account_store::{Account, AccountsView};
pub use connection::ConnectionState;

mod account_store;
mod accounts;
mod archive;
mod connection;
mod command_error;
mod database_keys;
mod database_encryption;
mod native_locale;
mod settings;
mod chats;
mod messages;
mod media;
mod media_access;
mod contact_actions;
mod quick_switcher;
mod keywords;
mod uploads;
mod plugins;
mod media_actions;
mod groups;
mod gallery;
mod devices;
mod favorites;
mod floating;
mod group_requests;
mod scheduled;
mod transcription;
mod notifications;
mod labels;
mod group_audit;
mod member_profiles;
mod quick_replies;
mod albums;
mod contact_sharing;
mod bulk_chats;
mod group_invites;
mod blocked_contacts;
mod group_create;
mod group_settings;
#[cfg(desktop)]
mod tray;
mod transcription_config;
mod transcription_credentials;
mod contacts;
mod usernames;
mod spaces;
mod broadcast_lists;
mod polls;
mod desktop;
mod camera;
mod migration;
mod logging;
#[cfg(feature = "wire-types")]
mod wire;
#[cfg(feature = "wire-types")]
pub use wire::{wire_types, wire_fixture};
#[cfg(test)]
mod tests;

/// Event name the frontend listens on for service updates.
const SERVICE_EVENT: &str = "service-event";
/// Event name for the optional Android instance's own state (QR, connect).
const ONCE_EVENT: &str = "once-event";

struct AppState {
    account_service: Mutex<Option<(String, std::sync::Weak<WhatsAppService>)>>,
    service: Mutex<Option<Arc<WhatsAppService>>>,
    /// The optional Android instance, running beside the main service.
    once_service: Mutex<Option<Arc<WhatsAppService>>>,
    /// Its pairing code while it waits to be linked.
    once_qr: Mutex<Option<String>>,
    once_connected: std::sync::atomic::AtomicBool,
    /// Whether a pairing session was asked for: the companion runs until the
    /// link exists, even though enabling it is not allowed before then.
    once_pairing: std::sync::atomic::AtomicBool,
    /// Pokes the companion manager: a message arrived, settings changed, or an
    /// account was switched, so it re-checks whether to wake the instance.
    once_wake: tokio::sync::Notify,
    settings: Mutex<UiSettings>,
    accounts: Mutex<AccountsFile>,
    uploads: Arc<uploads::Uploads>,
    plugins: plugins::Plugins,
}

impl AppState {
    fn service(&self) -> Result<Arc<WhatsAppService>, String> {
        self.service
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "not connected yet".to_string())
    }

    fn service_for_account(&self, account_id: &str) -> Result<Arc<WhatsAppService>, String> {
        self.account_service(account_id)
    }
}

/// Every command the UI may invoke. A macro rather than a function so the
/// dispatch table does not count against the function-length budget.
macro_rules! postal_commands {
    () => {
        tauri::generate_handler![
            database_encryption::database_encryption_status,
            native_locale::set_native_locale,
            floating::open_float_chat,
            floating::float_context,
            floating::float_subscribe,
            floating::float_message_page,
            floating::float_send_text,
            floating::close_float_chat,
            transcription::transcription_settings,
            transcription::set_transcription_settings,
            transcription::grant_transcription_cloud_consent,
            transcription::configure_transcription_key,
            transcription::forget_transcription_key,
            transcription::install_transcription_model,
            transcription::transcribe_message,
            transcription::cancel_transcription,
            transcription::message_transcript,
            transcription::chat_auto_transcribe,
            transcription::set_chat_auto_transcribe,
            connection::connection_state,
            connection::boolean_props,
            connection::connect,
            connection::request_pair_code,
            connection::cancel_pair_code,
            accounts::accounts,
            accounts::add_account,
            accounts::switch_account,
            accounts::remove_account,
            accounts::rename_account,
            messages::messages,
            messages::message_page,
            chats::chats,
            contacts::resolve_names,
            usernames::lookup_username,
            spaces::spaces_snapshot,
            spaces::spaces_action,
            spaces::resolve_spaces,
            spaces::space_group_catalog,
            spaces::export_space_metadata,
            spaces::import_space_metadata,
            broadcast_lists::broadcast_list,
            messages::mark_read,
            messages::mark_read_until,
            messages::send_reply,
            media::send_media,
            albums::send_album,
            messages::send_text,
            messages::edit_message,
            desktop::open_path,
            media_actions::message_media_action,
            media::read_file,
            media_access::authorize_media_assets,
            contact_actions::save_contact,
            contact_actions::remove_contact,
            quick_switcher::switcher_catalog,
            quick_switcher::switcher_messages,
            keywords::keyword_mentions,
            keywords::keyword_matches,
            media::playable_audio,
            media::playable_video,
            groups::participants,
            groups::group_info,
            groups::group_kinds,
            chats::set_pinned,
            chats::set_archived,
            chats::set_muted,
            chats::set_marked_unread,
            groups::leave_group,
            groups::add_group_participants,
            groups::group_history_offer,
            groups::add_group_participants_with_history,
            groups::retry_group_history,
            favorites::favorite_chats,
            favorites::set_favorite,
            group_requests::group_join_requests,
            group_requests::change_group_join_requests,
            contacts::contact_identities,
            gallery::gallery_page,
            devices::linked_devices,
            devices::unlink_device,
            scheduled::schedule_message,
            scheduled::scheduled_messages,
            scheduled::update_scheduled_message,
            scheduled::cancel_scheduled_message,
            scheduled::retry_scheduled_message,
            scheduled::send_scheduled_message,
            groups::remove_group_participants,
            groups::promote_group_participants,
            groups::demote_group_participants,
            groups::set_members_can_add,
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
            member_profiles::user_profile,
            member_profiles::set_member_note,
            quick_replies::quick_replies_view,
            quick_replies::sync_quick_replies,
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
            messages::delete_messages,
            messages::report_message,
            messages::forward_message,
            messages::marks,
            media::send_sticker,
            media::media_library,
            media::send_from_library,
            media::sticker_library,
            media::sticker_pack,
            media::favorite_sticker,
            media::favorite_sticker_path,
            media::sticker_recent,
            media::fetch_sticker_pack,
            media::download_sticker,
            media::resync_stickers,
            polls::create_poll,
            polls::create_quiz,
            polls::vote_poll,
            polls::create_event,
            polls::respond_event,
            contacts::set_push_name,
            contacts::set_privacy,
            messages::load_older,
            messages::backfill_history,
            media::flush_media,
            media::storage_report,
            archive::export_archive,
            archive::restore_local_backup,
            uploads::begin_upload,
            uploads::append_upload,
            uploads::cancel_upload,
            plugins::list_plugins,
            plugins::set_plugin_enabled,
            media::storage_cleanup,
            chats::clear_history,
            chats::clear_chat,
            chats::delete_chat,
            logging::frontend_log,
            logging::open_log,
            media::download_media,
            media::recover_quote_media,
            chats::set_chat_auto_download,
            chats::chat_media_auto_download,
            chats::set_chat_media_auto_download,
            chats::set_chat_mute_at_all,
            notifications::chat_sound_muted,
            notifications::set_chat_sound_muted,
            notifications::show_chat_notification,
            labels::labels_view,
            labels::save_label,
            labels::delete_label,
            labels::label_chat,
            labels::label_message,
            labels::labelled_messages,
            group_audit::group_audit_page,
            contact_sharing::own_contact_link,
            contact_sharing::resolve_contact_link,
            contact_sharing::send_contacts,
            contact_sharing::message_contacts,
            bulk_chats::mark_all_read,
            chats::chat_unarchive,
            chats::set_chat_unarchive,
            group_invites::group_invite_link,
            group_invites::join_group_invite_message,
            blocked_contacts::blocked_contacts,
            blocked_contacts::set_contact_blocked,
            group_create::create_group,
            group_create::group_creation_contacts,
            group_settings::group_settings,
            group_settings::change_group_setting,
            group_settings::set_group_picture,
            chats::set_chat_privacy,
            contacts::contact_aliases,
            contacts::add_contact_alias,
            contacts::remove_contact_alias,
            messages::chat_for_message,
            contacts::search,
            desktop::open_url,
            desktop::qr_svg,
            desktop::get_desktop_status,
            tray::desktop_unread,
            settings::get_settings,
            settings::set_settings,
            connection::once_state,
            connection::set_pairing
        ]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before any app path resolves, adopt an install from before the rename.
    migrate_bundle_id();
    webkit_renderer_workaround();

    let builder = app_builder();
    // A second launch hands its arguments to the running instance and exits,
    // so one process at a time owns the WhatsApp session. The dev server is
    // exempt so a dev instance can run beside the installed app.
    let builder = if tauri::is_dev() { builder } else { builder.plugin(single_instance()) };

    builder
        .setup(setup_app)
        .invoke_handler(postal_commands!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(handle_run_event);
}

/// WebKitGTK's DMA-BUF renderer trips over Wayland and aborts with
/// "Gdk Error 71". Disabling it costs hardware acceleration and makes the
/// whole webview CPU-painted, so on NVIDIA — where the failure comes from
/// explicit sync — keep the renderer and turn explicit sync off instead.
/// Other Wayland drivers keep the older workaround. X11 needs neither.
fn webkit_renderer_workaround() {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland");
        if !wayland {
            return;
        }
        if nvidia_loaded() {
            if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
                std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
            }
            return;
        }
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

/// Whether an NVIDIA (or nouveau) kernel module is loaded, read from
/// `/proc/modules`. The text is a parameter so the check is unit-testable.
#[cfg(target_os = "linux")]
fn nvidia_loaded() -> bool {
    nvidia_module_loaded(&std::fs::read_to_string("/proc/modules").unwrap_or_default())
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn nvidia_module_loaded(modules: &str) -> bool {
    modules.lines().any(|line| {
        line.split_whitespace().next().is_some_and(|name| {
            name == "nvidia" || name == "nouveau" || name.starts_with("nvidia_")
        })
    })
}

fn app_builder() -> tauri::Builder<tauri::Wry> {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build());
    builder
}

/// A second launch hands its arguments to the running instance and exits.
fn single_instance() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    })
}

fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let mut settings = load_settings(app.handle());
    init_logging(&log_path(app.handle()), settings.verbose_whatsapp_logs);
    #[cfg(desktop)]
    match desktop::get_desktop_status(app.handle().clone()) {
        Ok(status) => settings.start_on_login = status.start_on_login,
        Err(error) => log::warn!("could not read start-on-login state: {error}"),
    }
    let accounts = load_accounts(app.handle());
    database_encryption::initialize(app.handle(), &settings, &accounts);
    migrate_media(app.handle(), &accounts);
    app.manage(transcription::TranscriptionState::load(app.handle())?);
    app.manage(floating::FloatingChats::default());
    app.manage(native_locale::NativeLocale::default());

    app.manage(AppState {
        account_service: Mutex::new(None),
        service: Mutex::new(None),
        once_service: Mutex::new(None),
        once_qr: Mutex::new(None),
        once_connected: std::sync::atomic::AtomicBool::new(false),
        once_pairing: std::sync::atomic::AtomicBool::new(false),
        once_wake: tokio::sync::Notify::new(),
        settings: Mutex::new(settings),
        accounts: Mutex::new(accounts),
        uploads: Arc::default(),
        plugins: plugins::initialize(app.handle()),
    });
    connection::spawn_once_manager(app.handle());
    let main = build_main_window(app.handle())?;
    camera::setup(&main)?;
    #[cfg(desktop)]
    tray::setup(app.handle())?;
    #[cfg(desktop)]
    if let Err(error) = tray::setup_shortcut(app.handle()) {
        log::warn!("could not register Postal window shortcut: {error}");
    }
    Ok(())
}

/// Built here rather than from the config so clipboard access can be turned
/// on. WebKitGTK only hands pasted images to the page when
/// `javascript_can_access_clipboard` is set, and it does not deliver them
/// through the paste event's clipboardData.
fn build_main_window(app: &tauri::AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Postal")
        .inner_size(1000.0, 720.0)
        .min_inner_size(480.0, 360.0)
        .decorations(!is_tiling())
        .enable_clipboard_access();
    // Browser File objects stage bytes; native drops must not grant asset directories.
    let builder = builder.disable_drag_drop_handler();
    builder.build()
}

fn handle_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(host) = &app.state::<AppState>().plugins.host {
                tauri::async_runtime::block_on(host.shutdown());
            }
        }
}

