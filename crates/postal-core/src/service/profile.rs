//! Our own profile and presence, and other chats' pictures.

use super::*;

impl WhatsAppService {
    /// Tells the chat we are typing, or that we stopped.
    pub async fn send_typing(&self, chat: &str, typing: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let chatstate = self.client.chatstate();
        let sent = if typing {
            chatstate.send_composing(&jid).await
        } else {
            chatstate.send_paused(&jid).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))
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
        set.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Subscribes to a contact's presence, which one-to-one typing needs.
    pub async fn watch_presence(&self, jid: &str) -> Result<()> {
        let jid: Jid = jid.parse()?;
        self.client
            .presence()
            .subscribe(jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
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
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
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
                .ok_or_else(|| anyhow::anyhow!("that file is not an image we can read"))?;
            profile.set_profile_picture(jpeg).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(dir) = &self.media_dir {
            let path = avatar_path(dir, &self.own_jid());
            let _ = std::fs::remove_file(path.with_extension("none"));
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(avatar_full_path(dir, &self.own_jid()));
        }
        Ok(())
    }

    pub async fn set_about(&self, text: &str) -> Result<()> {
        self.client
            .profile()
            .set_status_text(text)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    pub async fn set_push_name(&self, name: &str) -> Result<()> {
        self.client
            .profile()
            .set_push_name(name)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    pub async fn set_privacy(&self, category: &str, value: &str) -> Result<()> {
        let (category, value) = (PrivacyCategory::from(category), PrivacyValue::from(value));
        if !category.is_valid_value(&value) {
            anyhow::bail!("{value} is not a valid setting for {category}");
        }
        self.client
            .set_privacy_setting(category, value)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
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
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let Some(picture) = picture else {
            let _ = std::fs::remove_file(&path);
            std::fs::write(&none, b"")?;
            return Ok(None);
        };
        let url = picture.url;
        let bytes = tokio::task::spawn_blocking(move || {
            let mut response = ureq::get(&url).call().ok()?;
            response.body_mut().read_to_vec().ok()
        })
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("could not download the profile picture"))?;
        std::fs::write(&path, bytes)?;
        let _ = std::fs::remove_file(&none);
        Ok(Some(path.to_string_lossy().into_owned()))
    }
}
