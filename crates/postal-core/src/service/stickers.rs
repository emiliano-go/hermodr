//! Sticker library: packs received from the phone, favourites and recents, and
//! the two-way sync that keeps them in step with the other linked devices.
//!
//! The store is upsert-only: sync never deletes a sticker row or file. A remote
//! unfavorite clears a flag; a removed recent clears its timestamp.

use super::*;

/// The sticker library as the picker and manager see it.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StickerLibrary {
    pub packs: Vec<StickerPack>,
    pub favorites: Vec<Sticker>,
    pub recent: Vec<Sticker>,
    pub catalog_complete: bool,
}

/// What a resync changed, for the UI to report.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StickerResyncReport {
    pub packs: usize,
    pub stickers: usize,
    pub known_packs: usize,
    pub packs_changed: usize,
    pub stickers_changed: usize,
    pub skipped_stickers: usize,
    pub app_state_synced: bool,
    pub app_state_retryable: bool,
    pub app_state_fatal: bool,
    pub app_state_error: Option<String>,
    pub pack_failures: Vec<StickerPackFailure>,
    pub mirror_verified: bool,
    pub catalog_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StickerPackFailure {
    pub pack_id: String,
    pub error: String,
}

#[derive(Debug, Default)]
struct StickerPackRefresh {
    stickers: usize,
    pack_changed: bool,
    stickers_changed: usize,
    skipped: usize,
}

fn sticker_hash(filehash: &str) -> Result<[u8; 32]> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(filehash)?;
    anyhow::ensure!(
        base64::engine::general_purpose::STANDARD.encode(&bytes) == filehash,
        "Invalid sticker hash encoding."
    );
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Sticker hash must contain 32 bytes."))
}

fn sticker_filename(filehash: &str) -> Result<String> {
    use std::fmt::Write as _;
    let mut name = String::with_capacity(69);
    for byte in sticker_hash(filehash)? {
        write!(&mut name, "{byte:02x}")?;
    }
    name.push_str(".webp");
    Ok(name)
}

fn now_ms() -> i64 {
    whatsapp_rust::wacore::time::now_millis()
}

fn pushed_result(result: Result<()>, change: &str) -> Result<()> {
    result.map_err(|_| {
        anyhow::anyhow!(
            "{change} saved locally; phone sync request failed. Remote outcome is unknown."
        )
    })
}

fn favorite_value(
    action: wa::sync_action_value::StickerAction,
    timestamp_ms: i64,
) -> wa::SyncActionValue {
    wa::SyncActionValue {
        sticker_action: buffa::MessageField::some(action),
        timestamp: Some(timestamp_ms),
        ..Default::default()
    }
}

fn recent_removal_value(timestamp_ms: i64, threshold_ms: Option<i64>) -> wa::SyncActionValue {
    wa::SyncActionValue {
        remove_recent_sticker_action: buffa::MessageField::some(
            wa::sync_action_value::RemoveRecentStickerAction {
                last_sticker_sent_ts: threshold_ms,
            },
        ),
        timestamp: Some(timestamp_ms),
        ..Default::default()
    }
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
fn sticker_id_from_locator(
    locator: &[u8],
) -> Option<(String, bool, bool, Vec<String>, Option<i64>)> {
    let message = <wa::Message as buffa::Message>::decode(&mut locator.as_ref()).ok()?;
    let decoded = decoded_message(&message);
    if decoded.spoiler || decoded.view_once {
        return None;
    }
    let sticker = decoded.message.sticker_message.as_option()?;
    let sha = sticker.file_sha256.as_deref()?;
    let emojis = sticker
        .emojis
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    if sha.len() != 32 {
        return None;
    }
    Some((
        filehash_of_hash(sha),
        sticker.is_animated.unwrap_or(false),
        sticker.is_lottie.unwrap_or(false),
        emojis,
        sticker.sticker_sent_ts.filter(|at| *at > 0),
    ))
}

/// Upserts the sticker a stored message carries, keyed by its filehash, so
/// favorites and recents from the phone have a row to land on.
pub(super) async fn record_sticker(store: &StoreWorker, message: &StoredMessage) -> Result<bool> {
    if message.media.kind.as_deref() != Some("sticker")
        || message.system.kind.is_some()
        || message.spoiler
        || message.local.revoked
        || message.local.deleted
        || message.media.once_kind.is_some()
    {
        return Ok(false);
    }
    let Some(locator) = message.media.locator.as_deref() else {
        return Ok(false);
    };
    let Some((filehash, animated, lottie, emojis, sent_ms)) = sticker_id_from_locator(locator)
    else {
        return Ok(false);
    };
    let mut dirty = store
        .upsert_sticker_changed(Sticker {
            filehash: filehash.clone(),
            path: message.media.path.clone(),
            animated,
            lottie,
            emojis,
            updated_at: unix_now(),
            ..Default::default()
        })
        .await?;
    dirty |= store
        .set_sticker_locator_changed(filehash.clone(), locator.to_vec())
        .await?;
    if message.header.from_me {
        if let Some(sent_ms) = sent_ms {
            dirty |= store.observe_sticker_recent(filehash, sent_ms).await?;
        }
    }
    Ok(dirty)
}

impl WhatsAppService {
    /// Packs, favourites and recents, for the picker and the sticker manager.
    pub async fn sticker_library(&self) -> Result<StickerLibrary> {
        Ok(StickerLibrary {
            packs: self.store.sticker_packs().await?,
            favorites: self.store.favorite_stickers().await?,
            recent: self.store.recent_stickers().await?,
            catalog_complete: false,
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
        self.store
            .set_sticker_path(filehash.clone(), path.to_string(), unix_now())
            .await?;
        self.set_sticker_favorite(&filehash, favorite).await
    }

    /// Favourites or unfavourites a sticker and tells the linked devices.
    pub async fn set_sticker_favorite(&self, filehash: &str, favorite: bool) -> Result<()> {
        sticker_hash(filehash)?;
        let at_ms = now_ms();
        self.store
            .observe_sticker_favorite(filehash.to_string(), favorite, at_ms)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Could not save sticker favorite locally. No phone sync request was sent."
                )
            })?;
        let pushed = self.push_sticker_favorite(filehash, favorite, at_ms).await;
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
            packs: false,
            favorites: true,
            recents: false,
        });
        pushed_result(pushed, "Sticker favorite")
    }

    /// Marks a sticker recently sent, or removes it from recents.
    pub async fn set_sticker_recent(&self, filehash: &str, recent: bool) -> Result<()> {
        sticker_hash(filehash)?;
        anyhow::ensure!(!recent,"Adding to phone recents requires sending a sticker. No recent-add app-state action is supported.");
        let at_ms = now_ms();
        let threshold = self
            .store
            .sticker_recent_sent_ms(filehash.to_string())
            .await?;
        self.store
            .remove_sticker_recent(filehash.to_string(), at_ms, threshold)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Could not save recent removal locally. No phone sync request was sent."
                )
            })?;
        let pushed = self
            .push_sticker_remove_recent(filehash, at_ms, threshold)
            .await;
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
            packs: false,
            favorites: false,
            recents: true,
        });
        pushed_result(pushed, "Recent removal")
    }

    /// The media fields a favorite push carries, from the sticker's stored
    /// locator, so a device without the file can still fetch it.
    async fn sticker_action(
        &self,
        filehash: &str,
        is_favorite: bool,
    ) -> Result<wa::sync_action_value::StickerAction> {
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

    async fn push_sticker_favorite(
        &self,
        filehash: &str,
        favorite: bool,
        at_ms: i64,
    ) -> Result<()> {
        let value = favorite_value(self.sticker_action(filehash, favorite).await?, at_ms);
        self.client
            .send_app_state_action(
                &whatsapp_rust::schemas::FAVORITE_STICKER,
                &[filehash],
                &value,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    async fn push_sticker_remove_recent(
        &self,
        filehash: &str,
        at_ms: i64,
        threshold_ms: Option<i64>,
    ) -> Result<()> {
        let value = recent_removal_value(at_ms, threshold_ms);
        self.client
            .send_app_state_action(
                &whatsapp_rust::schemas::REMOVE_RECENT_STICKER,
                &[filehash],
                &value,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Fetches a pack's contents and upserts them, downloading nothing yet.
    pub async fn fetch_sticker_pack(&self, pack_id: &str) -> Result<usize> {
        let mut refreshed = StickerPackRefresh::default();
        self.refresh_sticker_pack(pack_id, &mut refreshed).await?;
        Ok(refreshed.stickers)
    }

    async fn refresh_sticker_pack(
        &self,
        pack_id: &str,
        refreshed: &mut StickerPackRefresh,
    ) -> Result<()> {
        let pack = self.client.fetch_sticker_pack(pack_id, "en").await?;
        let now = unix_now();
        refreshed.pack_changed = self
            .store
            .upsert_sticker_pack_changed(StickerPack {
                pack_id: pack_id.to_string(),
                name: pack.name.clone(),
                publisher: pack.publisher.clone(),
                origin: Some("fetched".into()),
                updated_at: now,
                ..Default::default()
            })
            .await?;
        for item in pack.stickers {
            let Some(hash) = item.file_hash.as_deref().filter(|hash| hash.len() == 32) else {
                refreshed.skipped += 1;
                continue;
            };
            // The pack lists the sticker's shape; the file downloads on demand.
            let sticker = Sticker {
                filehash: filehash_of_hash(hash),
                pack_id: Some(pack_id.to_string()),
                animated: pack.animated != 0,
                lottie: pack.lottie != 0
                    || item.mimetype.as_deref() == Some("application/lottie+zip"),
                emojis: item.emojis.clone(),
                updated_at: now,
                ..Default::default()
            };
            let metadata_changed = self.store.upsert_sticker_changed(sticker).await?;
            refreshed.stickers_changed += usize::from(metadata_changed);
            let locator_changed = self
                .store
                .set_sticker_locator_changed(filehash_of_hash(hash), item_to_locator(&item)?)
                .await?;
            refreshed.stickers_changed += usize::from(locator_changed && !metadata_changed);
            refreshed.stickers += 1;
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
            packs: true,
            favorites: false,
            recents: false,
        });
        Ok(())
    }

    /// Downloads a pack sticker to the media folder, returning its path.
    pub async fn download_sticker(&self, filehash: &str) -> Result<String> {
        let filename = sticker_filename(filehash)?;
        let Some(sticker) = self.store.sticker(filehash.to_string()).await? else {
            anyhow::bail!("no such sticker");
        };
        anyhow::ensure!(
            !sticker.lottie,
            "Lottie sticker rendering and sending are not supported."
        );
        if let Some(path) = sticker
            .path
            .as_deref()
            .filter(|p| std::path::Path::new(p).is_file())
        {
            return Ok(path.to_string());
        }
        let locator = self.store.sticker_locator(filehash.to_string()).await?;
        let locator =
            locator.ok_or_else(|| anyhow::anyhow!("the sticker has no downloadable reference"))?;
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
        let dir = self
            .media_dir()
            .ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let dir = dir.join("stickers").join("library");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(filename);
        tokio::fs::write(&path, &data).await?;
        let path = path.to_string_lossy().into_owned();
        self.store
            .set_sticker_path(filehash.to_string(), path.clone(), unix_now())
            .await?;
        Ok(path)
    }

    /// Reconciles the sticker library with the phone: resync the app-state
    /// collection, then re-fetch every known pack. Upsert-only.
    pub async fn resync_stickers(&self) -> Result<StickerResyncReport> {
        let packs = self.store.sticker_packs().await?;
        let mut changed = StickerResyncReport {
            known_packs: packs.len(),
            ..Default::default()
        };
        let report = self
            .client
            .resync_app_state(
                [whatsapp_rust::WAPatchName::RegularLow],
                whatsapp_rust::AppStateResyncMode::Snapshot,
            )
            .await;
        match report {
            Ok(report) => {
                changed.app_state_synced = report.all_synced();
                changed.app_state_retryable =
                    !report.retryable.is_empty() || !report.skipped.is_empty();
                changed.app_state_fatal = !report.fatal.is_empty();
                if !changed.app_state_synced {
                    changed.app_state_error =
                        Some("Sticker app-state collection remained unsynced.".into());
                }
            }
            Err(_) => changed.app_state_error = Some(
                "Sticker app-state snapshot request failed. Remote state could not be verified."
                    .into(),
            ),
        }
        for pack in packs {
            let mut refreshed = StickerPackRefresh::default();
            match self
                .refresh_sticker_pack(&pack.pack_id, &mut refreshed)
                .await
            {
                Ok(()) => changed.packs += 1,
                Err(_) => changed.pack_failures.push(StickerPackFailure {
                    pack_id: pack.pack_id,
                    error: "Pack refresh failed; confirmed local updates were preserved.".into(),
                }),
            }
            changed.stickers += refreshed.stickers;
            changed.packs_changed += usize::from(refreshed.pack_changed);
            changed.stickers_changed += refreshed.stickers_changed;
            changed.skipped_stickers += refreshed.skipped;
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
            packs: true,
            favorites: true,
            recents: true,
        });
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

/// A slim `StickerMessage` locator from a synced favorite action, so a sticker
/// the desktop never received can still be downloaded.
fn locator_from_sticker_action(
    filehash: &str,
    action: &wa::sync_action_value::StickerAction,
) -> Option<Vec<u8>> {
    let hash = sticker_hash(filehash).ok()?;
    if action.media_key.as_ref().is_none_or(|key| key.len() != 32)
        || action
            .file_enc_sha256
            .as_ref()
            .is_none_or(|hash| hash.len() != 32)
        || action
            .direct_path
            .as_ref()
            .is_none_or(|path| path.is_empty())
    {
        return None;
    }
    let message = wa::Message {
        sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
            url: action.url.clone(),
            direct_path: action.direct_path.clone(),
            media_key: action.media_key.clone(),
            file_sha256: Some(hash.to_vec()),
            file_enc_sha256: action.file_enc_sha256.clone(),
            file_length: action.file_length,
            mimetype: action.mimetype.clone(),
            width: action.width,
            height: action.height,
            is_lottie: action.is_lottie,
            ..Default::default()
        }),
        ..Default::default()
    };
    Some(media_locator(&message))
}

/// A slim `StickerMessage` locator from a `StickerMetadata` entry.
fn locator_from_sticker_metadata(sticker: &wa::StickerMetadata) -> Option<Vec<u8>> {
    if sticker.media_key.as_ref().is_none_or(|key| key.len() != 32)
        || sticker
            .file_enc_sha256
            .as_ref()
            .is_none_or(|hash| hash.len() != 32)
        || sticker
            .file_sha256
            .as_ref()
            .is_none_or(|hash| hash.len() != 32)
        || sticker
            .direct_path
            .as_ref()
            .is_none_or(|path| path.is_empty())
    {
        return None;
    }
    let message = wa::Message {
        sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
            url: sticker.url.clone(),
            direct_path: sticker.direct_path.clone(),
            media_key: sticker.media_key.clone(),
            file_sha256: sticker.file_sha256.clone(),
            file_enc_sha256: sticker.file_enc_sha256.clone(),
            file_length: sticker.file_length,
            mimetype: sticker.mimetype.clone(),
            width: sticker.width,
            height: sticker.height,
            is_lottie: sticker.is_lottie,
            ..Default::default()
        }),
        ..Default::default()
    };
    Some(media_locator(&message))
}

/// A received `StickerPackMessage` as a pack row. Its listed stickers carry only
/// file names, not hashes, so their rows wait for `fetch_sticker_pack`.
pub(super) fn pack_from_message(message: &wa::Message) -> Option<StickerPack> {
    let decoded = decoded_message(message);
    if decoded.spoiler || decoded.view_once {
        return None;
    }
    let pack = decoded.message.sticker_pack_message.as_option()?;
    let pack_id = pack.sticker_pack_id.clone()?;
    if pack_id.is_empty() {
        return None;
    }
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
    /// A favorite change made on another linked device. Its action carries the
    /// media fields, so the sticker can be downloaded even if never received.
    pub(super) async fn on_sticker_favorite(
        &self,
        filehash: &str,
        favorite: bool,
        timestamp_ms: i64,
        action: &wa::sync_action_value::StickerAction,
    ) {
        if sticker_hash(filehash).is_err() {
            log::warn!("Ignoring malformed synced sticker hash.");
            return;
        }
        let accepted = match self
            .store
            .observe_sticker_favorite(filehash.to_owned(), favorite, timestamp_ms)
            .await
        {
            Ok(accepted) => accepted,
            Err(_) => {
                log::warn!("Could not record synced sticker favorite.");
                return;
            }
        };
        if !accepted {
            return;
        }
        if let Err(e) = self
            .store
            .upsert_sticker(Sticker {
                filehash: filehash.to_string(),
                animated: false,
                lottie: action.is_lottie.unwrap_or(false),
                updated_at: timestamp_ms / 1000,
                ..Default::default()
            })
            .await
        {
            log::warn!("could not record favorite sticker {filehash}: {e}");
        }
        if let Some(locator) = locator_from_sticker_action(filehash, action) {
            if let Err(e) = self
                .store
                .set_sticker_locator(filehash.to_string(), locator)
                .await
            {
                log::warn!("could not record favorite sticker locator {filehash}: {e}");
            }
        }
        let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
            packs: false,
            favorites: true,
            recents: false,
        });
    }

    /// A recent-sticker removal made on another linked device.
    pub(super) async fn on_sticker_recent_removed(
        &self,
        filehash: &str,
        timestamp_ms: i64,
        threshold_ms: Option<i64>,
    ) {
        if sticker_hash(filehash).is_err() {
            log::warn!("Ignoring malformed synced sticker hash.");
            return;
        }
        if self
            .store
            .remove_sticker_recent(filehash.to_string(), timestamp_ms, threshold_ms)
            .await
            .observed()
            == Some(true)
        {
            let _ = self.events.send(ServiceEvent::StickerLibraryChanged {
                packs: false,
                favorites: false,
                recents: true,
            });
        }
    }

    /// The phone's recent stickers, carried by the initial history sync.
    pub(super) async fn seed_recent_stickers(
        &self,
        store: &StoreWorker,
        recent: &[wa::StickerMetadata],
    ) -> Result<bool> {
        let mut dirty = false;
        for sticker in recent {
            let Some(filehash) = sticker
                .file_sha256
                .as_deref()
                .filter(|hash| hash.len() == 32)
                .map(filehash_of_hash)
            else {
                continue;
            };
            dirty |= store
                .upsert_sticker_changed(Sticker {
                    filehash: filehash.clone(),
                    animated: false,
                    lottie: sticker.is_lottie.unwrap_or(false),
                    updated_at: unix_now(),
                    ..Default::default()
                })
                .await?;
            if let Some(at_ms) = sticker.last_sticker_sent_ts.filter(|at| *at > 0) {
                dirty |= store
                    .observe_sticker_recent(filehash.clone(), at_ms)
                    .await?;
            }
            if let Some(locator) = locator_from_sticker_metadata(sticker) {
                dirty |= store.set_sticker_locator_changed(filehash, locator).await?;
            }
        }
        Ok(dirty)
    }

    /// A shared sticker pack: record it, upsert-only. Its stickers load when the
    /// pack is opened, from `fetch_sticker_pack`.
    pub(super) async fn on_sticker_pack(
        &self,
        store: &StoreWorker,
        message: &wa::Message,
    ) -> Result<bool> {
        if let Some(pack) = pack_from_message(message) {
            return store.upsert_sticker_pack_changed(pack).await;
        }
        Ok(false)
    }
}

#[cfg(test)]
#[path = "stickers_tests.rs"]
mod tests;
