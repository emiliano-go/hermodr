use std::{io::Write as _, path::PathBuf};
use tauri::AppHandle;
use crate::{account_store::{data_dir, now_millis}, desktop::shell_open};

/// Copies log output to stderr and to the log file. The file is unbuffered so
/// a line written before an abort is on disk.
pub(crate) struct Tee(std::fs::File);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Where [`init_logging`] writes.
pub(crate) fn log_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("postal.log")
}

/// Sends logs and panics to `<app data>/postal.log` as well as stderr, which
/// is discarded when the app is launched from a desktop entry. Past 5 MB the
/// file moves to `postal.log.old`, so the run before a crash is still there.
pub(crate) fn init_logging(path: &std::path::Path, verbose_whatsapp: bool) {
    // Our crates and the UI (`ui`) log at info, debug in dev builds; history
    // sync and peer requests fail silently otherwise. RUST_LOG overrides.
    let ours = if cfg!(debug_assertions) { "debug" } else { "info" };
    // The keepalive and transport targets explain why a link stalls; on by
    // default, and switchable off from Settings → Advanced for a quieter log.
    let transport = if verbose_whatsapp {
        ",Client/Keepalive=debug,whatsapp_rust::client::node_io=debug"
    } else {
        ""
    };
    let filter = format!(
        "warn,postal_lib={ours},postal_core={ours},ui={ours},\
         whatsapp_rust::history_sync=info,whatsapp_rust::pdo=info{transport}"
    );
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter));
    builder.format_timestamp_millis();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 5 << 20) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    let opened = file.is_ok();
    if let Ok(file) = file {
        if let Ok(panics) = file.try_clone() {
            let default = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let thread = std::thread::current();
                let _ = writeln!(
                    &panics,
                    "[{} ms] panic in thread '{}': {info}\n{}",
                    now_millis(),
                    thread.name().unwrap_or("<unnamed>"),
                    std::backtrace::Backtrace::force_capture(),
                );
                default(info);
            }));
        }
        builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
    }
    builder.init();
    log::info!(
        "Postal {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if opened {
        log::info!("logging to {}", path.display());
    } else {
        log::warn!("could not open {}; logging to stderr only", path.display());
    }
}

/// Writes a line from the UI into the log under the `ui` target.
#[tauri::command(async)]
pub(crate) fn frontend_log(level: String, message: String) {
    let level = match level.as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "debug" => log::Level::Debug,
        _ => log::Level::Info,
    };
    log::log!(target: "ui", level, "{message}");
}

/// Opens the log file with the desktop's default application.
#[tauri::command(async)]
pub(crate) fn open_log(app: AppHandle) -> Result<(), String> {
    shell_open(log_path(&app).as_os_str())
}
