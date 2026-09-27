use super::*;

/// Downloads a stored message's media from its locator and records the file.
///
/// A failed or corrupt download asks the sender's phone to upload the file
/// again, once per call, and keeps the new location for later attempts.
pub(super) async fn fetch_media(client: &Client, store: &MessageStore, dir: &Path, chat: &str, id: &str) -> Result<StoredMessage> {
    let message = store
        .media_ref_for(chat, id)?
        .map(|bytes| <wa::Message as buffa::Message>::decode(&mut bytes.as_slice()))
        .transpose()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let once = store.is_view_once(chat, id).unwrap_or(false);
    // A view-once has no address of its own, or reached this device only as a
    // stub. A reply quoting it carries a complete copy, and that is the only
    // one the platform ever sends, so it is used before the sender's phone is
    // troubled with a reupload.
    if once && message.as_ref().is_none_or(|m| !has_direct_path(m)) {
        if let Some((copied, data)) = fetch_quoted_copy(client, store, id).await? {
            log::debug!("recovered {id} {} ({} KB) from the copy inside a reply", copied.kind, data.len() / 1024);
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("{id}.{}", copied.extension()));
            std::fs::write(&path, &data)?;
            store.set_media_path(chat, id, &path.to_string_lossy())?;
            store.set_once_kind(chat, id, copied.kind)?;
            return store.message(chat, id);
        }
    }
    let Some(message) = message else {
        if once {
            anyhow::bail!("this view-once was never sent to this device; open it on your phone");
        }
        anyhow::bail!("no stored media reference");
    };
    let Some(mut media) = detect_media(&message) else {
        anyhow::bail!("message carries no media");
    };
    let started = std::time::Instant::now();
    let data = match download_bytes(client, &media).await {
        Ok(data) => data,
        Err(first) => {
            log::info!("download of {id} failed ({first:#}); asking the sender to upload it again");
            let mut message = message;
            let path = reupload(client, store, chat, id, &message).await.map_err(|e| first.context(e))?;
            set_direct_path(&mut message, &path);
            store.set_media_ref(chat, id, &buffa::Message::encode_to_vec(&message))?;
            media = detect_media(&message).ok_or_else(|| anyhow::anyhow!("message carries no media"))?;
            download_bytes(client, &media).await?
        }
    };
    log::debug!("downloaded {id} {} ({} KB) in {:?}", media.kind, data.len() / 1024, started.elapsed());
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.{}", id, media.extension()));
    std::fs::write(&path, &data)?;
    store.set_media_path(chat, id, &path.to_string_lossy())?;
    store.message(chat, id)
}

/// Downloads the copy a reply to a view-once carries.
///
/// The copy arrives complete, with the address the view-once itself lacks, so
/// this is the only route to the media that does not need the sender's phone.
async fn fetch_quoted_copy(client: &Client, store: &MessageStore, id: &str) -> Result<Option<(MediaInfo, Vec<u8>)>> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let Some(source) = store.quote_source_for(id)? else { return Ok(None) };
    let message = <wa::Message as buffa::Message>::decode(&mut source.locator.as_slice())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let media = detect_media(message.get_base_message())
        .ok_or_else(|| anyhow::anyhow!("the copy inside the reply carries no media"))?;
    let started = std::time::Instant::now();
    let data = download_bytes(client, &media).await?;
    log::info!("view-once {id}: took the copy inside reply {} ({} KB) in {:?}", source.id, data.len() / 1024, started.elapsed());
    Ok(Some((media, data)))
}

/// Asks the sender's phone to upload a message's media again; the new direct path on success.
async fn reupload(client: &Client, store: &MessageStore, chat: &str, id: &str, message: &wa::Message) -> Result<String> {
    let key = media_key(message).ok_or_else(|| anyhow::anyhow!("the message carries no media key"))?;
    let row = store.message(chat, id)?;
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
    [
        message.image_message.as_option().and_then(|m| m.media_key.clone()),
        message.video_message.as_option().and_then(|m| m.media_key.clone()),
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
    repoint!(image_message, video_message, audio_message, document_message, sticker_message);
}

/// Whether a media message carries somewhere to fetch its bytes from.
///
/// A view-once arrives with its media key but no `direct_path`, so there is
/// nothing on the CDN to download and nothing to ask a reupload about.
pub(super) fn has_direct_path(message: &wa::Message) -> bool {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    let url = |path: Option<String>| path.is_some_and(|p| !p.is_empty());
    url(base.image_message.as_option().and_then(|m| m.direct_path.clone()))
        || url(base.video_message.as_option().and_then(|m| m.direct_path.clone()))
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
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let named = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(QUOTE_FILE_PREFIX));
        if named && !path.is_dir() && !keep.contains(&path.to_string_lossy().to_string()) {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
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
    store: &MessageStore,
    dir: &Path,
    chat: &str,
    id: &str,
) -> Result<StoredMessage> {
    let row = store.message(chat, id)?;
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
    let media = detect_media(message.get_base_message())
        .ok_or_else(|| anyhow::anyhow!("the quoted message carries no media"))?;
    if !has_direct_path(message.get_base_message()) {
        anyhow::bail!("WhatsApp sent this device no place to fetch that view-once from; open it on your phone");
    }
    let started = std::time::Instant::now();
    let data = download_bytes(client, &media).await?;
    log::debug!(
        "recovered quoted view-once {quoted} {} ({} KB) in {:?}",
        media.kind,
        data.len() / 1024,
        started.elapsed()
    );
    std::fs::create_dir_all(dir)?;
    // Named after the quoted message, so every reply quoting the same view-once
    // shares one file and one download.
    let path = dir.join(format!("{QUOTE_FILE_PREFIX}{quoted}.{}", media.extension()));
    std::fs::write(&path, &data)?;
    store.set_quote_media_path(chat, id, &path.to_string_lossy())?;
    store.message(chat, id)
}

/// Fetches and decrypts a media submessage.
async fn download_bytes(client: &Client, media: &MediaInfo) -> Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(120), client.download(media.downloadable.as_ref()))
        .await
        .map_err(|_| anyhow::anyhow!("download timed out"))?
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
