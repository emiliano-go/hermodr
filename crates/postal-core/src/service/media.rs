//! Sending attachments, tracking uploads and managing the local media library.

use super::*;
use whatsapp_rust::media::{self, AudioOptions, DocumentOptions, ImageOptions, VideoOptions};

/// Encrypted media handed to the uploader, reporting how far it has been read.
struct ProgressSource<S> {
    source: S,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

impl<S: whatsapp_rust::wacore::upload::UploadSource> whatsapp_rust::wacore::upload::UploadSource for ProgressSource<S> {
    fn len(&self) -> u64 {
        self.source.len()
    }

    fn reader_from(&self, offset: u64) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let reader = self.source.reader_from(offset)?;
        Ok(Box::new(CountingReader { inner: reader, read: offset.min(self.len()), report: Arc::clone(&self.report) }))
    }
}

struct CountingReader {
    inner: Box<dyn std::io::Read + Send>,
    read: u64,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

pub(super) enum MediaInput { Bytes(Vec<u8>), File(PathBuf) }

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.report)(self.read);
        Ok(n)
    }
}

impl WhatsAppService {
    /// Returns an ordinary attachment, downloading its original file if needed.
    pub async fn media_for_export(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let mut message = self.store.message(chat, id).await?;
        ensure_exportable_media(&message)?;
        if message.media.path.as_ref().is_none_or(|path| !Path::new(path).is_file()) {
            self.download_media(chat, id).await?;
            message = self.store.message(chat, id).await?;
            ensure_exportable_media(&message)?;
        }
        Ok(message)
    }

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

    /// Takes the view-once a reply quotes, for one this account sent.
    ///
    /// WhatsApp never hands view-once media to a linked device; a reply quoting
    /// it carries the only copy that arrives. It is written under the quoted
    /// message's own name, so every reply quoting the same view-once shares one
    /// file and one download.
    pub async fn recover_quote_media(&self, chat: &str, id: &str) -> Result<()> {
        let dir = self
            .media_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no media folder configured"))?;
        let updated = fetch_quote_media(&self.client, &self.store, &dir, chat, id).await?;
        let _ = self.events.send(ServiceEvent::hint(&updated, false));
        Ok(())
    }

    /// Deletes recovered view-once files no stored message points at any more.
    ///
    /// Called wherever rows go, since a copy outlives the reply that fetched it
    /// only as long as some row still names it.
    pub async fn prune_quote_files(&self) -> Result<usize> {
        let directory = self.media_dir.clone();
        self.store.run(move |store| prune_quote_files(directory.as_deref(), store)).await
    }

    /// Deletes downloaded media and forgets the paths, keeping the messages.
    pub async fn flush_media(&self) -> Result<usize> {
        let directory = self.media_dir.clone();
        self.store.run(move |store| {
            let cleared = store.clear_media_paths()?;
            if let Some(dir) = &directory {
                if dir.exists() {
                    for entry in std::fs::read_dir(dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_dir() {
                            std::fs::remove_dir_all(&path)?;
                        } else {
                            std::fs::remove_file(&path)?;
                        }
                    }
                }
            }
            Ok(cleared)
        }).await
    }

    /// Where media is stored, if enabled.
    pub fn media_dir(&self) -> Option<PathBuf> {
        self.media_dir.clone()
    }

    /// Every downloaded file this account's messages point at.
    pub async fn media_paths(&self) -> Result<Vec<String>> {
        self.store.media_paths().await
    }

    /// Incoming one-time messages still waiting for their media. The optional
    /// Android instance wakes on these instead of staying linked.
    pub async fn pending_view_once(&self, within: std::time::Duration) -> Vec<(String, String)> {
        match self.store.pending_view_once(within).await {
            Ok(pending) => pending,
            Err(error) => {
                log::error!("could not list pending view-once media: {error}");
                Vec::new()
            }
        }
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
        self.send_media_input(chat, file_name, MediaInput::Bytes(bytes), caption, reply, options).await
    }

    pub async fn send_media_file(
        &self, chat: &str, file_name: &str, path: PathBuf, caption: Option<String>,
        reply: Option<(String, String, String)>, options: SendOptions,
    ) -> Result<Option<String>> {
        self.send_media_input(chat, file_name, MediaInput::File(path), caption, reply, options).await
    }

    async fn send_media_input(
        &self, chat: &str, file_name: &str, input: MediaInput, caption: Option<String>,
        reply: Option<(String, String, String)>, options: SendOptions,
    ) -> Result<Option<String>> {
        let SendOptions { gif, view_once, voice, forwarded, mentions, progress, quality } = options;
        let to = super::broadcast_lists::writable_target(chat)?;
        let original_extension = file_extension(file_name);
        let (_, original_kind) = media_kind_for(&original_extension);
        let prepared = super::media_quality::prepare(input, file_name.to_string(), original_kind, quality, gif).await?;
        let input = &prepared.input;
        self.unarchive_on_send(chat).await;
        let to_self = self.is_self_jid(&to);
        let chat_jid = to.to_string();
        let file_name = &prepared.file_name;
        let extension = file_extension(file_name);
        let (media_type, kind) = media_kind_for(&extension);

        let upload = self.upload_media(input, media_type, progress).await?;
        let mimetype = mime_for(&extension).map(str::to_string);
        let thumb = self.outgoing_thumbnail(kind, input).await?;
        let warning = missing_preview_warning(kind, &thumb);
        let context = self.media_context(&to, reply.as_ref(), forwarded, mentions).await?;
        let voice_seconds = voice.as_ref().map(|note| note.seconds);
        let mut message = build_media_message(
            file_name, kind, upload, &caption, mimetype, &thumb, context, gif, voice, extension == "ogg",
        );
        if view_once {
            message = apply_view_once(kind, message)?;
        }

        let locator = (!view_once).then(|| media_locator(&message));
        group_history::guard_ordinary_message(&message)?;
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id).await?;
        }
        // One-time media is never kept: the sender cannot reopen it either.
        let stored_path = if view_once {
            None
        } else {
            self.keep_sent_copy(input, &result.message_id, &extension).await
        };
        let stored = self.record_sent_media(
            chat, &chat_jid, &result.message_id, caption.unwrap_or_default(), kind, to_self,
            view_once, stored_path, locator, voice_seconds, reply.as_ref(),
        ).await?;
        let stored = self.store.insert_message_row(&stored).await?;
        let _ = self.events.send(ServiceEvent::arrival(&stored));
        Ok(warning)
    }

    /// Uploads staged bytes or a file, reporting progress when a token was given.
    pub(super) async fn upload_media(
        &self,
        input: &MediaInput,
        media_type: MediaType,
        progress: Option<String>,
    ) -> Result<whatsapp_rust::upload::UploadResponse> {
        match input {
            MediaInput::Bytes(bytes) => match progress {
                Some(token) => self.upload_reporting(bytes.clone(), media_type, token).await,
                None => Ok(self.client.upload(bytes.clone(), media_type, Default::default()).await?),
            },
            MediaInput::File(path) => {
                let path = path.clone();
                let (source, info) = tokio::task::spawn_blocking(move || super::media_files::encrypt_file(&path, media_type)).await??;
                match progress {
                    Some(token) => {
                        use whatsapp_rust::wacore::upload::UploadSource;
                        let report = self.upload_reporter(token, source.len());
                        Ok(self.client.upload_stream(ProgressSource { source, report }, info, media_type).await?)
                    }
                    None => Ok(self.client.upload_stream(source, info, media_type).await?),
                }
            }
        }
    }

    /// A thumbnail the recipient sees before the file lands, if one can be made.
    pub(super) async fn outgoing_thumbnail(&self, kind: &str, input: &MediaInput) -> Result<Option<Vec<u8>>> {
        match input {
            MediaInput::Bytes(bytes) => Ok(media_thumbnail(kind, bytes)),
            MediaInput::File(path) => {
                let path = path.clone();
                let kind = kind.to_string();
                Ok(tokio::task::spawn_blocking(move || super::media_codec::media_thumbnail_file(&kind, &path)).await?)
            }
        }
    }

    /// The quote, forwarded marker and mentions an attachment goes out with.
    /// The quoted message is the message itself where the store has it, so the
    /// recipient renders the view-once it answers rather than a stand-in.
    pub(super) async fn media_context(
        &self,
        to: &Jid,
        reply: Option<&(String, String, String)>,
        forwarded: bool,
        mentions: Vec<String>,
    ) -> Result<Option<Box<wa::ContextInfo>>> {
        let context = self.reply_context(to, reply).await?;
        let context = if forwarded { Some(forwarded_context(context)) } else { context };
        if mentions.is_empty() {
            return Ok(context);
        }
        let mut context = context.unwrap_or_default();
        context.mentioned_jid = mentions;
        Ok(Some(context))
    }

    /// A local copy of what was just sent, so the sender sees their own media.
    pub(super) async fn keep_sent_copy(&self, input: &MediaInput, id: &str, extension: &str) -> Option<String> {
        let dir = self.media_dir()?;
        if tokio::fs::create_dir_all(&dir).await.observed().is_none() {
            return None;
        }
        let name = if extension.is_empty() { "bin" } else { extension };
        let dest = dir.join(format!("{id}.{name}"));
        let saved = match input {
            MediaInput::Bytes(bytes) => tokio::fs::write(&dest, bytes).await,
            MediaInput::File(path) => tokio::fs::copy(path, &dest).await.map(|_| ()),
        };
        saved.observed()?;
        Some(dest.to_string_lossy().to_string())
    }

    /// The stored row for media we just sent; one-time media keeps its one-time
    /// form and no file.
    pub(super) async fn record_sent_media(
        &self,
        chat: &str,
        chat_jid: &str,
        id: &str,
        caption: String,
        kind: &str,
        to_self: bool,
        view_once: bool,
        stored_path: Option<String>,
        locator: Option<Vec<u8>>,
        voice_seconds: Option<u32>,
        reply: Option<&(String, String, String)>,
    ) -> Result<StoredMessage> {
        if view_once {
            self.store.set_view_once(chat, id, true).await?;
        }
        let mut stored = self.own_message(chat, id, caption, kind, to_self);
        if view_once {
            // The sender cannot reopen it either, so it is kept as the one-time
            // form (no file, original kind remembered) rather than a broken
            // ordinary attachment.
            stored.media.once_kind = Some(kind.to_string());
            stored.media.kind = Some("view_once".to_string());
        }
        stored.media.path = stored_path;
        stored.media.locator = locator;
        stored.media.duration = voice_seconds;
        if let Some(reply) = reply {
            stored.quote = self.reply_quote(chat_jid, reply).await?;
        }
        Ok(stored)
    }

    /// The quote context for an attachment answering `(id, sender, text)`. The
    /// quoted message is the message itself where the store has it, so the
    /// recipient renders the view-once it answers rather than a stand-in.
    async fn reply_context(&self, to: &Jid, reply: Option<&(String, String, String)>) -> Result<Option<Box<wa::ContextInfo>>> {
        let Some((id, sender, text)) = reply else { return Ok(None) };
        use whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info;
        let sender: Jid = sender.parse::<Jid>()?.to_non_ad();
        let quoted = self.quoted_message(&to.to_string(), id, text).await;
        let mut context = build_quote_context_with_info(id, &sender, to, to, quoted.as_ref().unwrap_or(&wa::Message::text("")));
        if quoted.is_none() {
            context.quoted_message = Default::default();
        }
        Ok(Some(Box::new(context)))
    }

    /// The quote stored beside an attachment this account sent as a reply.
    async fn reply_quote(&self, chat: &str, (id, sender, _): &(String, String, String)) -> Result<Quote> {
        let sender: Jid = sender.parse::<Jid>()?.to_non_ad();
        Ok(self.local_quote(chat, id, &sender.to_string(), sender.to_string() == self.own_jid()).await)
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
        let report = self.upload_reporter(token, total);
        let source = ProgressSource { source: Arc::<[u8]>::from(enc.data_to_upload), report };
        let info = EncryptedMediaInfo {
            media_key: enc.media_key,
            file_sha256: enc.file_sha256,
            file_enc_sha256: enc.file_enc_sha256,
            file_length,
            streaming_sidecar: enc.streaming_sidecar,
        };
        Ok(self.client.upload_stream(source, info, media_type).await?)
    }

    fn upload_reporter(&self, token: String, total: u64) -> Arc<dyn Fn(u64) + Send + Sync> {
        let events = self.events.clone();
        let last = Arc::new(AtomicU64::new(u64::MAX));
        Arc::new(move |sent: u64| {
            let percent = sent.saturating_mul(100) / total.max(1);
            if last.swap(percent, Ordering::Relaxed) != percent {
                let _ = events.send(ServiceEvent::UploadProgress { token: token.clone(), sent, total });
            }
        })
    }

    /// Sends a picture as a sticker (see [`sticker_webp`]).
    pub async fn send_sticker(&self, chat: &str, bytes: Vec<u8>, reply: Option<(String, String, String)>) -> Result<()> {
        self.send_sticker_as(chat, bytes, false, reply).await
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
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!("saved-{nanos}.webp"));
        std::fs::write(&path, webp)?;
        Ok(path.to_string_lossy().into_owned())
    }

    pub(super) async fn send_sticker_as(
        &self,
        chat: &str,
        bytes: Vec<u8>,
        forwarded: bool,
        reply: Option<(String, String, String)>,
    ) -> Result<()> {
        let to = super::broadcast_lists::writable_target(chat)?;
        self.unarchive_on_send(chat).await;
        let context = self.reply_context(&to, reply.as_ref()).await?;
        let context = if forwarded { Some(forwarded_context(context)) } else { context };
        let to_self = self.is_self_jid(&to);
        let prepared = tokio::task::spawn_blocking(move || super::media_sticker_file::prepare(bytes)).await??;
        let (width, height) = prepared.dimensions;
        let animated = prepared.animated;
        let png_thumbnail = prepared.thumbnail;
        let input = MediaInput::File(prepared.file.path.clone());
        let upload = self.upload_media(&input, MediaType::Sticker, None).await?;
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
                width: Some(width),
                height: Some(height),
                is_animated: animated.then_some(true),
                png_thumbnail,
                sticker_sent_ts: Some(whatsapp_rust::wacore::time::now_millis()),
                context_info: context.map(|c| MessageField::some(*c)).unwrap_or_else(MessageField::none),
                ..Default::default()
            }),
            ..Default::default()
        };
        let locator = media_locator(&message);
        group_history::guard_ordinary_message(&message)?;
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id).await?;
        }

        let media_path = self.media_dir().and_then(|dir| {
            std::fs::create_dir_all(&dir).observed()?;
            let dest = dir.join(format!("{}.webp", result.message_id));
            std::fs::copy(&prepared.file.path, &dest).observed()?;
            Some(dest.to_string_lossy().into_owned())
        });
        let mut stored = self.own_message(chat, &result.message_id, "[sticker]".into(), "sticker", to_self);
        stored.media.path = media_path;
        stored.media.locator = Some(locator);
        if let Some(reply) = &reply {
            stored.quote = self.reply_quote(chat, reply).await?;
        }
        let stored = self.store.insert_message_row(&stored).await?;
        match record_sticker(&self.store, &stored).await {
            Ok(true) => { let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: false, favorites: false, recents: true }); },
            Ok(false) => {},
            Err(error) => log::warn!("could not record a sent sticker: {error}"),
        }
        let _ = self.events.send(ServiceEvent::arrival(&stored));
        Ok(())
    }

    /// Stickers or GIFs already on this device, newest first, for the picker.
    /// Recent stickers or GIFs, newest first, one per distinct file. Copies of a
    /// path in `prefer` (the favourites) are dropped in its favour.
    pub async fn media_library(&self, kind: &str, prefer: &[String]) -> Result<Vec<String>> {
        use std::hash::{Hash, Hasher};
        use std::io::Read;
        let recent = self.store.recent_media(kind, 200).await?;
        let dir = self.media_dir().and_then(|d| std::fs::canonicalize(d).ok());
        // Favourites come from the UI, so only files in the media folder count.
        let preferred = prefer.iter().filter(|p| {
            dir.as_ref()
                .is_some_and(|d| std::fs::canonicalize(p).is_ok_and(|f| f.starts_with(d)))
        });
        // Stickers saved here are files, not messages, so nothing in the store
        // names them; list the folder so they do not vanish when unfavourited.
        let saved: Vec<String> = if kind == "sticker" {
            self.media_dir()
                .map(|d| d.join("stickers"))
                .and_then(|d| std::fs::read_dir(d).ok())
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .map(|path| path.to_string_lossy().into_owned())
                .collect()
        } else {
            Vec::new()
        };
        // The same sticker sent or received again is a new file; show it once.
        // Length plus the first 64 KiB identifies it without reading whole videos.
        let mut seen = std::collections::HashSet::new();
        let mut listed = std::collections::HashSet::new();
        Ok(preferred
            .cloned()
            .chain(saved)
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
    pub async fn send_from_library(
        &self,
        chat: &str,
        path: &str,
        kind: &str,
        reply: Option<(String, String, String)>,
    ) -> Result<()> {
        super::broadcast_lists::writable_target(chat)?;
        let dir = self.media_dir().ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let file = std::fs::canonicalize(path)?;
        if !file.starts_with(std::fs::canonicalize(&dir)?) {
            anyhow::bail!("refusing to send a file from outside the media folder");
        }
        match kind {
            "sticker" => self.send_sticker(chat, super::media_sticker_file::read_sticker_file(&file)?, reply).await,
            "gif" => {
                let options = SendOptions { gif: true, ..Default::default() };
                self.send_media_file(chat, "gif.mp4", file, None, reply, options).await.map(|_| ())
            }
            _ => anyhow::bail!("only stickers and GIFs are sent from the library"),
        }
    }
}

fn ensure_exportable_media(message: &StoredMessage) -> Result<()> {
    anyhow::ensure!(!message.local.revoked, "this message was deleted");
    anyhow::ensure!(matches!(message.media.kind.as_deref(),
        Some("image" | "video" | "gif" | "audio" | "document" | "sticker")),
        "this message has no exportable attachment");
    Ok(())
}

/// Extensions with a dedicated wire media type; anything else goes as a
/// document, which is how the receiver will render it.
pub(super) fn file_extension(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The extension decides how the receiver renders the file, so a video only
/// arrives as a video (not a document) if it is sent as one.
pub(super) fn media_kind_for(extension: &str) -> (MediaType, &'static str) {
    match extension {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => (MediaType::Image, "image"),
        "mp4" | "mov" | "m4v" | "webm" | "mkv" => (MediaType::Video, "video"),
        "ogg" | "opus" | "mp3" | "m4a" | "aac" | "wav" => (MediaType::Audio, "audio"),
        _ => (MediaType::Document, "document"),
    }
}

/// Why a video went out without a preview, when it did.
pub(super) fn missing_preview_warning(kind: &str, thumb: &Option<Vec<u8>>) -> Option<String> {
    if !(kind == "video" || kind == "gif") || thumb.is_some() {
        return None;
    }
    Some(
        if cfg!(windows) {
            "The video was sent without a preview because Windows could not decode it."
        } else {
            "The video was sent without a preview because ffmpeg is not installed."
        }
        .to_string(),
    )
}

/// The wire message for an uploaded attachment, by kind.
pub(super) fn build_media_message(
    file_name: &str,
    kind: &str,
    upload: whatsapp_rust::upload::UploadResponse,
    caption: &Option<String>,
    mimetype: Option<String>,
    thumb: &Option<Vec<u8>>,
    context: Option<Box<wa::ContextInfo>>,
    gif: bool,
    voice: Option<VoiceNote>,
    ptt: bool,
) -> wa::Message {
    match kind {
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
                ptt: Some(ptt),
                duration_seconds: voice.as_ref().map(|note| note.seconds),
                waveform: voice.map(|note| note.waveform),
                context_info: context,
            },
        ),
        _ => media::document_message(
            upload,
            DocumentOptions {
                file_name: Some(file_name.to_string()),
                caption: caption.clone(),
                mimetype,
                jpeg_thumbnail: thumb.clone(),
                context_info: context,
                ..Default::default()
            },
        ),
    }
}

/// Marks the media one-time and wraps it in the V2 container WhatsApp renders
/// one-time media from. Documents have no view-once form.
fn apply_view_once(kind: &str, mut message: wa::Message) -> Result<wa::Message> {
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
    // The inline flags above are hints kept for clients that read them.
    Ok(wrap_view_once(message))
}


#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn only_ordinary_undeleted_attachments_can_be_exported() {
        let mut message = StoredMessage::default();
        for kind in [None, Some("view_once"), Some("poll"), Some("event")] {
            message.media.kind = kind.map(str::to_owned);
            assert!(ensure_exportable_media(&message).is_err());
        }
        for kind in ["image", "video", "gif", "audio", "document", "sticker"] {
            message.media.kind = Some(kind.into());
            message.local.revoked = false;
            assert!(ensure_exportable_media(&message).is_ok());
            message.local.revoked = true;
            assert!(ensure_exportable_media(&message).is_err());
        }
    }
}
