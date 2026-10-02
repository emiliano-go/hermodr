use super::*;
use super::media_files::TemporaryFile;

#[cfg(test)]
#[path = "media_download_tests.rs"]
mod tests;

/// Downloads a stored message's media from its locator and records the file.
///
/// A failed or corrupt download asks the sender's phone to upload the file
/// again, once per call, and keeps the new location for later attempts.
pub(super) async fn fetch_media(client: &Client, store: &StoreWorker, dir: &Path, chat: &str, id: &str) -> Result<StoredMessage> {
    let row = store.message(chat, id).await?;
    if row.media.kind.as_deref() == Some("music") && row.media.thumb.is_some() { return Ok(row); }
    let message = store
        .media_ref_for(chat, id).await?
        .map(|bytes| <wa::Message as buffa::Message>::decode(&mut bytes.as_slice()))
        .transpose()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let once = store.is_view_once(chat, id).await?;
    // A view-once has no address of its own, or reached this device only as a
    // stub. A reply quoting it carries a complete copy, and that is the only
    // one the platform ever sends, so it is used before the sender's phone is
    // troubled with a reupload.
    if once && message.as_ref().is_none_or(|m| !has_direct_path(m)) {
        if let Some((copied, data)) = fetch_quoted_copy(client, store, dir, id).await? {
            log::debug!("recovered {id} {} from the copy inside a reply", copied.kind);
            let path = media_path(dir, id, &copied.extension())?;
            tokio::fs::rename(&data.path, &path).await?;
            store.set_media_path(chat, id, &path.to_string_lossy()).await?;
            store.set_once_kind(chat, id, copied.kind).await?;
            record_thumb(store, chat, id, copied.kind, &path).await?;
            return store.message(chat, id).await;
        }
    }
    let Some(message) = message else {
        if once {
            anyhow::bail!("this view-once was never sent to this device; open it on your phone");
        }
        if row.media.kind.as_deref() == Some("music") { anyhow::bail!("this music message has no supported artwork URI"); }
        anyhow::bail!("no stored media reference");
    };
    if let Some(music) = decoded_message(&message).message.music_message.as_option() {
        return fetch_music_artwork(store, chat, id, music, fetch_public_thumbnail).await;
    }
    fetch_stored_media(store, dir, chat, id, message,
        |media, writer| async move { download_file(client, &media, writer).await },
        |message| async move { reupload(client, store, chat, id, &message).await },
    ).await
}

async fn fetch_music_artwork<F>(store: &StoreWorker, chat: &str, id: &str, music: &wa::message::MusicMessage, fetch: F) -> Result<StoredMessage>
where F: FnOnce(&str) -> Option<Vec<u8>> + Send + 'static,
{
    let uri = music.artwork_uri.clone().filter(|uri| !uri.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("this music message has no supported artwork URI"))?;
    let thumb = tokio::task::spawn_blocking(move || fetch(&uri)).await?
        .ok_or_else(|| anyhow::anyhow!("music artwork could not be fetched from a public image URI"))?;
    store.set_media_thumb(chat, id, &thumb_uri(&thumb)).await?;
    store.message(chat, id).await
}

async fn fetch_stored_media<D, DF, R, RF>(
    store: &StoreWorker, dir: &Path, chat: &str, id: &str, mut message: wa::Message,
    download: D, reupload: R,
) -> Result<StoredMessage>
where
    D: Fn(MediaInfo, std::fs::File) -> DF,
    DF: std::future::Future<Output = Result<std::fs::File>>,
    R: FnOnce(wa::Message) -> RF,
    RF: std::future::Future<Output = Result<String>>,
{
    let media = detect_media(decoded_message(&message).message).ok_or_else(|| anyhow::anyhow!("message carries no media"))?;
    let kind = media.kind;
    let extension = media.extension();
    let path = media_path(dir, id, &extension)?;
    let (temporary, writer) = download_target(dir).await?;
    let started = std::time::Instant::now();
    let data = match download(media, writer).await {
        Ok(data) => data,
        Err(first) => {
            log::info!("download of {id} failed ({first:#}); asking the sender to upload it again");
            let path = reupload(message.clone()).await.map_err(|e| first.context(e))?;
            set_direct_path(&mut message, &path);
            store.set_media_ref(chat, id, &buffa::Message::encode_to_vec(&message)).await?;
            let media = detect_media(decoded_message(&message).message).ok_or_else(|| anyhow::anyhow!("message carries no media"))?;
            let writer = tokio::fs::OpenOptions::new().write(true).truncate(true).open(&temporary.path).await?.into_std().await;
            download(media, writer).await?
        }
    };
    log::debug!("downloaded {id} {kind} ({} KB) in {:?}", data.metadata()?.len() / 1024, started.elapsed());
    drop(data);
    tokio::fs::rename(&temporary.path, &path).await?;
    store.set_media_path(chat, id, &path.to_string_lossy()).await?;
    record_thumb(store, chat, id, kind, &path).await?;
    store.message(chat, id).await
}

/// Generates and records a preview when the stored row has none.
///
/// A view-once arrives without a thumbnail, so a copy the companion keeps has to
/// make its own; ordinary media already carries the sender's.
async fn record_thumb(store: &StoreWorker, chat: &str, id: &str, kind: &'static str, path: &Path) -> Result<()> {
    if store.message(chat, id).await?.media.thumb.is_some() {
        return Ok(());
    }
    let path = path.to_path_buf();
    if let Some(thumb) = tokio::task::spawn_blocking(move || super::media_codec::media_thumbnail_file(kind, &path)).await? {
        store.set_media_thumb(chat, id, &thumb_uri(&thumb)).await?;
    }
    Ok(())
}

/// Downloads the copy a reply to a view-once carries.
///
/// The copy arrives complete, with the address the view-once itself lacks, so
/// this is the only route to the media that does not need the sender's phone.
async fn fetch_quoted_copy(client: &Client, store: &StoreWorker, dir: &Path, id: &str) -> Result<Option<(MediaInfo, TemporaryFile)>> {
    let Some(source) = store.quote_source_for(id).await? else { return Ok(None) };
    let message = <wa::Message as buffa::Message>::decode(&mut source.locator.as_slice())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let media = detect_media(decoded_message(&message).message)
        .ok_or_else(|| anyhow::anyhow!("the copy inside the reply carries no media"))?;
    let started = std::time::Instant::now();
    let (data, writer) = download_target(dir).await?;
    drop(download_file(client, &media, writer).await?);
    log::info!("view-once {id}: took the copy inside reply {} in {:?}", source.id, started.elapsed());
    Ok(Some((media, data)))
}

/// Asks the sender's phone to upload a message's media again; the new direct path on success.
async fn reupload(client: &Client, store: &StoreWorker, chat: &str, id: &str, message: &wa::Message) -> Result<String> {
    let key = media_key(message).ok_or_else(|| anyhow::anyhow!("the message carries no media key"))?;
    let row = store.message(chat, id).await?;
    let chat_jid: Jid = chat.parse().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let sender: Option<Jid> = chat_jid.is_group().then(|| row.header.sender.parse().ok()).flatten();
    let request = whatsapp_rust::MediaReuploadRequest {
        msg_id: id,
        chat_jid: &chat_jid,
        media_key: &key,
        is_from_me: row.header.from_me,
        participant: sender.as_ref(),
    };
    match client.media_reupload().request(&request).await? {
        whatsapp_rust::MediaRetryResult::Success { direct_path } => Ok(direct_path),
        other => anyhow::bail!("the sender could not upload it again: {other:?}"),
    }
}

fn media_key(message: &wa::Message) -> Option<Vec<u8>> {
    let message = decoded_message(message).message;
    [
        message.image_message.as_option().and_then(|m| m.media_key.clone()),
        message.video_message.as_option().and_then(|m| m.media_key.clone()),
        message.ptv_message.as_option().and_then(|m| m.media_key.clone()),
        message.audio_message.as_option().and_then(|m| m.media_key.clone()),
        message.document_message.as_option().and_then(|m| m.media_key.clone()),
        message.sticker_message.as_option().and_then(|m| m.media_key.clone()),
    ]
    .into_iter()
    .flatten()
    .next()
}

/// Points a message's media at a freshly uploaded copy. The old URL is dropped
/// so the download builds its address from the new path.
pub(super) fn set_direct_path(message: &mut wa::Message, path: &str) {
    macro_rules! repoint {
        ($($field:ident),*) => {$(
            if let Some(m) = message.$field.as_option_mut() {
                m.direct_path = Some(path.to_string());
                m.url = None;
            }
        )*};
    }
    repoint!(image_message, video_message, ptv_message, audio_message, document_message, sticker_message);
    if let Some(inner) = message.device_sent_message.as_option_mut().and_then(|wrapper| wrapper.message.as_option_mut()) {
        set_direct_path(inner, path);
    }
    for wrapper in [message.ephemeral_message.as_option_mut(), message.view_once_message.as_option_mut(),
        message.view_once_message_v2.as_option_mut(), message.view_once_message_v2_extension.as_option_mut(),
        message.document_with_caption_message.as_option_mut(), message.edited_message.as_option_mut(),
        message.spoiler_message.as_option_mut()].into_iter().flatten() {
        if let Some(inner) = wrapper.message.as_option_mut() { set_direct_path(inner, path); }
    }
}

/// Whether a media message carries somewhere to fetch its bytes from.
///
/// A view-once arrives with its media key but no `direct_path`, so there is
/// nothing on the CDN to download and nothing to ask a reupload about.
pub(super) fn has_direct_path(message: &wa::Message) -> bool {
    let base = decoded_message(message).message;
    let url = |path: Option<String>| path.is_some_and(|p| !p.is_empty());
    url(base.image_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.video_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.ptv_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.audio_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.document_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.sticker_message.as_option().and_then(|m| m.direct_path.clone()))
}

/// What a recovered view-once copy is named after, so those files are told
/// apart from a message's own media and the prune only looks at these.
const QUOTE_FILE_PREFIX: &str = "quote-";

/// Deletes recovered view-once files that no stored message points at any more.
pub fn prune_quote_files(dir: Option<&Path>, store: &MessageStore) -> Result<usize> {
    let Some(dir) = dir else { return Ok(0) };
    let keep = store.quote_media_paths()?;
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let named = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(QUOTE_FILE_PREFIX));
        if named && !path.is_dir() && !keep.contains(&path.to_string_lossy().to_string()) {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Downloads the view-once a reply quotes and records it on every reply quoting
/// the same message.
///
/// There is no reupload fallback: asking a sender's phone to upload the file
/// again needs a message addressable in the chat it is asked about, and the
/// quoted view-once is not one.
pub(super) async fn fetch_quote_media(
    client: &Client,
    store: &StoreWorker,
    dir: &Path,
    chat: &str,
    id: &str,
) -> Result<StoredMessage> {
    let row = store.message(chat, id).await?;
    // The gate again, applied here rather than at store time, and with the same
    // answer the store recorded: the operator owns this client, so the owner
    // branch is the one that applies.
    if !row.quote.recoverable {
        anyhow::bail!("that view-once was not sent to this account, so there is no copy to take");
    }
    let Some(quoted) = row.quote.id.clone().filter(|q| !q.is_empty()) else {
        anyhow::bail!("the reply quotes nothing");
    };
    let Some(bytes) = row.quote.locator else {
        anyhow::bail!("the reply carries no copy of it");
    };
    let message = <wa::Message as buffa::Message>::decode(&mut bytes.as_slice())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let media = detect_media(decoded_message(&message).message)
        .ok_or_else(|| anyhow::anyhow!("the quoted message carries no media"))?;
    if !has_direct_path(&message) {
        anyhow::bail!("WhatsApp sent this device no place to fetch that view-once from; open it on your phone");
    }
    let started = std::time::Instant::now();
    let (data, writer) = download_target(dir).await?;
    let verified = download_file(client, &media, writer).await?;
    log::debug!(
        "recovered quoted view-once {quoted} {} ({} KB) in {:?}",
        media.kind,
        verified.metadata()?.len() / 1024,
        started.elapsed()
    );
    drop(verified);
    // Named after the quoted message, so every reply quoting the same view-once
    // shares one file and one download.
    let path = media_path(dir, &format!("{QUOTE_FILE_PREFIX}{quoted}"), &media.extension())?;
    tokio::fs::rename(&data.path, &path).await?;
    store.set_quote_media_path(chat, id, &path.to_string_lossy()).await?;
    store.message(chat, id).await
}

/// Fetches and decrypts a media submessage.
pub(super) async fn download_file(client: &Client, media: &MediaInfo, writer: std::fs::File) -> Result<std::fs::File> {
    tokio::time::timeout(Duration::from_secs(120), client.download_to_writer(media.downloadable.as_ref(), writer))
        .await
        .map_err(|_| anyhow::anyhow!("download timed out"))?
}

pub(super) async fn download_target(dir: &Path) -> Result<(TemporaryFile, std::fs::File)> {
    tokio::fs::create_dir_all(dir).await?;
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || TemporaryFile::download(&dir)).await?
}

pub(super) fn media_path(dir: &Path, id: &str, extension: &str) -> Result<PathBuf> {
    anyhow::ensure!(!id.contains(['/', '\\', ':', '\0']), "invalid media identifier");
    let name = format!("{id}.{extension}");
    let mut parts = Path::new(&name).components();
    anyhow::ensure!(matches!(parts.next(), Some(std::path::Component::Normal(_))) && parts.next().is_none(), "invalid media identifier");
    Ok(dir.join(name))
}
