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

enum MediaInput { Bytes(Vec<u8>), File(PathBuf) }

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

    /// Deletes media files no stored message points at any more, so a retention
    /// prune does not leave the disk growing with orphans.
    pub async fn prune_orphaned_media(&self) -> Result<usize> {
        let Some(directory) = self.media_dir.clone() else { return Ok(0) };
        let referenced = self.store.referenced_media_paths().await?;
        let mut removed = 0;
        for path in orphaned_media_files(Some(&directory), &referenced) {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
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
        let SendOptions { gif, view_once, voice, forwarded, mentions, progress } = options;
        let to: Jid = chat.parse()?;
        self.unarchive_on_send(chat).await;
        let to_self = self.is_self_jid(&to);
        let chat_jid = to.to_string();
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

        let upload = match &input {
            MediaInput::Bytes(bytes) => match progress {
                Some(token) => self.upload_reporting(bytes.clone(), media_type, token).await?,
                None => self.client.upload(bytes.clone(), media_type, Default::default()).await?,
            },
            MediaInput::File(path) => {
                let path = path.clone();
                let (source, info) = tokio::task::spawn_blocking(move || super::media_files::encrypt_file(&path, media_type)).await??;
                match progress {
                    Some(token) => {
                        use whatsapp_rust::wacore::upload::UploadSource;
                        let report = self.upload_reporter(token, source.len());
                        self.client.upload_stream(ProgressSource { source, report }, info, media_type).await?
                    }
                    None => self.client.upload_stream(source, info, media_type).await?,
                }
            }
        };

        let mimetype = mime_for(&extension).map(str::to_string);

        // A thumbnail lets the recipient see a preview before the file lands.
        let thumb = match &input {
            MediaInput::Bytes(bytes) => media_thumbnail(kind, bytes),
            MediaInput::File(path) => {
                let path = path.clone();
                tokio::task::spawn_blocking(move || super::media_codec::media_thumbnail_file(kind, &path)).await?
            }
        };
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

        // An attachment can carry a quote, the same as a text reply. The quoted
        // message is the message itself where the store has it, so the recipient
        // renders the view-once it answers rather than a stand-in for it.
        let context = self.reply_context(&to, reply.as_ref()).await?;
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

        let locator = (!view_once).then(|| media_locator(&message));
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id).await?;
        }
        // The sender cannot reopen view-once media either, so no copy is kept.
        if view_once {
            self.store.set_view_once(chat, &result.message_id, true).await?;
        }

        // Keep our own copy so the sender sees what they sent.
        let mut stored_path = None;
        if let Some(dir) = self.media_dir().filter(|_| !view_once) {
            let dir = &dir;
            if tokio::fs::create_dir_all(dir).await.observed().is_some() {
                let name = if extension.is_empty() { "bin".to_string() } else { extension.clone() };
                let dest = dir.join(format!("{}.{}", result.message_id, name));
                let saved = match &input {
                    MediaInput::Bytes(bytes) => tokio::fs::write(&dest, bytes).await,
                    MediaInput::File(path) => tokio::fs::copy(path, &dest).await.map(|_| ()),
                };
                if saved.observed().is_some() {
                    stored_path = Some(dest.to_string_lossy().to_string());
                }
            }
        }

        let kind = if gif && kind == "video" { "gif" } else { kind };
        let mut stored = self.own_message(chat, &result.message_id, caption.unwrap_or_default(), kind, to_self);
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
        if let Some(reply) = &reply {
            stored.quote = self.reply_quote(&chat_jid, reply).await?;
        }
        self.store.insert_message(&stored).await?;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
        Ok(warning)
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
        let to: Jid = chat.parse()?;
        self.unarchive_on_send(chat).await;
        let context = self.reply_context(&to, reply.as_ref()).await?;
        let context = if forwarded { Some(forwarded_context(context)) } else { context };
        let to_self = self.is_self_jid(&to);
        let webp = sticker_webp(&bytes)
            .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
        let (width, height) = webp_dimensions(&webp).unwrap_or((512, 512));
        let animated = webp_is_animated(&webp);
        let png_thumbnail = sticker_png_thumbnail(&webp);
        let upload = self
            .client
            .upload(webp.clone(), MediaType::Sticker, Default::default())
            .await?;
        let filehash = filehash_of_hash(&upload.file_sha256);
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
                sticker_sent_ts: Some(unix_now() * 1000),
                context_info: context.map(|c| MessageField::some(*c)).unwrap_or_else(MessageField::none),
                ..Default::default()
            }),
            ..Default::default()
        };
        let locator = media_locator(&message);
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id).await?;
        }

        let media_path = self.media_dir().and_then(|dir| {
            std::fs::create_dir_all(&dir).observed()?;
            let dest = dir.join(format!("{}.webp", result.message_id));
            std::fs::write(&dest, &webp).observed()?;
            Some(dest.to_string_lossy().into_owned())
        });
        let mut stored = self.own_message(chat, &result.message_id, "[sticker]".into(), "sticker", to_self);
        stored.media.path = media_path;
        stored.media.locator = Some(locator);
        if let Some(reply) = &reply {
            stored.quote = self.reply_quote(chat, reply).await?;
        }
        self.store.insert_message(&stored).await?;
        if let Err(e) = record_sticker(&self.store, &stored).await {
            log::warn!("could not record a sent sticker: {e}");
        }
        let now = unix_now();
        let _ = self.store.set_sticker_recent(filehash, Some(now), now).await;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
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
        let dir = self.media_dir().ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let file = std::fs::canonicalize(path)?;
        if !file.starts_with(std::fs::canonicalize(&dir)?) {
            anyhow::bail!("refusing to send a file from outside the media folder");
        }
        let bytes = std::fs::read(&file)?;
        match kind {
            "sticker" => self.send_sticker(chat, bytes, reply).await,
            "gif" => {
                let options = SendOptions { gif: true, ..Default::default() };
                self.send_media(chat, "gif.mp4", bytes, None, reply, options).await.map(|_| ())
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
