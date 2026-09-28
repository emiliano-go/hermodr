/// Any picture as a WhatsApp sticker: fitted into 512×512 on transparency, as
/// WebP. A WebP is sent unchanged, so an animated sticker stays animated.
pub(super) fn sticker_webp(bytes: &[u8]) -> Option<Vec<u8>> {
    if image::guess_format(bytes).ok()? == image::ImageFormat::WebP {
        return Some(bytes.to_vec());
    }
    let fitted = image::load_from_memory(bytes).ok()?.thumbnail(512, 512).to_rgba8();
    let mut canvas = image::RgbaImage::new(512, 512);
    let (x, y) = ((512 - fitted.width()) / 2, (512 - fitted.height()) / 2);
    image::imageops::overlay(&mut canvas, &fitted, x.into(), y.into());
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::WebP)
        .ok()?;
    Some(out)
}

/// A small JPEG preview for an outgoing attachment.
///
/// Images are downscaled locally. Video needs a decoder, so it is best effort:
/// Media Foundation on Windows, ffmpeg elsewhere when present, and `None`
/// means the file goes without a preview.
pub(super) fn media_thumbnail(kind: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    match kind {
        "image" => image_thumbnail(bytes),
        "video" | "gif" => video_thumbnail(bytes),
        _ => None,
    }
}

pub(super) fn image_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    image_thumbnail_reader(image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?)
}

pub(super) fn media_thumbnail_file(kind: &str, path: &std::path::Path) -> Option<Vec<u8>> {
    match kind {
        "image" => image_thumbnail_reader(image::ImageReader::open(path).ok()?.with_guessed_format().ok()?),
        "video" | "gif" => video_thumbnail_file(path),
        _ => None,
    }
}

fn image_thumbnail_reader<R: std::io::BufRead + std::io::Seek>(mut reader: image::ImageReader<R>) -> Option<Vec<u8>> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    jpeg_thumbnail(reader.decode().ok()?)
}

/// A centred square crop, at most `size` pixels a side, as JPEG.
pub(super) fn square_jpeg(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let side = image.width().min(image.height());
    let square = image
        .crop_imm((image.width() - side) / 2, (image.height() - side) / 2, side, side)
        .resize_exact(side.min(size), side.min(size), image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(square.to_rgb8())
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

fn jpeg_thumbnail(image: image::DynamicImage) -> Option<Vec<u8>> {
    let thumb = image.thumbnail(256, 256);
    let mut out = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

#[cfg(windows)]
fn video_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    video_thumbnail_source(VideoInput::Bytes(bytes))
}

#[cfg(windows)]
fn video_thumbnail_file(path: &std::path::Path) -> Option<Vec<u8>> {
    video_thumbnail_source(VideoInput::File(path))
}

#[cfg(windows)]
enum VideoInput<'a> { Bytes(&'a [u8]), File(&'a std::path::Path) }

#[cfg(windows)]
fn video_thumbnail_source(input: VideoInput<'_>) -> Option<Vec<u8>> {
    use windows::Win32::{
        Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_NOSOCKET, MF_VERSION},
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
    };

    // COM is initialised per thread, so decode on a thread of our own rather
    // than on a runtime worker.
    let frame = std::thread::scope(|scope| {
        scope
            .spawn(|| unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
                let frame = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).ok().and_then(|()| {
                    let frame = first_frame(input);
                    let _ = MFShutdown();
                    frame
                });
                CoUninitialize();
                frame
            })
            .join()
            .ok()
            .flatten()
    })?;
    jpeg_thumbnail(image::DynamicImage::ImageRgb8(frame))
}

/// The first decodable video frame, cropped to its visible area.
///
/// Must run between `MFStartup` and `MFShutdown` on a COM thread.
#[cfg(windows)]
unsafe fn first_frame(input: VideoInput<'_>) -> Option<image::RgbImage> {
    use windows::Win32::{Media::MediaFoundation::*, UI::Shell::SHCreateMemStream};

    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 1).ok()?;
    let attributes = attributes?;
    // Lets the reader convert whatever the decoder emits to RGB32.
    attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
    let reader = match input {
        VideoInput::Bytes(bytes) => {
            let stream = MFCreateMFByteStreamOnStream(&SHCreateMemStream(Some(bytes))?).ok()?;
            MFCreateSourceReaderFromByteStream(&stream, &attributes).ok()?
        }
        VideoInput::File(path) => {
            use std::os::windows::ffi::OsStrExt;
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let stream = MFCreateFile(MF_ACCESSMODE_READ, MF_OPENMODE_FAIL_IF_NOT_EXIST, MF_FILEFLAGS_NONE, windows::core::PCWSTR(path.as_ptr())).ok()?;
            MFCreateSourceReaderFromByteStream(&stream, &attributes).ok()?
        }
    };

    let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).ok()?;
    reader.SetStreamSelection(video, true).ok()?;
    let wanted = MFCreateMediaType().ok()?;
    wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
    wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
    reader.SetCurrentMediaType(video, None, &wanted).ok()?;

    let mut sample: Option<IMFSample> = None;
    for _ in 0..64 {
        let mut flags = 0u32;
        reader
            .ReadSample(video, 0, None, Some(&mut flags as *mut _), None, Some(&mut sample as *mut _))
            .ok()?;
        if sample.is_some() || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
    }
    let buffer = sample?.ConvertToContiguousBuffer().ok()?;

    // Read after the first sample: the decoder only settles the frame size then.
    let format = reader.GetCurrentMediaType(video).ok()?;
    let size = format.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
    let (width, height) = ((size >> 32) as u32, size as u32);
    let stride = format
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|s| s as i32)
        .unwrap_or(width as i32 * 4);
    // Decoders pad to whole macroblocks (1080 rows become 1088); the aperture is
    // the picture. MFVideoArea: two MFOffset { fract: u16, value: i16 }, then SIZE.
    let mut area = [0u8; 16];
    let (left, top, visible_w, visible_h) = format
        .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None)
        .ok()
        .map(|()| {
            (
                i16::from_le_bytes([area[2], area[3]]).max(0) as u32,
                i16::from_le_bytes([area[6], area[7]]).max(0) as u32,
                i32::from_le_bytes([area[8], area[9], area[10], area[11]]).max(0) as u32,
                i32::from_le_bytes([area[12], area[13], area[14], area[15]]).max(0) as u32,
            )
        })
        .filter(|&(x, y, w, h)| w > 0 && h > 0 && x + w <= width && y + h <= height)
        .unwrap_or((0, 0, width, height));

    let mut data: *mut u8 = std::ptr::null_mut();
    let mut len = 0u32;
    buffer.Lock(&mut data, None, Some(&mut len as *mut _)).ok()?;
    let pixels = std::slice::from_raw_parts(data, len as usize);
    let row = stride.unsigned_abs() as usize;
    let frame = (width > 0 && row >= width as usize * 4 && pixels.len() >= row * height as usize)
        .then(|| {
            image::RgbImage::from_fn(visible_w, visible_h, |x, y| {
                let y = top + y;
                // A negative stride means the rows are stored bottom-up.
                let y = (if stride < 0 { height - 1 - y } else { y }) as usize;
                let i = y * row + (left + x) as usize * 4;
                // RGB32 is BGRX in memory.
                image::Rgb([pixels[i + 2], pixels[i + 1], pixels[i]])
            })
        });
    let _ = buffer.Unlock();
    frame
}

#[cfg(not(windows))]
fn video_thumbnail_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"]).arg(path)
        .args(["-frames:v", "1", "-vf", "scale=256:256:force_original_aspect_ratio=decrease", "-f", "image2pipe", "-vcodec", "mjpeg", "pipe:1"])
        .output().ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}

#[cfg(not(windows))]
fn video_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("ffmpeg")
        .args([
            "-loglevel", "error",
            "-i", "pipe:0",
            "-frames:v", "1",
            "-vf", "scale=256:-2",
            "-f", "mjpeg",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Fed from another thread while stdout drains: ffmpeg stops reading after
    // the first frame, so a write can fail with a broken pipe after the frame
    // is out, or block while ffmpeg waits on a full stdout pipe.
    let mut stdin = child.stdin.take()?;
    let output = std::thread::scope(|scope| {
        scope.spawn(move || {
            let _ = stdin.write_all(bytes);
        });
        child.wait_with_output()
    })
    .ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}
