use super::*;
use crate::store::transcription::StoredTranscript;
use anyhow::Context;

pub struct TranscriptionAudio {
    pub chat: String,
    pub mime: String,
    pub duration_ms: u64,
    pub bytes: Vec<u8>,
}

impl WhatsAppService {
    pub async fn message_transcript(
        &self,
        chat: &str,
        id: &str,
    ) -> Result<Option<StoredTranscript>> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.store
            .run(move |s| s.message_transcript(&chat, &id))
            .await
    }
    pub async fn save_transcript(&self, transcript: StoredTranscript) -> Result<()> {
        self.store
            .run(move |s| s.save_transcript(&transcript))
            .await
    }
    pub async fn chat_auto_transcribe(&self, chat: &str) -> Result<Option<bool>> {
        let chat = chat.to_owned();
        self.store.run(move |s| s.chat_auto_transcribe(&chat)).await
    }
    pub async fn set_chat_auto_transcribe(&self, chat: &str, enabled: Option<bool>) -> Result<()> {
        let chat = chat.to_owned();
        self.store
            .run(move |s| s.set_chat_auto_transcribe(&chat, enabled))
            .await
    }
    pub async fn transcription_audio(
        &self,
        chat: &str,
        id: &str,
        automatic: bool,
        max_bytes: usize,
    ) -> Result<TranscriptionAudio> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        let query_chat = chat.clone();
        let query_id = id.clone();
        let mut media = self
            .store
            .run(move |s| s.transcription_media(&query_chat, &query_id))
            .await?
            .context("voice note unavailable")?;
        anyhow::ensure!(
            !automatic || !media.spoiler,
            "spoiler voice note needs manual transcription"
        );
        if media.path.is_none() {
            anyhow::ensure!(
                !automatic,
                "automatic transcription waits for downloaded audio"
            );
            self.download_media(&chat, &id).await?;
            let query_chat = chat.clone();
            let query_id = id.clone();
            media = self
                .store
                .run(move |s| s.transcription_media(&query_chat, &query_id))
                .await?
                .context("voice note unavailable")?;
        }
        let path = PathBuf::from(media.path.context("audio not downloaded")?);
        let root = self
            .media_dir
            .clone()
            .context("no media folder configured")?;
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            use std::io::Read;
            let path = path.canonicalize()?;
            anyhow::ensure!(
                path.starts_with(root.canonicalize()?),
                "audio path outside media directory"
            );
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(max_bytes as u64 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                !bytes.is_empty() && bytes.len() <= max_bytes,
                "audio exceeds transcription size limit"
            );
            Ok(bytes)
        })
        .await??;
        let mime = if bytes.starts_with(b"OggS") {
            "audio/ogg"
        } else if bytes.starts_with(b"RIFF") {
            "audio/wav"
        } else {
            anyhow::bail!("voice note must be OGG/Opus or WAV")
        };
        Ok(TranscriptionAudio {
            chat: media.chat,
            mime: mime.into(),
            duration_ms: media.duration_ms,
            bytes,
        })
    }
}
