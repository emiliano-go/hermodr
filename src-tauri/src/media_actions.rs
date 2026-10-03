use std::{io::{BufRead, Cursor, Read, Seek}, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
use serde::Deserialize;
use tauri::{State, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use crate::{AppState, desktop::shell_open};
use crate::command_error::{CommandError, CommandResult};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) enum MediaAction { CopyImage, Save, Open }

#[tauri::command]
pub(crate) async fn message_media_action(
    window: WebviewWindow, state: State<'_, AppState>, chat: String, id: String, action: MediaAction,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let message = service.media_for_export(&chat, &id).await?;
    let directory = service.media_dir().ok_or_else(|| CommandError::code("error.media_directory_missing"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let source = export_path(&directory, message.media.path.as_deref().ok_or_else(|| CommandError::code("error.media_download_missing"))?)?;
        match action {
            MediaAction::CopyImage => {
                if !matches!(message.media.kind.as_deref(), Some("image" | "sticker")) {
                    return Err(CommandError::code("error.media_clipboard_image_only"));
                }
                let image = clipboard_image(&source)?;
                window.clipboard().write_image(&image).map_err(CommandError::operation_failed)
            }
            MediaAction::Save => {
                let name = source.file_name().ok_or_else(|| CommandError::code("error.media_file_name_missing"))?.to_string_lossy();
                let selected = window.dialog().file().set_parent(&window)
                    .set_file_name(name.as_ref()).blocking_save_file();
                if let Some(selected) = selected {
                    let destination = selected.into_path().map_err(|e| e.to_string())?;
                    if dunce::canonicalize(&destination).ok().as_ref() != Some(&source) {
                        std::fs::copy(&source, destination).map_err(|e| e.to_string())?;
                    }
                }
                Ok(())
            }
            MediaAction::Open => shell_open(source.as_os_str()).map_err(CommandError::from),
        }
    }).await.map_err(|e| e.to_string())?
}

fn export_path(directory: &Path, source: &str) -> CommandResult<PathBuf> {
    let directory = dunce::canonicalize(directory).map_err(|e| e.to_string())?;
    let source = dunce::canonicalize(source).map_err(|e| e.to_string())?;
    if !source.starts_with(directory) || !source.is_file() {
        return Err(CommandError::code("error.media_path_denied"));
    }
    Ok(source)
}

fn clipboard_image(path: &Path) -> CommandResult<tauri::image::Image<'static>> {
    let rgba = decode_first_frame(path)?.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(tauri::image::Image::new_owned(rgba.into_raw(), width, height))
}

/// A still image from `path`: the `image` crate first, and for an animated WebP
/// it may refuse, the first frame decoded by ffmpeg.
fn decode_first_frame(path: &Path) -> CommandResult<image::DynamicImage> {
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?
        .with_guessed_format().map_err(|e| e.to_string())?;
    if let Ok(decoded) = decode_limited(reader) {
        return Ok(decoded);
    }
    let frame = first_frame(path)?;
    decode_limited(image::ImageReader::with_format(Cursor::new(frame), image::ImageFormat::Png))
        .map_err(CommandError::operation_failed)
}

fn decode_limited<R: BufRead + Seek>(mut reader: image::ImageReader<R>) -> image::ImageResult<image::DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode()
}

/// The first frame of a video or animated WebP as PNG, via ffmpeg when present.
fn first_frame(path: &Path) -> CommandResult<Vec<u8>> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "pipe:1"]);
    run_first_frame(command, Duration::from_secs(10), 16 << 20)
}

fn run_first_frame(mut command: Command, timeout: Duration, max_bytes: usize) -> CommandResult<Vec<u8>> {
    struct Reap(std::process::Child, bool);
    impl Drop for Reap {
        fn drop(&mut self) {
            if !self.1 { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
    }
    let error = |reason| CommandError::code("error.media_image_decode_failed").with_diagnostic(reason);
    let mut child = Reap(command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .map_err(|err| error(format!("ffmpeg could not start: {err}")))?, false);
    let deadline = Instant::now() + timeout;
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("ffmpeg-frame-reader".into()).spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(max_bytes.saturating_add(1) as u64).read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    }).map_err(|err| error(format!("ffmpeg reader could not start: {err}")))?;
    let mut frame = None;
    loop {
        match receive.try_recv() {
            Ok(result) => frame = Some(result),
            Err(std::sync::mpsc::TryRecvError::Disconnected) if frame.is_none() => return Err(error("ffmpeg reader stopped".into())),
            _ => {}
        }
        if frame.as_ref().is_some_and(|result: &std::io::Result<Vec<u8>>| result.as_ref().is_ok_and(|bytes| bytes.len() > max_bytes)) {
            return Err(error(format!("ffmpeg first frame exceeded {max_bytes} bytes")));
        }
        if let Some(status) = child.0.try_wait().map_err(|err| error(format!("ffmpeg wait failed: {err}")))? {
            child.1 = true;
            let bytes = match frame {
                Some(result) => result,
                None => receive.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| error("ffmpeg stdout did not finish within the timeout".into()))?,
            }.map_err(|err| error(format!("ffmpeg stdout read failed: {err}")))?;
            if bytes.len() > max_bytes { return Err(error(format!("ffmpeg first frame exceeded {max_bytes} bytes"))); }
            if !status.success() || bytes.is_empty() { return Err(error(format!("ffmpeg returned {status} without a frame"))); }
            return Ok(bytes);
        }
        if Instant::now() >= deadline { return Err(error(format!("ffmpeg first-frame export exceeded {} ms", timeout.as_millis()))); }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_fallback_frame_obeys_pixel_limits() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8193, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        assert!(png.get_ref().len() < 16 << 20);
        png.set_position(0);
        assert!(decode_limited(image::ImageReader::with_format(png, image::ImageFormat::Png)).is_err());
    }

    #[test]
    fn fake_frame_process_times_out_and_caps_stdout() {
        #[cfg(windows)]
        let hanging = {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            command
        };
        #[cfg(unix)]
        let hanging = {
            let mut command = Command::new("sh");
            command.args(["-c", "exec sleep 30"]);
            command
        };
        let started = Instant::now();
        let failure = run_first_frame(hanging, Duration::from_millis(250), 1024).unwrap_err();
        assert_eq!(failure.message.code, "error.media_image_decode_failed");
        assert!(failure.diagnostic.unwrap().contains("exceeded 250 ms"));
        assert!(started.elapsed() < Duration::from_secs(5));

        #[cfg(windows)]
        let overflowing = {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "[Console]::OpenStandardOutput().Write((New-Object byte[] 2048),0,2048)"]);
            command
        };
        #[cfg(unix)]
        let overflowing = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf '%2048s' x"]);
            command
        };
        let failure = run_first_frame(overflowing, Duration::from_secs(10), 1024).unwrap_err();
        assert_eq!(failure.message.code, "error.media_image_decode_failed");
        assert!(failure.diagnostic.unwrap().contains("exceeded 1024 bytes"));
    }

    #[test]
    fn exports_stay_in_media_folder_and_copy_decodes_pixels() {
        let root = std::env::temp_dir().join(format!("postal-export-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let media = root.join("media café 📨");
        std::fs::create_dir_all(&media).unwrap();
        let source = media.join("写真 e\u{301}.png");
        image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255])).save(&source).unwrap();
        let image = clipboard_image(&source).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.rgba(), &[12, 34, 56, 255, 12, 34, 56, 255]);
        assert_eq!(export_path(&media, source.to_str().unwrap()).unwrap(), dunce::canonicalize(&source).unwrap());
        assert_eq!(export_path(&media, source.canonicalize().unwrap().to_str().unwrap()).unwrap(), dunce::canonicalize(&source).unwrap());
        let outside = root.join("outside.png");
        std::fs::copy(&source, &outside).unwrap();
        assert!(export_path(&media, outside.to_str().unwrap()).is_err());
        assert!(export_path(&media, media.join("..").join("outside.png").to_str().unwrap()).is_err());
        let sibling = root.join("media café 📨-outside");
        std::fs::create_dir(&sibling).unwrap();
        std::fs::copy(&source, sibling.join("image.png")).unwrap();
        assert!(export_path(&media, sibling.join("image.png").to_str().unwrap()).is_err());
        #[cfg(unix)]
        {
            let escape = media.join("escape.png");
            std::os::unix::fs::symlink(&outside, &escape).unwrap();
            assert!(export_path(&media, escape.to_str().unwrap()).is_err());
            let inside = media.join("inside.png");
            std::os::unix::fs::symlink(&source, &inside).unwrap();
            assert_eq!(export_path(&media, inside.to_str().unwrap()).unwrap(), dunce::canonicalize(&source).unwrap());
        }
        assert!(export_path(&media, media.to_str().unwrap()).is_err());
        assert!(export_path(&media, media.join("missing.png").to_str().unwrap()).is_err());
        std::fs::write(&source, b"not an image").unwrap();
        assert!(clipboard_image(&source).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
