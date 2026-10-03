use crate::command_error::{CommandError, CommandResult};
use crate::{
    account_store::{data_dir, now_millis},
    desktop::shell_open,
};
use std::{
    io::Write as _,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

const LOG_LIMIT: u64 = 5 << 20;
static ROTATION_WARNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Copies log output to stderr and to the current log file.
struct Tee(PathBuf);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Err(error) = append_log(&self.0, buf) {
            eprintln!(
                "could not write {}; logging to stderr only: {error}",
                self.0.display()
            );
            return Err(error);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

/// Where [`init_logging`] writes.
pub(crate) fn log_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("postal.log")
}

fn append_log(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let len = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if len > 0 && len.saturating_add(bytes.len() as u64) > LOG_LIMIT {
        let old = path.with_extension("log.old");
        if let Err(error) = std::fs::rename(path, old) {
            if !ROTATION_WARNING.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!(
                    "could not rotate {}; continuing to write current log: {error}",
                    path.display()
                );
            }
        } else {
            ROTATION_WARNING.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(bytes)
}

fn filter(verbose_whatsapp: bool) -> String {
    let ours = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    let transport = if verbose_whatsapp {
        ",Client/Keepalive=debug,whatsapp_rust::client::node_io=debug"
    } else {
        ""
    };
    format!(
        "warn,postal_lib={ours},postal_core={ours},ui={ours},\
         whatsapp_rust::history_sync=info,whatsapp_rust::pdo=info{transport}"
    )
}

fn configure_builder(builder: &mut env_logger::Builder, path: &Path) {
    builder.format_timestamp_millis();
    builder.target(env_logger::Target::Pipe(Box::new(Tee(path.to_path_buf()))));
}

/// Installs logging to `<app data>/postal.log` and stderr. Rotation keeps the
/// previous 5 MB segment in `postal.log.old`.
pub(crate) fn init_logging(path: &Path, verbose_whatsapp: bool) -> std::io::Result<()> {
    let boot = format!(
        "Postal {} on {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let opened = (|| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        append_log(path, boot.as_bytes())
    })();

    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(filter(verbose_whatsapp)),
    );
    if opened.is_ok() {
        configure_builder(&mut builder, path);
    }
    builder.init();
    if let Err(error) = opened {
        eprintln!(
            "could not write {}; logging to stderr only: {error}",
            path.display()
        );
        return Err(error);
    }

    let panic_path = path.to_path_buf();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let panic = format!(
            "[{} ms] panic in thread '{}': {info}\n{}\n",
            now_millis(),
            thread.name().unwrap_or("<unnamed>"),
            std::backtrace::Backtrace::force_capture(),
        );
        if let Err(error) = append_log(&panic_path, panic.as_bytes()) {
            eprintln!("could not write {}: {error}", panic_path.display());
        }
        default(info);
    }));
    log::info!("logging to {}", path.display());
    Ok(())
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
pub(crate) fn open_log(app: AppHandle) -> CommandResult<()> {
    shell_open(log_path(&app).as_os_str()).map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::{append_log, configure_builder, filter, Tee, LOG_LIMIT};
    use log::{Level, Log, Record};
    use std::{
        io::Write as _,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "postal-logging-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn boot_and_filtered_logger_target_write_after_rotation() {
        let dir = temp_dir();
        let path = dir.join("postal.log");
        std::fs::write(&path, vec![b'x'; LOG_LIMIT as usize]).unwrap();
        append_log(&path, b"Postal boot\n").unwrap();

        let mut builder = env_logger::Builder::new();
        builder.parse_filters(&filter(false));
        configure_builder(&mut builder, &path);
        let logger = builder.build();
        for target in ["postal_lib::logging", "postal_core::service::connection"] {
            let record = Record::builder()
                .args(format_args!("logger probe"))
                .level(Level::Info)
                .target(target)
                .build();
            assert!(logger.enabled(&record.metadata()));
            logger.log(&record);
        }

        assert_eq!(
            std::fs::metadata(dir.join("postal.log.old")).unwrap().len(),
            LOG_LIMIT
        );
        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("Postal boot"));
        assert!(log.contains("logger probe"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unwritable_path_returns_an_error() {
        let dir = temp_dir();
        assert!(append_log(&dir, b"Postal boot\n").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_rotation_preserves_old_data_and_appends_to_current_log() {
        let dir = temp_dir();
        let path = dir.join("postal.log");
        std::fs::write(&path, vec![b'x'; LOG_LIMIT as usize]).unwrap();
        let old = path.with_extension("log.old");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("keep"), b"previous dump").unwrap();

        append_log(&path, b"current run\n").unwrap();

        assert_eq!(std::fs::read(old.join("keep")).unwrap(), b"previous dump");
        assert!(std::fs::read(&path).unwrap().ends_with(b"current run\n"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeated_rotation_replaces_the_previous_dump() {
        let dir = temp_dir();
        let path = dir.join("postal.log");
        std::fs::write(&path, vec![b'a'; LOG_LIMIT as usize]).unwrap();
        append_log(&path, b"first run\n").unwrap();
        std::fs::write(&path, vec![b'b'; LOG_LIMIT as usize]).unwrap();

        append_log(&path, b"second run\n").unwrap();

        let old = std::fs::read(path.with_extension("log.old")).unwrap();
        assert_eq!(old.len(), LOG_LIMIT as usize);
        assert!(old.iter().all(|byte| *byte == b'b'));
        assert_eq!(std::fs::read(&path).unwrap(), b"second run\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tee_reopens_the_log_path_after_rotation() {
        let dir = temp_dir();
        let path = dir.join("postal.log");
        std::fs::write(&path, vec![b'x'; LOG_LIMIT as usize]).unwrap();
        Tee(path.clone()).write_all(b"current run\n").unwrap();
        assert!(std::fs::read_to_string(&path.with_extension("log.old")).is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "current run\n");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
