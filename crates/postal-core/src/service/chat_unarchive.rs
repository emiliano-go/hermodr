use super::*;

impl WhatsAppService {
    pub async fn chat_unarchive(&self, chat: &str) -> Result<Option<bool>> {
        self.store.chat_unarchive(chat).await
    }

    pub async fn set_chat_unarchive(&self, chat: &str, enabled: Option<bool>) -> Result<()> {
        self.store.set_chat_unarchive(chat, enabled).await
    }
}

#[cfg(test)]
#[path = "chat_unarchive_tests.rs"]
mod tests;
