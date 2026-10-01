#[cfg(target_os = "linux")]
use gtk::{glib::translate::ToGlibPtr, prelude::*};
#[cfg(target_os = "linux")]
use webkit2gtk::prelude::*;

#[cfg(target_os = "linux")]
pub(crate) fn setup(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use tauri::Manager;

    if window.label() != "main" {
        return Ok(());
    }
    let trusted = if tauri::is_dev() {
        window.config().build.dev_url.clone()
    } else {
        None
    }
    .unwrap_or_else(|| tauri::Url::parse("tauri://localhost").expect("valid app origin"));
    let main = window.clone();
    window.with_webview(move |platform| {
        let parent = match main.gtk_window() {
            Ok(parent) => parent.downgrade(),
            Err(error) => {
                log::error!("Could not install camera permission handler: {error}");
                return;
            }
        };
        let view = platform.inner();
        if let Some(settings) = view.settings() {
            settings.set_enable_media_stream(true);
        }
        view.connect_permission_request(move |view, request| {
            let Some(media) = request.downcast_ref::<webkit2gtk::UserMediaPermissionRequest>()
            else {
                return false;
            };
            // Safe bindings omit WebKit's screen-capture predicate.
            let display = unsafe {
                webkit2gtk::ffi::webkit_user_media_permission_is_for_display_device(
                    media.to_glib_none().0,
                ) != 0
            };
            if !camera_request_allowed(
                view.uri().as_deref(),
                &trusted,
                media.is_for_video_device(),
                media.is_for_audio_device(),
                display,
            ) {
                request.deny();
                return true;
            }
            let Some(parent) = parent.upgrade() else {
                request.deny();
                return true;
            };
            ask_for_camera(view, request, &parent, trusted.clone());
            true
        });
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn setup(_: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn ask_for_camera(
    view: &webkit2gtk::WebView,
    request: &webkit2gtk::PermissionRequest,
    parent: &gtk::ApplicationWindow,
    trusted: tauri::Url,
) {
    use std::{cell::RefCell, rc::Rc};

    let dialog = gtk::MessageDialog::new(
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Question,
        gtk::ButtonsType::None,
        "Allow Postal to use your camera to take a photo?",
    );
    dialog.set_title("Postal camera permission");
    dialog.add_buttons(&[
        ("Cancel", gtk::ResponseType::Cancel),
        ("Allow camera", gtk::ResponseType::Accept),
    ]);
    dialog.set_default_response(gtk::ResponseType::Cancel);
    let pending = Rc::new(RefCell::new(Some(request.clone())));
    let closing = pending.clone();
    dialog.connect_destroy(move |_| {
        let request = closing.borrow_mut().take();
        if let Some(request) = request {
            request.deny();
        }
    });
    let view = view.downgrade();
    dialog.connect_response(move |dialog, response| {
        let request = pending.borrow_mut().take();
        let Some(request) = request else {
            return;
        };
        if response == gtk::ResponseType::Accept
            && view
                .upgrade()
                .is_some_and(|view| trusted_origin(view.uri().as_deref(), &trusted))
        {
            request.allow();
        } else {
            request.deny();
        }
        dialog.close();
    });
    dialog.show_all();
}

#[cfg(any(target_os = "linux", test))]
fn camera_request_allowed(
    uri: Option<&str>,
    trusted: &tauri::Url,
    video: bool,
    audio: bool,
    display: bool,
) -> bool {
    video && !audio && !display && trusted_origin(uri, trusted)
}

#[cfg(any(target_os = "linux", test))]
fn trusted_origin(uri: Option<&str>, trusted: &tauri::Url) -> bool {
    let Some(uri) = uri.and_then(|uri| tauri::Url::parse(uri).ok()) else {
        return false;
    };
    // App CSP blocks frames; this WebKit API exposes only the main URI.
    matches!(uri.scheme(), "tauri" | "http" | "https")
        && uri.host_str().is_some()
        && uri.username().is_empty()
        && uri.password().is_none()
        && uri.scheme() == trusted.scheme()
        && uri.host_str() == trusted.host_str()
        && uri.port_or_known_default() == trusted.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::camera_request_allowed;

    #[test]
    fn camera_requires_the_configured_app_origin_and_video_only() {
        for origin in ["tauri://localhost", "http://localhost:1420"] {
            let trusted = tauri::Url::parse(origin).unwrap();
            let page = format!("{origin}/chat?preview=true#camera");
            assert!(camera_request_allowed(
                Some(&page),
                &trusted,
                true,
                false,
                false
            ));
            for (video, audio, display) in [
                (false, false, false),
                (false, true, false),
                (true, true, false),
                (true, false, true),
            ] {
                assert!(!camera_request_allowed(
                    Some(&page),
                    &trusted,
                    video,
                    audio,
                    display
                ));
            }
        }
    }

    #[test]
    fn camera_rejects_external_opaque_missing_and_credential_urls() {
        let trusted = tauri::Url::parse("tauri://localhost").unwrap();
        for uri in [
            None,
            Some(""),
            Some("not a URL"),
            Some("about:blank"),
            Some("data:text/html,camera"),
            Some("tauri:localhost"),
            Some("asset://localhost"),
            Some("tauri://localhost.attacker"),
            Some("tauri://user@localhost"),
            Some("tauri://localhost:1420"),
            Some("https://localhost"),
        ] {
            assert!(!camera_request_allowed(uri, &trusted, true, false, false));
        }
        let dev = tauri::Url::parse("http://localhost:1420").unwrap();
        for uri in [
            "http://localhost:1421",
            "http://localhost:1420.attacker",
            "http://127.0.0.1:1420",
            "https://localhost:1420",
            "http://user:pass@localhost:1420",
            "http://localhost.attacker:1420",
        ] {
            assert!(!camera_request_allowed(Some(uri), &dev, true, false, false));
        }
    }
}
