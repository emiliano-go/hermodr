use super::*;
use crate::store::media_policy::{MediaAutoDownload, MediaAutoDownloadOverrides};

impl WhatsAppService {
    pub async fn chat_media_auto_download(&self, chat: &str) -> Result<MediaAutoDownloadOverrides> {
        self.store.chat_media_auto_download(chat).await
    }

    pub async fn set_chat_media_auto_download(
        &self,
        chat: &str,
        overrides: MediaAutoDownloadOverrides,
    ) -> Result<()> {
        self.store
            .set_chat_media_auto_download(chat, overrides)
            .await
    }

    pub async fn effective_media_auto_download(
        &self,
        chat: &str,
        kind: &str,
        global: MediaAutoDownload,
    ) -> Result<bool> {
        Ok(global.effective(kind, self.chat_media_auto_download(chat).await?))
    }
}

#[cfg(test)]
#[path = "media_policy_tests.rs"]
mod tests;
