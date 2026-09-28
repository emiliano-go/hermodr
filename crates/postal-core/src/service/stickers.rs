//! Sticker library: packs received from the phone, favourites and recents, and
//! the two-way sync that keeps them in step with the other linked devices.
//!
//! The store is upsert-only: sync never deletes a sticker row or file. A remote
//! unfavorite clears a flag; a removed recent clears its timestamp.

use super::*;

/// The sticker library as the picker and manager see it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StickerLibrary {
    pub packs: Vec<StickerPack>,
    pub favorites: Vec<Sticker>,
    pub recent: Vec<Sticker>,
}

/// What a resync changed, for the UI to report.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StickerResyncReport {
    pub packs: usize,
    pub stickers: usize,
}

/// The base64 SHA-256 app-state uses to index a sticker, from its raw hash.
pub(super) fn filehash_of_hash(sha256: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(sha256)
}

/// The filehash of a sticker file's bytes.
pub(super) fn filehash_of_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    filehash_of_hash(&Sha256::digest(bytes))
}

/// The id on a stored sticker message, as `media_ref` keeps it.
fn sticker_id_from_locator(locator: &[u8]) -> Option<(String, bool, Vec<String>)> {
    let message = <wa::Message as buffa::Message>::decode(&mut locator.as_ref()).ok()?;
    let sticker = message.get_base_message().sticker_message.as_option()?;
    let sha = sticker.file_sha256.as_deref()?;
    let emojis = sticker
        .emojis
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    Some((filehash_of_hash(sha), sticker.is_animated.unwrap_or(false), emojis))
}

/// Upserts the sticker a stored message carries, keyed by its filehash, so
/// favorites and recents from the phone have a row to land on.
pub(super) async fn record_sticker(store: &StoreWorker, message: &StoredMessage) -> Result<()> {
    let Some(locator) = message.media.locator.as_deref() else { return Ok(()) };
    let Some((filehash, animated, emojis)) = sticker_id_from_locator(locator) else { return Ok(()) };
    store
        .upsert_sticker(Sticker {
            filehash: filehash.clone(),
            path: message.media.path.clone(),
            animated,
            emojis,
            updated_at: unix_now(),
            ..Default::default()
        })
        .await?;
    store.set_sticker_locator(filehash, locator.to_vec()).await?;
    Ok(())
}

impl WhatsAppService {
    /// Packs, favourites and recents, for the picker and the sticker manager.
    pub async fn sticker_library(&self) -> Result<StickerLibrary> {
        Ok(StickerLibrary {
            packs: self.store.sticker_packs().await?,
            favorites: self.store.favorite_stickers().await?,
            recent: self.store.recent_stickers().await?,
        })
    }

    pub async fn stickers_in_pack(&self, pack_id: &str) -> Result<Vec<Sticker>> {
        self.store.stickers_in_pack(pack_id.to_string()).await
    }

    /// Favourites a sticker by its file path, creating its row first when it
    /// never came from a message (a saved or hand-made sticker).
    pub async fn favorite_sticker_path(&self, path: &str, favorite: bool) -> Result<()> {
        let bytes = tokio::fs::read(path).await?;
        let filehash = filehash_of_bytes(&bytes);
        self.store.set_sticker_path(filehash.clone(), path.to_string(), unix_now()).await?;
        self.set_sticker_favorite(&filehash, favorite).await
    }

    /// Favourites or unfavourites a sticker and tells the linked devices.
    pub async fn set_sticker_favorite(&self, filehash: &str, favorite: bool) -> Result<()> {        let now = unix_now();
        self.store.set_sticker_favorite(filehash.to_string(), favorite, now).await?;
        if let Err(e) = self.push_sticker_favorite(filehash, favorite).await {
            log::warn!("could not push sticker favorite for {filehash}: {e:#}");
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: false, favorites: true, recents: false });
        Ok(())
    }

    /// Marks a sticker recently sent, or removes it from recents.
    pub async fn set_sticker_recent(&self, filehash: &str, recent: bool) -> Result<()> {
        let now = unix_now();
        let at = recent.then_some(now);
        self.store.set_sticker_recent(filehash.to_string(), at, now).await?;
        if !recent {
            if let Err(e) = self.push_sticker_remove_recent(filehash).await {
                log::warn!("could not push sticker recent removal for {filehash}: {e:#}");
            }
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: false, favorites: false, recents: true });
        Ok(())
    }

    /// The media fields a favorite push carries, from the sticker's stored
    /// locator, so a device without the file can still fetch it.
    async fn sticker_action(&self, filehash: &str, is_favorite: bool) -> Result<wa::sync_action_value::StickerAction> {
        let locator = self.store.sticker_locator(filehash.to_string()).await?;
        let mut action = wa::sync_action_value::StickerAction {
            is_favorite: Some(is_favorite),
            ..Default::default()
        };
        if let Some(locator) = locator {
            if let Ok(message) = <wa::Message as buffa::Message>::decode(&mut locator.as_ref()) {
                if let Some(sticker) = message.get_base_message().sticker_message.as_option() {
                    action.url = sticker.url.clone();
                    action.direct_path = sticker.direct_path.clone();
                    action.media_key = sticker.media_key.clone();
                    action.file_enc_sha256 = sticker.file_enc_sha256.clone();
                    action.file_length = sticker.file_length;
                    action.mimetype = sticker.mimetype.clone();
                    action.height = sticker.height;
                    action.width = sticker.width;
                    action.is_lottie = sticker.is_lottie;
                }
            }
        }
        Ok(action)
    }

    async fn push_sticker_favorite(&self, filehash: &str, favorite: bool) -> Result<()> {
        let value = wa::SyncActionValue {
            sticker_action: buffa::MessageField::some(self.sticker_action(filehash, favorite).await?),
            timestamp: Some(unix_now()),
            ..Default::default()
        };
        self.client
            .send_app_state_action(&whatsapp_rust::schemas::FAVORITE_STICKER, &[filehash], &value)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    async fn push_sticker_remove_recent(&self, filehash: &str) -> Result<()> {
        let value = wa::SyncActionValue {
            remove_recent_sticker_action: buffa::MessageField::some(wa::sync_action_value::RemoveRecentStickerAction {
                last_sticker_sent_ts: Some(unix_now() * 1000),
            }),
            timestamp: Some(unix_now()),
            ..Default::default()
        };
        self.client
            .send_app_state_action(&whatsapp_rust::schemas::REMOVE_RECENT_STICKER, &[filehash], &value)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Fetches a pack's contents and upserts them, downloading nothing yet.
    pub async fn fetch_sticker_pack(&self, pack_id: &str) -> Result<usize> {
        let pack = self.client.fetch_sticker_pack(pack_id, "en").await?;
        let now = unix_now();
        self.store
            .upsert_sticker_pack(StickerPack {
                pack_id: pack_id.to_string(),
                name: pack.name.clone(),
                publisher: pack.publisher.clone(),
                origin: Some("fetched".into()),
                updated_at: now,
                ..Default::default()
            })
            .await?;
        let mut count = 0;
        for item in pack.stickers {
            let Some(hash) = item.file_hash.as_deref() else { continue };
            // The pack lists the sticker's shape; the file downloads on demand.
            let sticker = Sticker {
                filehash: filehash_of_hash(hash),
                pack_id: Some(pack_id.to_string()),
                animated: pack.animated != 0,
                lottie: pack.lottie != 0 || item.mimetype.as_deref() == Some("application/lottie+zip"),
                emojis: item.emojis.clone(),
                updated_at: now,
                ..Default::default()
            };
            self.store.upsert_sticker(sticker).await?;
            self.store
                .set_sticker_locator(
                    filehash_of_hash(hash),
                    item_to_locator(&item)?,
                )
                .await?;
            count += 1;
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: true, favorites: false, recents: false });
        Ok(count)
    }

    /// Downloads a pack sticker to the media folder, returning its path.
    pub async fn download_sticker(&self, filehash: &str) -> Result<String> {
        let Some(sticker) = self.store.sticker(filehash.to_string()).await? else {
            anyhow::bail!("no such sticker");
        };
        if let Some(path) = sticker.path.as_deref().filter(|p| std::path::Path::new(p).is_file()) {
            return Ok(path.to_string());
        }
        let locator = self.store.sticker_locator(filehash.to_string()).await?;
        let locator = locator.ok_or_else(|| anyhow::anyhow!("the sticker has no downloadable reference"))?;
        let message = <wa::Message as buffa::Message>::decode(&mut locator.as_ref())?;
        let media = detect_media(message.get_base_message())
            .ok_or_else(|| anyhow::anyhow!("the sticker reference carries no media"))?;
        let data = tokio::time::timeout(
            Duration::from_secs(120),
            self.client.download(media.downloadable.as_ref()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("sticker download timed out"))?
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let dir = self.media_dir().ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let dir = dir.join("stickers");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{filehash}.webp"));
        tokio::fs::write(&path, &data).await?;
        let path = path.to_string_lossy().into_owned();
        self.store.set_sticker_path(filehash.to_string(), path.clone(), unix_now()).await?;
        Ok(path)
    }

    /// Reconciles the sticker library with the phone: resync the app-state
    /// collection, then re-fetch every known pack. Upsert-only.
    pub async fn resync_stickers(&self) -> Result<StickerResyncReport> {
        let report = self
            .client
            .resync_app_state(
                [whatsapp_rust::WAPatchName::RegularLow],
                whatsapp_rust::AppStateResyncMode::Incremental,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if !report.all_synced() {
            log::warn!("sticker app-state resync still stale");
        }
        let packs = self.store.sticker_packs().await?;
        let mut changed = StickerResyncReport::default();
        for pack in packs {
            match self.fetch_sticker_pack(&pack.pack_id).await {
                Ok(stickers) => {
                    changed.packs += 1;
                    changed.stickers += stickers;
                }
                Err(e) => log::warn!("could not refresh sticker pack {}: {e:#}", pack.pack_id),
            }
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: true, favorites: true, recents: true });
        Ok(changed)
    }
}

/// A slim `StickerMessage` locator for a single pack item, so `download_sticker`
/// can fetch it with the ordinary media path.
fn item_to_locator(item: &whatsapp_rust::sticker_pack::StickerPackItem) -> Result<Vec<u8>> {
    let message = wa::Message {
        sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
            url: item.url.clone(),
            direct_path: item.direct_path.clone(),
            media_key: item.media_key.clone(),
            file_sha256: item.file_hash.clone(),
            file_enc_sha256: item.enc_file_hash.clone(),
            file_length: item.file_size,
            mimetype: item.mimetype.clone(),
            width: item.width,
            height: item.height,
            ..Default::default()
        }),
        ..Default::default()
    };
    Ok(media_locator(&message))
}

/// A received `StickerPackMessage` as a pack row. Its listed stickers carry only
/// file names, not hashes, so their rows wait for `fetch_sticker_pack`.
pub(super) fn pack_from_message(message: &wa::Message) -> Option<StickerPack> {
    let pack = message.get_base_message().sticker_pack_message.as_option()?;
    let pack_id = pack.sticker_pack_id.clone()?;
    Some(StickerPack {
        pack_id,
        name: pack.name.clone(),
        publisher: pack.publisher.clone(),
        origin: pack.sticker_pack_origin.map(|o| format!("{o:?}")),
        updated_at: unix_now(),
        ..Default::default()
    })
}

impl Inbound {
    /// A favorite change made on another linked device.
    pub(super) async fn on_sticker_favorite(&self, filehash: &str, favorite: bool, timestamp: i64) {
        self.store
            .set_sticker_favorite(filehash.to_string(), favorite, timestamp)
            .await
            .logged();
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: false, favorites: true, recents: false });
    }

    /// A recent-sticker removal made on another linked device.
    pub(super) async fn on_sticker_recent_removed(&self, filehash: &str, timestamp: i64) {
        self.store
            .set_sticker_recent(filehash.to_string(), None, timestamp)
            .await
            .logged();
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: false, favorites: false, recents: true });
    }

    /// A shared sticker pack: record it, upsert-only. Its stickers load when the
    /// pack is opened, from `fetch_sticker_pack`.
    pub(super) async fn on_sticker_pack(&self, message: &wa::Message) {
        if let Some(pack) = pack_from_message(message) {
            if let Err(e) = self.store.upsert_sticker_pack(pack).await {
                log::warn!("could not record a sticker pack: {e}");
            }
            let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: true, favorites: false, recents: false });
        }
    }
}