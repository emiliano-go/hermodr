//! Our own profile and presence, and other chats' pictures.

use super::*;
use crate::message_ref::MessageRef;
use std::{collections::VecDeque, future::Future};

const PRESENCE_CAP: usize = 8;

#[derive(Default)]
pub(super) struct PresenceWatches {
    recent: VecDeque<Jid>,
    active: Option<Jid>,
}

async fn watch_presence_lru<S, U, SF, UF>(tracked: &tokio::sync::Mutex<PresenceWatches>, jid: Option<Jid>, active: bool, mut subscribe: S, mut unsubscribe: U) -> Result<()>
where S: FnMut(Jid) -> SF, U: FnMut(Jid) -> UF,
    SF: Future<Output = Result<()>>, UF: Future<Output = Result<()>>,
{
    let mut tracked = tracked.lock().await;
    if active && tracked.active.as_ref() != jid.as_ref() {
        if let Some(previous) = tracked.active.clone() {
            unsubscribe(previous.clone()).await?;
            tracked.recent.retain(|saved| saved != &previous);
            tracked.active = None;
        }
    }
    let Some(jid) = jid else { return Ok(()); };
    if let Some(index) = tracked.recent.iter().position(|saved| *saved == jid) {
        let saved = tracked.recent.remove(index).unwrap();
        tracked.recent.push_back(saved);
        if active { tracked.active = Some(jid); }
        return Ok(());
    }
    if tracked.recent.len() == PRESENCE_CAP {
        let index = tracked.recent.iter().position(|saved| tracked.active.as_ref() != Some(saved)).unwrap();
        let oldest = tracked.recent[index].clone();
        unsubscribe(oldest).await?;
        tracked.recent.remove(index);
    }
    subscribe(jid.clone()).await?;
    tracked.recent.push_back(jid.clone());
    if active { tracked.active = Some(jid); }
    Ok(())
}

impl WhatsAppService {
    /// Tells the chat we are typing, or that we stopped.
    pub async fn send_typing(&self, chat: &str, typing: bool) -> Result<()> {
        let jid = broadcast_lists::writable_target(chat)?;
        let chatstate = self.client.chatstate();
        let sent = if typing {
            chatstate.send_composing(&jid).await
        } else {
            chatstate.send_paused(&jid).await
        };
        sent.map_err(anyhow::Error::from)
    }

    /// Marks us online or away, as WhatsApp Web does on window focus. Typing
    /// indicators only arrive while we are online.
    pub async fn set_online(&self, online: bool) -> Result<()> {
        let presence = self.client.presence();
        let set = if online {
            presence.set_available().await
        } else {
            presence.set_unavailable().await
        };
        set.map_err(anyhow::Error::from)
    }

    /// Subscribes to a contact's presence, which one-to-one typing needs.
    pub async fn watch_presence(&self, jid: Option<&str>, active: bool) -> Result<()> {
        let jid = jid.map(|jid| jid.parse::<Jid>().map(|parsed| parsed.to_non_ad())).transpose()?;
        watch_presence_lru(&self.presence_watches, jid, active,
            |jid| async move { self.client.presence().subscribe(jid).await.map_err(anyhow::Error::from) },
            |jid| async move { self.client.presence().unsubscribe(&jid).await.map_err(anyhow::Error::from) },
        ).await
    }

    /// Our own name, about text and privacy settings.
    pub async fn profile(&self) -> Result<Profile> {
        let own: Jid = self.own_jid().parse()?;
        let about = self
            .user_info(std::slice::from_ref(&own))
            .await
            .ok()
            .and_then(|mut info| info.drain().next())
            .and_then(|(_, info)| info.status);
        let privacy = self
            .client
            .fetch_privacy_settings()
            .await
            .map_err(anyhow::Error::from)?
            .settings
            .into_iter()
            .map(|s| (s.category.to_string(), s.value.to_string()))
            .collect();
        let own_username = self.client.mex().get_username().await.ok().flatten();
        let username_reserved = own_username
            .as_ref()
            .and_then(|u| u.state.as_deref())
            .is_some_and(|state| state.eq_ignore_ascii_case("reserved"));
        Ok(Profile {
            name: self.client.push_name(),
            about,
            username: own_username.and_then(|u| u.username),
            username_reserved,
            privacy,
        })
    }

    /// Replaces our profile picture with any image, cropped square and sized
    /// the way WhatsApp expects. Empty bytes remove the picture.
    pub async fn set_own_picture(&self, bytes: Vec<u8>) -> Result<()> {
        let profile = self.client.profile();
        let sent = if bytes.is_empty() {
            profile.remove_profile_picture().await
        } else {
            let jpeg = square_jpeg(&bytes, 640)
                .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.profile_picture_format")))?;
            profile.set_profile_picture(jpeg).await
        };
        sent.map_err(anyhow::Error::from)?;
        invalidate_avatar_cache(self.media_dir.as_deref(), &self.own_jid());
        Ok(())
    }

    pub async fn set_about(&self, text: &str) -> Result<()> {
        self.client
            .profile()
            .set_status_text(text)
            .await
            .map_err(anyhow::Error::from)
    }

    pub async fn set_push_name(&self, name: &str) -> Result<()> {
        self.client
            .profile()
            .set_push_name(name)
            .await
            .map_err(anyhow::Error::from)
    }

    pub async fn set_privacy(&self, category: &str, value: &str) -> Result<()> {
        let (category, value) = (PrivacyCategory::from(category), PrivacyValue::from(value));
        if !category.is_valid_value(&value) {
            anyhow::bail!(MessageRef::new("error.privacy_value").with_param("value", value.to_string()).with_param("category", category.to_string()));
        }
        self.client
            .set_privacy_setting(category, value)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    }

    /// Path to a chat's profile picture, cached for a day in the media folder.
    ///
    /// `None` when the chat has no picture, hides it from us, or media is off.
    /// `full` asks for the full-size picture instead of the small preview.
    pub async fn avatar(&self, jid: &str, full: bool) -> Result<Option<String>> {
        const TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
        let Some(dir) = self.media_dir.clone() else {
            return Ok(None);
        };
        let path = if full { avatar_full_path(&dir, jid) } else { avatar_path(&dir, jid) };
        // Marks a chat known to have no picture, so it is not asked again.
        let none = path.with_extension("none");
        let fresh = |p: &Path| {
            p.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < TTL)
        };
        if fresh(path.as_path()) {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
        if fresh(none.as_path()) {
            return Ok(None);
        }

        let target: Jid = jid.parse()?;
        let picture = self
            .client
            .contacts()
            .get_profile_picture(&target, !full)
            .await
            .map_err(anyhow::Error::from)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let Some(picture) = picture else {
            remove_cached_file(&path);
            std::fs::write(&none, b"")?;
            return Ok(None);
        };
        let url = picture.url;
        let bytes = tokio::task::spawn_blocking(move || links::fetch_public_avatar(&url))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.profile_picture_download")))?;
        std::fs::write(&path, bytes)?;
        remove_cached_file(&none);
        Ok(Some(path.to_string_lossy().into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fake_watch(tracked: &tokio::sync::Mutex<PresenceWatches>, calls: &Arc<Mutex<Vec<(bool, String)>>>, jid: Option<Jid>, active: bool) {
        let (subscribed, unsubscribed) = (calls.clone(), calls.clone());
        watch_presence_lru(tracked, jid, active,
            move |jid| { let calls = subscribed.clone(); async move { calls.lock().unwrap().push((true, jid.to_string())); Ok(()) } },
            move |jid| { let calls = unsubscribed.clone(); async move { calls.lock().unwrap().push((false, jid.to_string())); Ok(()) } },
        ).await.unwrap();
    }

    #[tokio::test]
    async fn fifty_chats_and_profile_watches_keep_the_open_chat_pinned() {
        let tracked = tokio::sync::Mutex::new(PresenceWatches::default());
        let calls = Arc::new(Mutex::new(Vec::<(bool, String)>::new()));
        for index in 0..50 {
            let jid = format!("{index}@s.whatsapp.net").parse().unwrap();
            fake_watch(&tracked, &calls, Some(jid), true).await;
        }
        let active: Jid = "49@s.whatsapp.net".parse().unwrap();
        for index in 0..16 {
            let jid = format!("{}@s.whatsapp.net", index + 100).parse().unwrap();
            fake_watch(&tracked, &calls, Some(jid), false).await;
        }
        let state = tracked.lock().await;
        assert_eq!(state.recent.len(), PRESENCE_CAP);
        assert_eq!(state.active.as_ref(), Some(&active));
        assert!(state.recent.contains(&active));
        drop(state);
        let observed = calls.lock().unwrap();
        assert_eq!(observed.iter().filter(|(subscribe, _)| *subscribe).count(), 66);
        assert!(!observed.iter().any(|(subscribe, jid)| !subscribe && jid == &active.to_string()));
        drop(observed);
        fake_watch(&tracked, &calls, None, true).await;
        let state = tracked.lock().await;
        assert!(state.active.is_none() && !state.recent.contains(&active));
        assert_eq!(state.recent.len(), PRESENCE_CAP - 1);
        assert_eq!(calls.lock().unwrap().last(), Some(&(false, active.to_string())));
    }
}
