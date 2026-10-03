use std::path::{Path, PathBuf};
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
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?
        .with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    if let Ok(decoded) = reader.decode() {
        return Ok(decoded);
    }
    let frame = first_frame(path).ok_or_else(|| CommandError::code("error.media_image_decode_failed"))?;
    image::load_from_memory(&frame).map_err(CommandError::operation_failed)
}

/// The first frame of a video or animated WebP as PNG, via ffmpeg when present.
fn first_frame(path: &Path) -> Option<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .output()
        .ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

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
