use super::*;
use super::media::{MediaInput, build_media_message, file_extension, media_kind_for, missing_preview_warning};
use crate::store::Album;
use whatsapp_rust::wacore::proto_helpers::wrap_as_album_child;

pub const MAX_ALBUM_ITEMS: usize = 8;
const MAX_CAPTION_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct AlbumMediaInput {
    pub name: String,
    pub path: PathBuf,
    pub caption: Option<String>,
    pub quality: Option<MediaQuality>,
    pub progress: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct AlbumSendResult {
    pub account_id: String,
    pub chat: String,
    pub parent_id: String,
    pub sent_ids: Vec<String>,
    pub next_index: usize,
    pub uncertain_index: Option<usize>,
    pub uncertain_id: Option<String>,
    pub parent_uncertain: bool,
    pub preflight_failed: bool,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl AlbumSendResult {
    pub fn preflight_failure(account: &str, chat: &str, parent: Option<&str>, error: impl Into<String>) -> Self {
        Self { account_id: account.into(), chat: chat.into(), parent_id: parent.unwrap_or_default().into(),
            preflight_failed: true, error: Some(error.into()), ..Default::default() }
    }

    fn sent(&mut self, index: usize, id: String) {
        self.sent_ids.push(id);
        self.next_index = index + 1;
    }

    fn stopped(&mut self, index: usize, error: impl std::fmt::Display) {
        self.next_index = index;
        self.error = Some(error.to_string());
    }

    fn uncertain(&mut self, index: usize, id: String, error: impl std::fmt::Display) {
        self.uncertain_index = Some(index);
        self.uncertain_id = Some(id);
        self.stopped(index + 1, error);
    }

    fn note_fanout(&mut self, label: &str, sent: &whatsapp_rust::SendResult) {
        if let Some(fanout) = &sent.recipient_fanout {
            if fanout.is_partial() {
                self.warnings.push(format!("Album {label} omitted {} recipient device(s){}.",
                    fanout.addressed - fanout.encrypted,
                    if fanout.skipped_primary { "; the primary device was omitted" } else { "" }));
            }
        }
    }
}

struct PreparedAlbumItem {
    media: media_quality::PreparedMedia,
    message: wa::Message,
    caption: String,
    kind: &'static str,
    extension: String,
    warning: Option<String>,
}

impl WhatsAppService {
    pub async fn send_album(
        &self, account_id: &str, chat: &str, items: Vec<AlbumMediaInput>,
        parent_id: Option<&str>, reply: Option<(String, String, String)>, mentions: Vec<String>,
        current: impl Fn() -> Result<()> + Send + Sync,
    ) -> Result<AlbumSendResult> {
        current()?;
        anyhow::ensure!(!account_id.is_empty() && self.is_connected(), "Album account is not connected.");
        let target = album_target(chat, reply.as_ref(), &mentions)?;
        let existing = self.album_continuation(chat, parent_id, &current).await?;
        let continuation = existing.is_some();
        let (images, videos) = album_counts(&items, continuation)?;
        let prepared = self.prepare_album(&target, items, mentions, &current).await?;
        let mut result = AlbumSendResult { account_id: account_id.into(), chat: chat.into(), ..Default::default() };
        let parent_key = if existing.is_some() {
            let existing = self.album_continuation(chat, parent_id, &current).await?
                .ok_or_else(|| anyhow::anyhow!("Album continuation parent disappeared."))?;
            result.parent_id = existing.header.id.clone();
            continuation_key(&existing, &existing.header.chat)?
        } else {
            match self.create_album_parent(chat, &target, (images, videos), reply.as_ref(), &current, &mut result).await {
                Ok(Some(key)) => key,
                Ok(None) => return Ok(result),
                Err(error) => { result.preflight_failed = true; result.error = Some(error.to_string()); return Ok(result); }
            }
        };
        send_album_sequence(prepared.len(), |index| {
            self.send_album_child(chat, &target, &parent_key, index,
                (!continuation).then_some(index as u32), &prepared[index], &current)
        }, &mut result).await;
        Ok(result)
    }

    async fn album_continuation(
        &self, chat: &str, parent_id: Option<&str>, current: &impl Fn() -> Result<()>,
    ) -> Result<Option<StoredMessage>> {
        let Some(id) = parent_id else { return Ok(None); };
        anyhow::ensure!(!id.is_empty() && id.len() <= 256, "Invalid album continuation parent.");
        current()?;
        let parent = self.store.album_parent_for_send(chat, id).await?;
        current()?;
        continuation_key(&parent, &parent.header.chat)?;
        let sender: Jid = parent.header.sender.parse()?;
        anyhow::ensure!(self.is_self_jid(&sender), "Album continuation parent belongs to another sender.");
        Ok(Some(parent))
    }

    async fn create_album_parent(
        &self, chat: &str, target: &Jid, counts: (u32, u32), reply: Option<&(String, String, String)>,
        current: &impl Fn() -> Result<()>, result: &mut AlbumSendResult,
    ) -> Result<Option<wa::MessageKey>> {
        let context = self.media_context(target, reply, false, Vec::new()).await?;
        current()?;
        let parent = album_message(counts.0, counts.1, context);
        group_history::guard_ordinary_message(&parent)?;
        let parent_id = self.client.generate_message_id();
        result.parent_id = parent_id.clone();
        self.unarchive_on_send(chat).await;
        current()?;
        let mut stored = self.record_sent_media(chat, &target.to_string(), &parent_id,
            String::new(), "album", self.is_self_jid(target), false, None, None, None, reply).await?;
        pending_album_row(&mut stored);
        stored.album = Some(Album { parent_id: None, expected_images: Some(counts.0), expected_videos: Some(counts.1), index: None });
        current()?;
        let stored = match self.store.insert_message_row(&stored).await {
            Ok(stored) => stored,
            Err(error) => return Err(anyhow::anyhow!(self.album_before_error(&stored, error).await)),
        };
        if let Err(error) = current() {
            return Err(anyhow::anyhow!(self.album_before_error(&stored, error).await));
        }
        let sent = match self.client.send_message_with_options(target.clone(), parent,
            whatsapp_rust::SendOptions::default().with_message_id(parent_id)).await {
            Ok(sent) => sent,
            Err(error) if failed_before_transport(&error) => {
                result.preflight_failed = true;
                result.error = Some(self.album_before_error(&stored, error).await);
                return Ok(None);
            }
            Err(error) => {
                result.parent_uncertain = true;
                result.error = Some(error.to_string());
                return Ok(None);
            }
        };
        if let Err(error) = self.store.mark_album_request_written(chat, &stored.header.id).await {
            let message = format!("Album parent was written to transport, but its local write provenance could not be saved: {error}");
            result.warnings.push(message.clone());
            result.stopped(0, message);
            return Ok(None);
        }
        result.note_fanout("parent", &sent);
        if let Some(warning) = self.album_arrival(&stored, self.is_self_jid(&target)).await { result.warnings.push(warning); }
        if let Err(error) = current() { result.stopped(0, error); return Ok(None); }
        Ok(Some(sent.message_key()))
    }

    async fn prepare_album(
        &self, target: &Jid, items: Vec<AlbumMediaInput>, mentions: Vec<String>,
        current: &impl Fn() -> Result<()>,
    ) -> Result<Vec<PreparedAlbumItem>> {
        let mut prepared = Vec::with_capacity(items.len());
        for (index, item) in items.into_iter().enumerate() {
            current()?;
            let (_, kind) = media_kind_for(&file_extension(&item.name));
            let metadata = tokio::fs::metadata(&item.path).await?;
            current()?;
            let limit = if kind == "image" { 64 * 1024 * 1024 } else { 512 * 1024 * 1024 };
            anyhow::ensure!(metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
                "Album item exceeds the application media size bound or is empty.");
            let media = media_quality::prepare(MediaInput::File(item.path), item.name, kind, item.quality, false).await?;
            current()?;
            let extension = file_extension(&media.file_name);
            let (media_type, kind) = media_kind_for(&extension);
            let upload = self.upload_media(&media.input, media_type, item.progress).await?;
            current()?;
            let thumb = self.outgoing_thumbnail(kind, &media.input).await?;
            current()?;
            let context = self.media_context(target, None, false, if index == 0 { mentions.clone() } else { Vec::new() }).await?;
            current()?;
            let message = build_media_message(&media.file_name, kind, upload, &item.caption,
                mime_for(&extension).map(str::to_string), &thumb, context, false, None, false);
            group_history::guard_ordinary_message(&message)?;
            prepared.push(PreparedAlbumItem { media, message, caption: item.caption.unwrap_or_default(),
                kind, extension, warning: missing_preview_warning(kind, &thumb) });
        }
        Ok(prepared)
    }

    async fn send_album_child(
        &self, chat: &str, target: &Jid, parent: &wa::MessageKey, index: usize,
        stored_index: Option<u32>, item: &PreparedAlbumItem, current: &impl Fn() -> Result<()>,
    ) -> std::result::Result<AlbumSendAttempt, AlbumSendFailure> {
        current().map_err(AlbumSendFailure::before)?;
        if stored_index.is_none() {
            self.album_continuation(chat, parent.id.as_deref(), current).await.map_err(AlbumSendFailure::before)?;
        }
        let id = self.client.generate_message_id();
        let stored = self.store_album_child(chat, target, &id, parent, stored_index, item, current)
            .await.map_err(AlbumSendFailure::before)?;
        if let Err(error) = current() {
            return Err(AlbumSendFailure::Before(self.album_before_error(&stored, error).await));
        }
        if let Err(error) = self.album_continuation(chat, parent.id.as_deref(), current).await {
            return Err(AlbumSendFailure::Before(self.album_before_error(&stored, error).await));
        }
        let message = wrap_as_album_child(item.message.clone(), parent.clone());
        let sent = match self.client.send_message_with_options(target.clone(), message,
            whatsapp_rust::SendOptions::default().with_message_id(id.clone())).await {
            Ok(sent) => sent,
            Err(error) if failed_before_transport(&error) => {
                return Err(AlbumSendFailure::Before(self.album_before_error(&stored, error).await));
            }
            Err(error) => return Err(AlbumSendFailure::Uncertain(id.clone(), error.to_string())),
        };
        let mut warnings = AlbumSendResult::default();
        warnings.note_fanout(&format!("item {}", index + 1), &sent);
        if stored.media.path.is_none() {
            warnings.warnings.push(format!("Album item {} has no retained local copy; download it to reopen it.", index + 1));
        }
        if let Some(warning) = &item.warning { warnings.warnings.push(format!("Item {}: {warning}", index + 1)); }
        if let Some(warning) = self.album_arrival(&stored, self.is_self_jid(target)).await { warnings.warnings.push(warning); }
        Ok(AlbumSendAttempt { id, warnings: warnings.warnings, stop: current().err().map(|error| error.to_string()) })
    }

    async fn store_album_child(
        &self, chat: &str, target: &Jid, id: &str, parent: &wa::MessageKey, index: Option<u32>,
        item: &PreparedAlbumItem, current: &impl Fn() -> Result<()>,
    ) -> Result<StoredMessage> {
        let mut stored = self.record_sent_media(chat, &target.to_string(), id, item.caption.clone(), item.kind,
            self.is_self_jid(target), false, None, Some(media_locator(&item.message)), None, None).await?;
        pending_album_row(&mut stored);
        stored.album = Some(Album { parent_id: parent.id.clone(), index, expected_images: None, expected_videos: None });
        current()?;
        stored.media.path = self.keep_sent_copy(&item.media.input, id, &item.extension).await;
        if let Err(error) = current() {
            return Err(anyhow::anyhow!(self.album_before_error(&stored, error).await));
        }
        // Persist before transport: a local save failure leaves this item provably unsent.
        let parent_id = parent.id.as_deref().ok_or_else(|| anyhow::anyhow!("Album parent has no id."))?;
        match self.store.insert_album_child(parent_id, &stored).await {
            Ok(stored) => Ok(stored),
            Err(error) => Err(anyhow::anyhow!(self.album_before_error(&stored, error).await)),
        }
    }

    async fn album_arrival(&self, stored: &StoredMessage, to_self: bool) -> Option<String> {
        if let Err(error) = restore_self_album_delivery(&self.store, stored, to_self).await {
            return Some(format!("Album message was written to self, but its local delivery state could not be saved: {error}"));
        }
        match self.store.message(&stored.header.chat, &stored.header.id).await {
            Ok(fresh) => { let _ = self.events.send(ServiceEvent::arrival(&fresh)); None }
            Err(error) => Some(format!("Album message was written to transport, but its local row could not be refreshed: {error}")),
        }
    }

    async fn album_before_error(&self, stored: &StoredMessage, error: impl std::fmt::Display) -> String {
        match self.discard_album_attempt(stored).await {
            Some(warning) => format!("{error}; {warning}"),
            None => error.to_string(),
        }
    }

    async fn discard_album_attempt(&self, stored: &StoredMessage) -> Option<String> {
        let path = match self.store.discard_unsent_album_attempt(stored).await {
            Ok(Some(path)) => PathBuf::from(path),
            Ok(None) => return None,
            Err(error) => return Some(format!("Unsent album attempt could not be discarded: {error}")),
        };
        if stored.media.path.as_deref().map(Path::new) != Some(path.as_path())
            || path.file_stem().and_then(|stem| stem.to_str()) != Some(stored.header.id.as_str())
            || self.media_dir().as_deref() != path.parent() {
            return Some("Unsent album copy was retained because its generated path changed.".into());
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(format!("Unsent album copy could not be removed: {error}")),
        }
    }
}

struct AlbumSendAttempt {
    id: String,
    warnings: Vec<String>,
    stop: Option<String>,
}

enum AlbumSendFailure { Before(String), Uncertain(String, String) }

impl AlbumSendFailure {
    fn before(error: impl std::fmt::Display) -> Self { Self::Before(error.to_string()) }
}

async fn send_album_sequence<Step, Attempt>(count: usize, mut step: Step, result: &mut AlbumSendResult)
where Step: FnMut(usize) -> Attempt,
      Attempt: std::future::Future<Output = std::result::Result<AlbumSendAttempt, AlbumSendFailure>>,
{
    for index in 0..count {
        match step(index).await {
            Ok(sent) => {
                result.sent(index, sent.id);
                result.warnings.extend(sent.warnings);
                if let Some(error) = sent.stop { result.stopped(index + 1, error); return; }
            }
            Err(AlbumSendFailure::Before(error)) => { result.stopped(index, error); return; }
            Err(AlbumSendFailure::Uncertain(id, error)) => { result.uncertain(index, id, error); return; }
        }
    }
}

fn failed_before_transport(error: &whatsapp_rust::SendError) -> bool {
    matches!(error, whatsapp_rust::SendError::NotLoggedIn | whatsapp_rust::SendError::InvalidRequest(_)
        | whatsapp_rust::SendError::NoRecipientDevice(_) | whatsapp_rust::SendError::PrimaryDeviceRejected(_))
}

fn pending_album_row(stored: &mut StoredMessage) {
    stored.local.status = Some("pending".into());
}

async fn restore_self_album_delivery(store: &StoreWorker, stored: &StoredMessage, to_self: bool) -> Result<()> {
    if to_self { store.set_delivery_state(&stored.header.chat, &stored.header.id, "delivered").await?; }
    Ok(())
}

fn album_counts(items: &[AlbumMediaInput], continuation: bool) -> Result<(u32, u32)> {
    anyhow::ensure!(((if continuation { 1 } else { 2 })..=MAX_ALBUM_ITEMS).contains(&items.len()),
        "New albums require 2 to 8 items; a continuation accepts 1 to 8 (application staging bound).");
    let (mut images, mut videos) = (0, 0);
    for item in items {
        anyhow::ensure!(!item.name.is_empty() && item.name.len() <= 1024
            && item.caption.as_ref().is_none_or(|text| text.len() <= MAX_CAPTION_BYTES)
            && item.progress.as_ref().is_none_or(|token| !token.is_empty() && token.len() <= 1024), "Invalid album item metadata.");
        let extension = file_extension(&item.name);
        let (_, kind) = media_kind_for(&extension);
        match kind {
            "image" if extension != "gif" => images += 1,
            "video" => videos += 1,
            _ => anyhow::bail!("Albums support ordinary photos and videos only."),
        }
    }
    Ok((images, videos))
}

fn continuation_key(parent: &StoredMessage, chat: &str) -> Result<wa::MessageKey> {
    anyhow::ensure!(parent.header.chat == chat && parent.header.from_me && !parent.header.id.is_empty()
        && !parent.local.deleted && !parent.local.revoked && !parent.spoiler && parent.media.once_kind.is_none()
        && parent.media.kind.as_deref() == Some("album") && parent.system.kind.is_none()
        && parent.album.as_ref().is_some_and(|album| album.parent_id.is_none()), "Album continuation parent is unavailable or private.");
    Ok(wa::MessageKey { remote_jid: Some(parent.header.chat.clone()), from_me: Some(true),
        id: Some(parent.header.id.clone()), participant: None })
}

fn album_target(chat: &str, reply: Option<&(String, String, String)>, mentions: &[String]) -> Result<Jid> {
    anyhow::ensure!(chat.len() <= 256, "Album destination is too long.");
    let target: Jid = chat.parse()?;
    anyhow::ensure!(!target.user.is_empty() && (target.is_pn() || target.is_lid() || target.is_group())
        && target.device == 0 && target.agent == 0 && target.integrator == 0, "Choose a contact or group album destination.");
    anyhow::ensure!(mentions.len() <= 256, "Album exceeds the application mention bound.");
    for mention in mentions {
        anyhow::ensure!(mention.len() <= 256, "Album mention is too long.");
        let jid: Jid = mention.parse()?;
        anyhow::ensure!(!jid.user.is_empty() && (jid.is_pn() || jid.is_lid()) && jid.device == 0
            && jid.agent == 0 && jid.integrator == 0, "Invalid album mention.");
    }
    if let Some((id, sender, text)) = reply {
        anyhow::ensure!(!id.is_empty() && id.len() <= 256 && sender.len() <= 256 && text.len() <= MAX_CAPTION_BYTES,
            "Invalid album reply target.");
        let _: Jid = sender.parse()?;
    }
    Ok(target)
}

fn album_message(images: u32, videos: u32, context: Option<Box<wa::ContextInfo>>) -> wa::Message {
    wa::Message { album_message: buffa::MessageField::some(wa::message::AlbumMessage {
        expected_image_count: Some(images), expected_video_count: Some(videos),
        context_info: context.map(buffa::MessageField::from_box).unwrap_or_default(), ..Default::default()
    }), ..Default::default() }
}

#[cfg(test)]
#[path = "albums_tests.rs"]
mod tests;
