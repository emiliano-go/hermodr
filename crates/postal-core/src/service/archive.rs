use super::*;
use crate::store::archive::ArchiveReport;

impl WhatsAppService {
    pub async fn export_archive(&self, directory: PathBuf, chat: Option<String>) -> Result<ArchiveReport> {
        let media = self.media_dir.clone().ok_or_else(|| anyhow::anyhow!("no media folder configured"))?;
        let aliases = if chat.is_none() { self.aliases.all().await? } else { Vec::new() };
        self.store.run(move |store| match chat {
            Some(chat) => store.export_conversation(&chat, &directory, &media),
            None => store.export_backup(&directory, &media, &aliases),
        }).await
    }
}
