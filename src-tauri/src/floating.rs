use std::{collections::HashMap, sync::{Arc, Mutex, Weak, atomic::{AtomicU64, Ordering}}};
use postal_core::{ServiceEvent, WhatsAppService};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, ipc::Channel};
use crate::AppState;

const PREFIX: &str = "postal-float-";
const MAX_WINDOWS: usize = 8;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct FloatContext {
    pub account_id: String,
    pub chat: String,
    pub title: String,
    pub connected: bool,
}

struct Binding {
    account_id: String,
    chat: String,
    title: String,
    service: Weak<WhatsAppService>,
    sending: tokio::sync::Mutex<()>,
}

struct Entry {
    binding: Arc<Binding>,
    updates: Option<Channel<()>>,
}

#[derive(Default)]
pub(crate) struct FloatingChats {
    entries: Mutex<HashMap<String, Entry>>,
}

fn chat_target(chat: &str) -> Result<(), String> {
    let (user, server) = chat.split_once('@').ok_or("Invalid floating chat address.")?;
    let numeric = user.bytes().all(|byte| byte.is_ascii_digit());
    let valid = match server {
        "s.whatsapp.net" | "lid" | "newsletter" | "broadcast" => numeric,
        "g.us" => user.split('-').count() <= 2 && user.split('-').all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        _ => false,
    };
    if chat.len() > 256 || user.is_empty() || !valid { return Err("Invalid floating chat address.".into()); }
    Ok(())
}

fn checked_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > 64 * 1024 || text.contains('\0') {
        return Err("Enter a message of at most 64 KiB without NUL characters.".into());
    }
    Ok(())
}

fn title(value: &str, fallback: &str) -> String {
    let clean: String = value.chars().filter(|ch| !ch.is_control()).take(120).collect();
    if clean.trim().is_empty() { fallback.to_owned() } else { clean.trim().to_owned() }
}

/// Prefix on the OS window title so tiling compositors can match float
/// windows. The Wayland app_id is shared with the main window, and the
/// Tauri label (`postal-float-<pid>-<n>`) is not visible to the compositor,
/// so sway can only match on `title`:
/// `for_window [title="^Postal Float — "] floating enable`.
pub(crate) const FLOAT_TITLE_PREFIX: &str = "Postal Float — ";

pub(crate) fn float_window_title(chat_title: &str) -> String {
    format!("{FLOAT_TITLE_PREFIX}{chat_title}")
}

fn same_service<T>(weak: &Weak<T>, current: &Arc<T>) -> bool {
    weak.upgrade().is_some_and(|bound| Arc::ptr_eq(&bound, current))
}

fn existing_window(entries: &HashMap<String, Entry>, account: &str, chat: &str) -> Result<Option<String>, String> {
    if let Some((label, _)) = entries.iter().find(|(_, entry)| entry.binding.account_id == account && entry.binding.chat == chat) {
        return Ok(Some(label.clone()));
    }
    if entries.len() >= MAX_WINDOWS { return Err("Close a floating chat before opening another; limit is 8.".into()); }
    Ok(None)
}

fn drain_bindings(registry: &FloatingChats) -> Vec<String> {
    registry.entries.lock().unwrap().drain().map(|(label, _)| label).collect()
}

fn binding(app: &AppHandle, label: &str) -> Result<Arc<Binding>, String> {
    app.state::<FloatingChats>().entries.lock().unwrap().get(label).map(|entry| entry.binding.clone())
        .ok_or_else(|| "Floating chat is closed or its account changed.".into())
}

fn check(app: &AppHandle, label: &str, bound: &Arc<Binding>, service: &Arc<WhatsAppService>) -> Result<(), String> {
    let current = binding(app, label)?;
    if !Arc::ptr_eq(&current, bound) || app.get_webview_window(label).is_none() {
        return Err("Floating chat binding changed.".into());
    }
    let active = app.state::<AppState>().account_service(&bound.account_id)?;
    if !Arc::ptr_eq(service, &active) || !same_service(&bound.service, &active) {
        return Err("Floating chat account changed.".into());
    }
    Ok(())
}

fn lease(window: &WebviewWindow) -> Result<(Arc<Binding>, Arc<WhatsAppService>), String> {
    let bound = binding(window.app_handle(), window.label())?;
    let service = bound.service.upgrade().ok_or("Floating chat service stopped.")?;
    check(window.app_handle(), window.label(), &bound, &service)?;
    Ok((bound, service))
}

#[tauri::command]
pub(crate) async fn open_float_chat(window: WebviewWindow, state: State<'_, AppState>, account_id: String, chat: String) -> Result<(), String> {
    if window.label() != "main" { return Err("Only the main window can open floating chats.".into()); }
    chat_target(&chat)?;
    let service = state.account_service(&account_id)?;
    let catalog = service.switcher_catalog().await.map_err(|error| error.to_string())?;
    if !Arc::ptr_eq(&service, &state.account_service(&account_id)?) { return Err("Account changed while opening chat.".into()); }
    let name = catalog.iter().find(|entry| entry.jid == chat).map(|entry| entry.name.as_str()).unwrap_or(&chat);
    let app = window.app_handle();
    let registry = app.state::<FloatingChats>();
    let (label, bound, existing) = {
        let mut entries = registry.entries.lock().unwrap();
        if let Some(label) = existing_window(&entries, &account_id, &chat)? {
            let bound = entries.get(&label).unwrap().binding.clone();
            (label, bound, true)
        } else {
            let label = format!("{PREFIX}{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
            let bound = Arc::new(Binding { account_id, chat: chat.clone(), title: title(name, &chat), service: Arc::downgrade(&service),
                sending: tokio::sync::Mutex::new(()) });
            entries.insert(label.clone(), Entry { binding: bound.clone(), updates: None });
            (label, bound, false)
        }
    };
    if existing {
        check(app, &label, &bound, &service)?;
        let existing = app.get_webview_window(&label).ok_or("Floating chat is still opening.")?;
        let _ = existing.set_title(&float_window_title(&bound.title));
        existing.unminimize().and_then(|_| existing.show()).and_then(|_| existing.set_focus()).map_err(|error| error.to_string())?;
        return check(app, &label, &bound, &service);
    }
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("float".into()))
        .title(float_window_title(&bound.title)).inner_size(420.0, 620.0).min_inner_size(300.0, 300.0)
        .transparent(true).background_color(tauri::window::Color(0, 0, 0, 0)).disable_drag_drop_handler().build();
    let created = match built {
        Ok(created) => created,
        Err(error) => { registry.entries.lock().unwrap().remove(&label); return Err(error.to_string()); }
    };
    let destroyed_app = app.clone(); let destroyed_label = label.clone();
    created.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            destroyed_app.state::<FloatingChats>().entries.lock().unwrap().remove(&destroyed_label);
        }
    });
    if let Err(error) = check(app, &label, &bound, &service) {
        registry.entries.lock().unwrap().remove(&label); let _ = created.close(); return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn float_context(window: WebviewWindow) -> Result<FloatContext, String> {
    let (bound, service) = lease(&window)?;
    Ok(FloatContext { account_id: bound.account_id.clone(), chat: bound.chat.clone(), title: bound.title.clone(), connected: service.is_connected() })
}

#[tauri::command]
pub(crate) fn float_subscribe(window: WebviewWindow, updates: Channel<()>) -> Result<(), String> {
    let (bound, service) = lease(&window)?;
    check(window.app_handle(), window.label(), &bound, &service)?;
    let registry = window.state::<FloatingChats>(); let mut entries = registry.entries.lock().unwrap();
    let entry = entries.get_mut(window.label()).ok_or("Floating chat is closed.")?;
    if !Arc::ptr_eq(&entry.binding, &bound) { return Err("Floating chat binding changed.".into()); }
    entry.updates = Some(updates);
    Ok(())
}

fn strip_paths(page: &mut postal_core::store::MessagePage) {
    for message in &mut page.messages {
        message.media.path = None; message.media.thumb = None;
        message.quote.path = None; message.quote.thumb = None;
        message.media.locator = None; message.quote.locator = None;
        if message.link.thumb.as_deref().is_some_and(|thumb| !thumb.starts_with("data:image/")) { message.link.thumb = None; }
    }
}

#[tauri::command]
pub(crate) async fn float_message_page(window: WebviewWindow, limit: Option<u32>, cursor: Option<postal_core::store::MessageCursor>)
    -> Result<postal_core::store::MessagePage, String> {
    let (bound, service) = lease(&window)?;
    let result = service.message_page(&bound.chat, limit.unwrap_or(200).clamp(1, 500), cursor,
        postal_core::store::MessagePageDirection::Before, None).await;
    check(window.app_handle(), window.label(), &bound, &service)?;
    let mut page = result.map_err(|error| error.to_string())?; strip_paths(&mut page);
    Ok(page)
}

#[tauri::command]
pub(crate) async fn float_send_text(window: WebviewWindow, text: String) -> Result<(), String> {
    checked_text(&text)?;
    let (bound, service) = lease(&window)?;
    let _send = bound.sending.lock().await;
    let app = window.app_handle().clone(); let label = window.label().to_owned();
    check(&app, &label, &bound, &service)?;
    if !service.is_connected() { return Err("Connect this account before replying.".into()); }
    postal_core::service::writable_target(&bound.chat).map_err(|error| error.to_string())?;
    if bound.chat.ends_with("@newsletter") { return Err("Channel posting is unavailable in floating chats.".into()); }
    if bound.chat.ends_with("@g.us") {
        let info = service.group_info(&bound.chat).await;
        check(&app, &label, &bound, &service)?;
        if !info.map_err(|error| error.to_string())?.can_send { return Err("You cannot send to this group or community.".into()); }
    }
    let guard_app = app.clone(); let guard_label = label.clone(); let guard_bound = bound.clone(); let guard_service = service.clone();
    let result = service.send_text_checked(&bound.chat, text, Vec::new(), move || {
        check(&guard_app, &guard_label, &guard_bound, &guard_service).and_then(|_| {
            if guard_service.is_connected() { Ok(()) } else { Err("Account disconnected before send.".into()) }
        }).map_err(|error| std::io::Error::new(std::io::ErrorKind::PermissionDenied, error).into())
    }).await;
    let current = check(&app, &label, &bound, &service);
    match result { Ok(()) => Ok(()), Err(error) => { current?; Err(crate::connection::command_error(&service, error)) } }
}

#[tauri::command]
pub(crate) fn close_float_chat(window: WebviewWindow) -> Result<(), String> {
    binding(window.app_handle(), window.label())?;
    window.state::<FloatingChats>().entries.lock().unwrap().remove(window.label());
    window.close().map_err(|error| error.to_string())
}

pub(crate) fn invalidate_all(app: &AppHandle) {
    let labels = drain_bindings(&app.state::<FloatingChats>());
    for label in labels { if let Some(window) = app.get_webview_window(&label) { let _ = window.close(); } }
}

fn relevant(event: &ServiceEvent, chat: &str) -> bool {
    match event {
        ServiceEvent::Message { message } => message.header.chat == chat,
        ServiceEvent::MessageHint { chat: target, .. } | ServiceEvent::Marks { chat: target }
        | ServiceEvent::ChatStateChanged { chat: target } | ServiceEvent::GroupChanged { chat: target }
        | ServiceEvent::ChatPinRemoved { chat: target } => target == chat,
        ServiceEvent::HistoryLoaded { chats } => chats.iter().any(|target| target == chat),
        ServiceEvent::Connected | ServiceEvent::Disconnected | ServiceEvent::LoggedOut | ServiceEvent::StoreChanged
        | ServiceEvent::NamesUpdated { .. } | ServiceEvent::RetentionApplied { .. } | ServiceEvent::Synced
        | ServiceEvent::InitialSyncComplete { .. } => true,
        _ => false,
    }
}

pub(crate) fn notify(app: &AppHandle, event: &ServiceEvent) {
    let targets: Vec<_> = app.state::<FloatingChats>().entries.lock().unwrap().iter()
        .filter(|(_, entry)| relevant(event, &entry.binding.chat))
        .filter_map(|(label, entry)| entry.updates.clone().map(|updates| (label.clone(), entry.binding.clone(), updates))).collect();
    for (label, bound, updates) in targets {
        let Some(service) = bound.service.upgrade() else { continue };
        if check(app, &label, &bound, &service).is_ok() && updates.send(()).is_err() {
            let registry = app.state::<FloatingChats>(); let mut entries = registry.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(&label) { if Arc::ptr_eq(&entry.binding, &bound) { entry.updates = None; } }
        }
    }
}

#[cfg(test)]
#[path = "floating_tests.rs"]
mod tests;
