//! Media: sending and downloading attachments, stickers, and their thumbnails.

use super::*;
use whatsapp_rust::media::{self, AudioOptions, DocumentOptions, ImageOptions, VideoOptions};

/// Encrypted media handed to the uploader, reporting how far it has been read.
struct ProgressSource {
    data: Arc<[u8]>,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

impl whatsapp_rust::wacore::upload::UploadSource for ProgressSource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn reader_from(&self, offset: u64) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let mut cursor = std::io::Cursor::new(Arc::clone(&self.data));
        cursor.set_position(offset.min(self.len()));
        Ok(Box::new(CountingReader { inner: cursor, read: offset, report: Arc::clone(&self.report) }))
    }
}

struct CountingReader {
    inner: std::io::Cursor<Arc<[u8]>>,
    read: u64,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.report)(self.read);
        Ok(n)
    }
}

impl Service {
    /// Downloads a message's media on demand, when automatic downloads were
    /// off or the earlier attempt failed.
    pub async fn download_media(&self, chat: &str, id: &str) -> Result<()> {
        let dir = self
            .media_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no media folder configured"))?;
        let updated = fetch_media(&self.client, &self.store, &dir, chat, id).await?;
        let _ = self.events.send(ServiceEvent::hint(&updated, false));
        Ok(())
    }

    /// Deletes downloaded media and forgets the paths, keeping the messages.
    pub fn flush_media(&self) -> Result<usize> {
        let cleared = self.store.clear_media_paths()?;
        if let Some(dir) = &self.media_dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let _ = std::fs::remove_dir_all(&path);
                    } else {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
        Ok(cleared)
    }

    /// Where media is stored, if enabled.
    pub fn media_dir(&self) -> Option<PathBuf> {
        self.media_dir.clone()
    }

    /// Sends a file as an image or document, chosen from its extension.
    ///
    /// Images are sent as images so they render inline; everything else goes as
    /// a document, which is what a file picker is usually for.
    pub async fn send_media(
        &self,
        chat: &str,
        file_name: &str,
        bytes: Vec<u8>,
        caption: Option<String>,
        reply: Option<(String, String, String)>,
        options: SendOptions,
    ) -> Result<Option<String>> {
        let SendOptions { gif, view_once, voice, forwarded, mentions, progress } = options;
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let file_name = file_name.to_string();
        let extension = std::path::Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        // The extension decides how the receiver renders the file, so a video
        // only arrives as a video (not a document) if it is sent as one.
        let (media_type, kind) = match extension.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" => (MediaType::Image, "image"),
            "mp4" | "mov" | "m4v" | "webm" | "mkv" => (MediaType::Video, "video"),
            "ogg" | "opus" | "mp3" | "m4a" | "aac" | "wav" => (MediaType::Audio, "audio"),
            _ => (MediaType::Document, "document"),
        };

        let upload = match progress {
            Some(token) => self.upload_reporting(bytes.clone(), media_type, token).await?,
            None => self.client.upload(bytes.clone(), media_type, Default::default()).await?,
        };

        let mimetype = mime_for(&extension).map(str::to_string);

        // A thumbnail lets the recipient see a preview before the file lands.
        let thumb = media_thumbnail(kind, &bytes);
        let warning = (kind == "video" || kind == "gif")
            .then(|| thumb.is_none())
            .filter(|missing| *missing)
            .map(|_| {
                if cfg!(windows) {
                    "The video was sent without a preview because Windows could not decode it."
                } else {
                    "The video was sent without a preview because ffmpeg is not installed."
                }
                .to_string()
            });

        // An attachment can carry a quote, the same as a text reply.
        let context = match &reply {
            Some((id, sender, text)) => {
                use whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info;
                let sender: Jid = sender.parse::<Jid>()?.to_non_ad();
                let quoted = wa::Message::text(text.clone());
                Some(Box::new(build_quote_context_with_info(
                    id, &sender, &to, &to, &quoted,
                )))
            }
            None => None,
        };
        let context = if forwarded { Some(forwarded_context(context)) } else { context };
        let context = if mentions.is_empty() {
            context
        } else {
            let mut context = context.unwrap_or_default();
            context.mentioned_jid = mentions;
            Some(context)
        };

        let voice_seconds = voice.as_ref().map(|v| v.seconds);
        let mut message = match kind {
            "image" => media::image_message(
                upload,
                ImageOptions {
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    context_info: context,
                    ..Default::default()
                },
            ),
            "video" => media::video_message(
                upload,
                VideoOptions {
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    gif_playback: gif.then_some(true),
                    context_info: context,
                    ..Default::default()
                },
            ),
            "audio" => media::audio_message(
                upload,
                AudioOptions {
                    mimetype: if voice.is_some() { Some("audio/ogg; codecs=opus".into()) } else { mimetype },
                    // An ogg/opus attachment is a voice note, which is how
                    // WhatsApp records and replays them.
                    ptt: Some(extension == "ogg"),
                    duration_seconds: voice_seconds,
                    waveform: voice.map(|v| v.waveform),
                    context_info: context,
                },
            ),
            _ => media::document_message(
                upload,
                DocumentOptions {
                    file_name: Some(file_name.clone()),
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    context_info: context,
                    ..Default::default()
                },
            ),
        };
        if view_once {
            // Documents have no view-once form, so a file the UI staged as a
            // photo but the extension sends as a document fails loudly instead
            // of going out as an ordinary attachment.
            if kind == "document" {
                anyhow::bail!("view-once only works for photos, videos and voice notes");
            }
            if let Some(m) = message.image_message.as_option_mut() {
                m.view_once = Some(true);
            }
            if let Some(m) = message.video_message.as_option_mut() {
                m.view_once = Some(true);
            }
            if let Some(m) = message.audio_message.as_option_mut() {
                m.view_once = Some(true);
            }
            // WhatsApp renders one-time media from the V2 container; the inline
            // flags above are hints kept for clients that read them.
            message = wrap_view_once(message);
        }

        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id)?;
        }
        // The sender cannot reopen view-once media either, so no copy is kept.
        if view_once {
            self.store.set_view_once(chat, &result.message_id, true)?;
        }

        // Keep our own copy so the sender sees what they sent.
        let mut stored_path = None;
        if let Some(dir) = self.media_dir().filter(|_| !view_once) {
            let dir = &dir;
            if std::fs::create_dir_all(dir).is_ok() {
                let name = if extension.is_empty() { "bin".to_string() } else { extension.clone() };
                let dest = dir.join(format!("{}.{}", result.message_id, name));
                if std::fs::write(&dest, &bytes).is_ok() {
                    stored_path = Some(dest.to_string_lossy().to_string());
                }
            }
        }

        let kind = if gif && kind == "video" { "gif" } else { kind };
        let mut stored = self.own_message(chat, &result.message_id, caption.unwrap_or_default(), kind, to_self);
        stored.media.path = stored_path;
        stored.media.duration = voice_seconds;
        if let Some((id, sender, text)) = reply {
            stored.quote = Quote { id: Some(id), text: Some(text), sender: Some(sender), ..Default::default() };
        }
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
        Ok(warning)
    }

    /// Uploads media while reporting its progress as [`ServiceEvent::UploadProgress`], about once per percent.
    async fn upload_reporting(
        &self,
        bytes: Vec<u8>,
        media_type: MediaType,
        token: String,
    ) -> Result<whatsapp_rust::upload::UploadResponse> {
        use whatsapp_rust::wacore::upload::{encrypt_media_with_key_and_sidecar, EncryptedMediaInfo};
        let file_length = bytes.len() as u64;
        let enc = tokio::task::spawn_blocking(move || {
            encrypt_media_with_key_and_sidecar(&bytes, media_type, None, None)
        })
        .await??;
        let total = enc.data_to_upload.len() as u64;
        let events = self.events.clone();
        let last = Arc::new(AtomicU64::new(u64::MAX));
        let report: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |sent: u64| {
            let percent = sent.saturating_mul(100) / total.max(1);
            if last.swap(percent, Ordering::Relaxed) != percent {
                let _ = events.send(ServiceEvent::UploadProgress { token: token.clone(), sent, total });
            }
        });
        let source = ProgressSource { data: Arc::from(enc.data_to_upload), report };
        let info = EncryptedMediaInfo {
            media_key: enc.media_key,
            file_sha256: enc.file_sha256,
            file_enc_sha256: enc.file_enc_sha256,
            file_length,
            streaming_sidecar: enc.streaming_sidecar,
        };
        Ok(self.client.upload_stream(source, info, media_type).await?)
    }

    /// Sends a picture as a sticker (see [`sticker_webp`]).
    pub async fn send_sticker(&self, chat: &str, bytes: Vec<u8>) -> Result<()> {
        self.send_sticker_as(chat, bytes, false).await
    }

    /// Turns a picture into a sticker in the media folder without sending it.
    pub fn save_sticker(&self, bytes: &[u8]) -> Result<String> {
        let webp = sticker_webp(bytes)
            .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
        let dir = self
            .media_dir()
            .ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?
            .join("stickers");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("saved-{}.webp", unix_now()));
        std::fs::write(&path, webp)?;
        Ok(path.to_string_lossy().into_owned())
    }

    pub(super) async fn send_sticker_as(&self, chat: &str, bytes: Vec<u8>, forwarded: bool) -> Result<()> {
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let webp = sticker_webp(&bytes)
            .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
        let upload = self
            .client
            .upload(webp.clone(), MediaType::Sticker, Default::default())
            .await?;
        let message = wa::Message {
            sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key.to_vec()),
                file_sha256: Some(upload.file_sha256.to_vec()),
                file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                file_length: Some(upload.file_length),
                media_key_timestamp: Some(upload.media_key_timestamp),
                mimetype: Some("image/webp".into()),
                width: Some(512),
                height: Some(512),
                context_info: if forwarded {
                    MessageField::some(*forwarded_context(None))
                } else {
                    MessageField::none()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id)?;
        }

        let media_path = self.media_dir().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            let dest = dir.join(format!("{}.webp", result.message_id));
            std::fs::write(&dest, &webp).ok()?;
            Some(dest.to_string_lossy().into_owned())
        });
        let mut stored = self.own_message(chat, &result.message_id, "[sticker]".into(), "sticker", to_self);
        stored.media.path = media_path;
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
        Ok(())
    }

    /// Stickers or GIFs already on this device, newest first, for the picker.
    /// Recent stickers or GIFs, newest first, one per distinct file. Copies of a
    /// path in `prefer` (the favourites) are dropped in its favour.
    pub fn media_library(&self, kind: &str, prefer: &[String]) -> Result<Vec<String>> {
        use std::hash::{Hash, Hasher};
        use std::io::Read;
        let recent = self.store.recent_media(kind, 200)?;
        let dir = self.media_dir().and_then(|d| std::fs::canonicalize(d).ok());
        // Favourites come from the UI, so only files in the media folder count.
        let preferred = prefer.iter().filter(|p| {
            dir.as_ref()
                .is_some_and(|d| std::fs::canonicalize(p).is_ok_and(|f| f.starts_with(d)))
        });
        // The same sticker sent or received again is a new file; show it once.
        // Length plus the first 64 KiB identifies it without reading whole videos.
        let mut seen = std::collections::HashSet::new();
        let mut listed = std::collections::HashSet::new();
        Ok(preferred
            .cloned()
            .chain(recent)
            .filter(|p| listed.insert(p.clone()))
            .filter(|p| {
                let Ok(file) = std::fs::File::open(p) else { return false };
                let length = file.metadata().map(|m| m.len()).unwrap_or(0);
                let mut head = Vec::new();
                if file.take(64 * 1024).read_to_end(&mut head).is_err() {
                    return false;
                }
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (length, head).hash(&mut hasher);
                seen.insert(hasher.finish())
            })
            .collect())
    }

    /// Re-sends a sticker or GIF from the media folder.
    pub async fn send_from_library(&self, chat: &str, path: &str, kind: &str) -> Result<()> {
        let dir = self.media_dir().ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let file = std::fs::canonicalize(path)?;
        if !file.starts_with(std::fs::canonicalize(&dir)?) {
            anyhow::bail!("refusing to send a file from outside the media folder");
        }
        let bytes = std::fs::read(&file)?;
        match kind {
            "sticker" => self.send_sticker(chat, bytes).await,
            "gif" => {
                let options = SendOptions { gif: true, ..Default::default() };
                self.send_media(chat, "gif.mp4", bytes, None, None, options).await.map(|_| ())
            }
            _ => anyhow::bail!("only stickers and GIFs are sent from the library"),
        }
    }
}

/// The media carried by a message, if any.
pub(super) struct MediaInfo {
    /// `image`, `video`, `gif`, `audio` or `document`. A video sent with
    /// `gifPlayback` is a GIF, which WhatsApp keeps as a muted looping clip.
    pub(super) kind: &'static str,
    media_type: MediaType,
    /// The four media protos are distinct types that each implement
    /// `Downloadable`.
    pub(super) downloadable: Box<dyn Downloadable + Send + Sync>,
    /// The small JPEG the message carries, available without downloading.
    pub(super) thumb: Option<Vec<u8>>,
    /// Audio length in seconds, when the message carries it.
    pub(super) duration: Option<u32>,
    /// A document's own extension, so the file opens as what it is.
    ext: Option<String>,
}

impl MediaInfo {
    pub(super) fn extension(&self) -> String {
        self.ext.clone().unwrap_or_else(|| extension_for(self.kind, self.media_type).to_string())
    }
}

/// A safe file extension from a document's name, or its MIME type for an SVG.
fn document_extension(document: &wa::message::DocumentMessage) -> Option<String> {
    let from_name = document
        .file_name
        .as_deref()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    from_name.or_else(|| (document.mimetype.as_deref() == Some("image/svg+xml")).then(|| "svg".to_string()))
}

pub(super) fn detect_media(message: &wa::Message) -> Option<MediaInfo> {
    if let Some(image) = message.image_message.as_option() {
        return Some(MediaInfo {
            kind: "image",
            media_type: MediaType::Image,
            downloadable: Box::new(image.clone()),
            thumb: image.jpeg_thumbnail.clone(),
            duration: None,
            ext: None,
        });
    }
    if let Some(video) = message.video_message.as_option() {
        let kind = if video.gif_playback.unwrap_or(false) {
            "gif"
        } else {
            "video"
        };
        return Some(MediaInfo {
            kind,
            media_type: MediaType::Video,
            downloadable: Box::new(video.clone()),
            thumb: video.jpeg_thumbnail.clone(),
            duration: None,
            ext: None,
        });
    }
    if let Some(audio) = message.audio_message.as_option() {
        return Some(MediaInfo {
            kind: "audio",
            media_type: MediaType::Audio,
            downloadable: Box::new(audio.clone()),
            thumb: None,
            duration: audio.seconds,
            ext: None,
        });
    }
    if let Some(document) = message.document_message.as_option() {
        return Some(MediaInfo {
            kind: "document",
            media_type: MediaType::Document,
            downloadable: Box::new(document.clone()),
            thumb: document.jpeg_thumbnail.clone(),
            duration: None,
            ext: document_extension(document),
        });
    }
    if let Some(sticker) = message.sticker_message.as_option() {
        return Some(MediaInfo {
            kind: "sticker",
            media_type: MediaType::Sticker,
            downloadable: Box::new(sticker.clone()),
            thumb: None,
            duration: None,
            ext: None,
        });
    }
    None
}

/// Any picture as a WhatsApp sticker: fitted into 512×512 on transparency, as
/// WebP. A WebP is sent unchanged, so an animated sticker stays animated.
fn sticker_webp(bytes: &[u8]) -> Option<Vec<u8>> {
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

/// Nests a media message in the V2 view-once container.
///
/// The protocol library sends a caller-built message unchanged, so the container
/// is ours to build; its classification unwraps it for the stanza type and the
/// `<meta view_once="true"/>` hint, and recipients render what is inside as
/// one-time media.
pub(super) fn wrap_view_once(message: wa::Message) -> wa::Message {
    wa::Message {
        view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(message),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Downloads a stored message's media from its locator and records the file.
pub(super) async fn fetch_media(client: &Client, store: &MessageStore, dir: &Path, chat: &str, id: &str) -> Result<StoredMessage> {
    let Some(bytes) = store.media_ref_for(chat, id)? else {
        anyhow::bail!("no stored media reference");
    };
    let message = <wa::Message as buffa::Message>::decode(&mut bytes.as_slice())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let Some(media) = detect_media(&message) else {
        anyhow::bail!("message carries no media");
    };
    let started = std::time::Instant::now();
    let data = tokio::time::timeout(Duration::from_secs(120), client.download(media.downloadable.as_ref()))
        .await
        .map_err(|_| anyhow::anyhow!("download timed out"))?
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    log::debug!("downloaded {id} {} ({} KB) in {:?}", media.kind, data.len() / 1024, started.elapsed());
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.{}", id, media.extension()));
    std::fs::write(&path, &data)?;
    store.set_media_path(chat, id, &path.to_string_lossy())?;
    store.message(chat, id)
}

/// Where a chat's cached profile picture lives.
pub(super) fn avatar_path(media_dir: &Path, jid: &str) -> PathBuf {
    let name: String = jid
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    media_dir.join("avatars").join(format!("{name}.jpg"))
}

/// Where the full-size picture is cached, beside the preview.
pub(super) fn avatar_full_path(media_dir: &Path, jid: &str) -> PathBuf {
    let preview = avatar_path(media_dir, jid);
    let stem = preview.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    preview.with_file_name(format!("{stem}-full.jpg"))
}

/// File extension for a downloaded media item.
fn extension_for(kind: &str, media_type: MediaType) -> &'static str {
    match kind {
        "image" => "jpg",
        "video" | "gif" => "mp4",
        "audio" => "ogg",
        "sticker" => "webp",
        "document" => "bin",
        _ => match media_type {
            MediaType::Image => "jpg",
            _ => "bin",
        },
    }
}

/// A small JPEG preview for an outgoing attachment.
///
/// Images are downscaled locally. Video needs a decoder, so it is best effort:
/// Media Foundation on Windows, ffmpeg elsewhere when present, and `None`
/// means the file goes without a preview.
fn media_thumbnail(kind: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    match kind {
        "image" => image_thumbnail(bytes),
        "video" | "gif" => video_thumbnail(bytes),
        _ => None,
    }
}

fn image_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    jpeg_thumbnail(image::load_from_memory(bytes).ok()?)
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
                    let frame = first_frame(bytes);
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
unsafe fn first_frame(bytes: &[u8]) -> Option<image::RgbImage> {
    use windows::Win32::{Media::MediaFoundation::*, UI::Shell::SHCreateMemStream};

    let stream = MFCreateMFByteStreamOnStream(&SHCreateMemStream(Some(bytes))?).ok()?;
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 1).ok()?;
    let attributes = attributes?;
    // Lets the reader convert whatever the decoder emits to RGB32.
    attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
    let reader = MFCreateSourceReaderFromByteStream(&stream, &attributes).ok()?;

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

/// MIME type for an outgoing attachment, from its file extension.
fn mime_for(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" | "mov" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        _ => return None,
    })
}
