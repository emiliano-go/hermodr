use super::{media::MediaInput, media_files::TemporaryFile};
use crate::message_ref::MessageRef;
use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MediaQuality {
    Standard,
    #[default]
    Hd,
}

const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 512 * 1024 * 1024;
const VIDEO_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct PreparedMedia {
    pub(super) input: MediaInput,
    pub(super) file_name: String,
    _temporary: Vec<TemporaryFile>,
}

pub(super) async fn prepare(
    input: MediaInput,
    file_name: String,
    kind: &str,
    quality: Option<MediaQuality>,
    gif: bool,
) -> Result<PreparedMedia> {
    if quality.is_none() {
        return Ok(PreparedMedia {
            input,
            file_name,
            _temporary: Vec::new(),
        });
    }
    ensure!(
        matches!(kind, "image" | "video")
            && !gif
            && !file_name.to_ascii_lowercase().ends_with(".gif"),
        MessageRef::new("error.media_quality_type_invalid")
    );
    if quality == Some(MediaQuality::Hd) {
        return Ok(PreparedMedia {
            input,
            file_name,
            _temporary: Vec::new(),
        });
    }
    let video = kind == "video";
    tokio::task::spawn_blocking(move || {
        if video {
            standard_video(input, file_name)
        } else {
            let bytes = read_image(input)?;
            let (bytes, extension) = standard_photo(&bytes)?;
            Ok(PreparedMedia {
                input: MediaInput::Bytes(bytes),
                file_name: renamed(&file_name, extension),
                _temporary: Vec::new(),
            })
        }
    })
    .await?
}

fn renamed(file_name: &str, extension: &str) -> String {
    Path::new(file_name)
        .with_extension(extension)
        .to_string_lossy()
        .into_owned()
}

fn read_image(input: MediaInput) -> Result<Vec<u8>> {
    let bytes = match input {
        MediaInput::Bytes(bytes) => bytes,
        MediaInput::File(path) => {
            let mut bytes = Vec::new();
            File::open(path)?
                .take(MAX_IMAGE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            bytes
        }
    };
    ensure!(
        bytes.len() as u64 <= MAX_IMAGE_BYTES,
        MessageRef::new("error.media_photo_size_limit").with_param("max_bytes", serde_json::Number::from(MAX_IMAGE_BYTES))
    );
    Ok(bytes)
}

fn standard_photo(bytes: &[u8]) -> Result<(Vec<u8>, &'static str)> {
    ensure!(
        bytes.len() as u64 <= MAX_IMAGE_BYTES,
        MessageRef::new("error.media_photo_size_limit").with_param("max_bytes", serde_json::Number::from(MAX_IMAGE_BYTES))
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut decoder: Box<dyn ImageDecoder + '_> = match image::guess_format(bytes)? {
        ImageFormat::Jpeg => Box::new(image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))?),
        ImageFormat::Png => {
            let decoder =
                image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits.clone())?;
            ensure!(
                !decoder.is_apng()?,
                MessageRef::new("error.media_quality_animated_photo")
            );
            Box::new(decoder)
        }
        ImageFormat::WebP => {
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
            ensure!(
                !decoder.has_animation(),
                MessageRef::new("error.media_quality_animated_photo")
            );
            Box::new(decoder)
        }
        _ => bail!(MessageRef::new("error.media_quality_photo_format")),
    };
    decoder.set_limits(limits)?;
    ensure!(
        decoder.total_bytes() <= 128 * 1024 * 1024,
        MessageRef::new("error.media_photo_decoded_limit").with_param("max_bytes", serde_json::Number::from(128 * 1024 * 1024))
    );
    let orientation = decoder.orientation()?;
    let icc = decoder.icc_profile()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if image.width() > 1600 || image.height() > 1600 {
        image = image.resize(1600, 1600, FilterType::Lanczos3);
    }
    let mut bytes = Vec::new();
    if image.color().has_alpha() {
        let pixels = image.to_rgba8();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        if let Some(icc) = icc {
            encoder.set_icc_profile(icc)?;
        }
        encoder.write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        Ok((bytes, "png"))
    } else {
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80);
        if let Some(icc) = icc {
            encoder.set_icc_profile(icc)?;
        }
        encoder.encode_image(&image.to_rgb8())?;
        Ok((bytes, "jpg"))
    }
}

fn standard_video(input: MediaInput, file_name: String) -> Result<PreparedMedia> {
    let mut temporary = Vec::new();
    let source = match input {
        MediaInput::File(path) => {
            ensure!(
                std::fs::metadata(&path)?.len() <= MAX_VIDEO_BYTES,
                MessageRef::new("error.media_video_size_limit").with_param("max_bytes", serde_json::Number::from(MAX_VIDEO_BYTES))
            );
            path
        }
        MediaInput::Bytes(bytes) => {
            ensure!(
                bytes.len() as u64 <= MAX_VIDEO_BYTES,
                MessageRef::new("error.media_video_size_limit").with_param("max_bytes", serde_json::Number::from(MAX_VIDEO_BYTES))
            );
            let (owned, mut file) = TemporaryFile::download(&std::env::temp_dir())?;
            file.write_all(&bytes)?;
            drop(file);
            let path = owned.path.clone();
            temporary.push(owned);
            path
        }
    };
    let (output, file) = TemporaryFile::download(&std::env::temp_dir())?;
    drop(file);
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-nostdin", "-loglevel", "error", "-i"])
        .arg(&source)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-vf",
            r"scale=iw:min(480\,ih):force_original_aspect_ratio=decrease:force_divisible_by=2",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "2",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(&output.path);
    transcode(&mut command, &output.path, VIDEO_TIMEOUT, MAX_VIDEO_BYTES)?;
    let path = output.path.clone();
    temporary.push(output);
    Ok(PreparedMedia {
        input: MediaInput::File(path),
        file_name: renamed(&file_name, "mp4"),
        _temporary: temporary,
    })
}

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

struct RunningTranscode(Child);

impl Drop for RunningTranscode {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn transcode(command: &mut Command, output: &Path, timeout: Duration, limit: u64) -> Result<()> {
    let mut process =
        RunningTranscode(command.spawn().context(MessageRef::new("error.media_video_converter_unavailable"))?);
    let started = Instant::now();
    loop {
        ensure!(
            std::fs::metadata(output)?.len() <= limit,
            MessageRef::new("error.media_video_output_limit").with_param("max_bytes", serde_json::Number::from(limit))
        );
        if let Some(status) = process.0.try_wait()? {
            ensure!(
                status.success(),
                MessageRef::new("error.media_video_conversion_failed").with_param("exit_code", serde_json::Number::from(status.code().unwrap_or(-1)))
            );
            let length = std::fs::metadata(output)?.len();
            ensure!(
                length <= limit,
                MessageRef::new("error.media_video_output_limit").with_param("max_bytes", serde_json::Number::from(limit))
            );
            ensure!(
                length > 0,
                MessageRef::new("error.media_video_output_empty")
            );
            return Ok(());
        }
        ensure!(
            started.elapsed() < timeout,
            MessageRef::new("error.media_video_conversion_timeout").with_param("seconds", serde_json::Number::from(timeout.as_secs()))
        );
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[cfg(test)]
#[path = "media_quality_tests.rs"]
mod tests;
