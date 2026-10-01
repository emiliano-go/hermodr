use super::*;

impl WhatsAppService {
    pub async fn keyword_mentions(
        &self,
        highlight: &[String],
        hide: &[String],
    ) -> Result<std::collections::HashMap<String, i64>> {
        let (highlight, hide) = (highlight.to_vec(), hide.to_vec());
        self.store
            .run(move |store| store.keyword_mentions(&highlight, &hide))
            .await
    }

    pub async fn keyword_matches(
        &self,
        chat: Option<&str>,
        unread_only: bool,
        highlight: &[String],
        hide: &[String],
    ) -> Result<Vec<StoredMessage>> {
        let (chat, highlight, hide) = (chat.map(str::to_owned), highlight.to_vec(), hide.to_vec());
        self.store
            .run(move |store| {
                store.keyword_matches(chat.as_deref(), unread_only, &highlight, &hide)
            })
            .await
    }
}
