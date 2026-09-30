use anyhow::{ensure, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_AUDIO_BYTES: usize = 700 * 1024;
pub const MAX_TRANSCRIPT_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum ProviderKind {
    Local,
    Cloud,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct TranscriptionProvider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub transmits_audio: bool,
    #[serde(default)]
    pub requires_key: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct TranscriptionContribution {
    pub id: String,
    pub providers: Vec<TranscriptionProvider>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct TranscriptionConfig {
    pub data_directory: Option<PathBuf>,
    pub whisper_executable: Option<PathBuf>,
    pub decoder_executable: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub model_sha256: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub cloud_consent: bool,
    pub api_key: Option<String>,
    pub timeout_secs: Option<u64>,
    pub idle_timeout_secs: Option<u64>,
}

impl Drop for TranscriptionConfig {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(key) = &mut self.api_key {
            key.zeroize();
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct TranscriptionRequest {
    pub provider: String,
    pub chat: String,
    pub message_id: String,
    pub mime: String,
    pub duration_ms: u64,
    pub audio: String,
    pub config: TranscriptionConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Transcript {
    pub provider: String,
    pub text: String,
    pub language: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ModelDownload {
    pub url: String,
    pub sha256: String,
    pub filename: String,
    pub data_directory: PathBuf,
}

impl ModelDownload {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.url.starts_with("https://") && self.url.len() <= 4096,
            "model download requires HTTPS"
        );
        ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "model SHA-256 required"
        );
        let path = std::path::Path::new(&self.filename);
        ensure!(
            path.components().count() == 1
                && matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "invalid model filename"
        );
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl TranscriptionContribution {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            identifier(&self.id),
            "invalid transcription contribution id"
        );
        ensure!(
            !self.providers.is_empty() && self.providers.len() <= 32,
            "invalid provider list"
        );
        let mut ids = std::collections::HashSet::new();
        for provider in &self.providers {
            ensure!(
                identifier(&provider.id) && ids.insert(&provider.id),
                "invalid or duplicate provider id"
            );
            ensure!(
                !provider.name.trim().is_empty() && provider.name.len() <= 200,
                "invalid provider name"
            );
            ensure!(
                provider.transmits_audio == (provider.kind == ProviderKind::Cloud),
                "provider privacy declaration mismatch"
            );
            ensure!(
                provider.kind != ProviderKind::Local || !provider.requires_key,
                "local provider cannot require cloud key"
            );
        }
        Ok(())
    }
}

impl TranscriptionRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(identifier(&self.provider), "invalid provider id");
        ensure!(
            !self.chat.is_empty() && self.chat.len() <= 300,
            "invalid chat"
        );
        ensure!(
            !self.message_id.is_empty() && self.message_id.len() <= 300,
            "invalid message id"
        );
        ensure!(
            matches!(
                self.mime.as_str(),
                "audio/ogg" | "audio/opus" | "audio/wav" | "audio/x-wav"
            ),
            "unsupported audio MIME"
        );
        ensure!(self.duration_ms <= 600_000, "audio exceeds ten minutes");
        ensure!(
            !self.audio.is_empty() && self.audio.len() <= MAX_AUDIO_BYTES.div_ceil(3) * 4,
            "audio exceeds payload limit"
        );
        let decoded = base64::engine::general_purpose::STANDARD.decode(&self.audio)?;
        ensure!(
            !decoded.is_empty() && decoded.len() <= MAX_AUDIO_BYTES,
            "audio exceeds payload limit"
        );
        ensure!(
            (1..=600).contains(&self.config.timeout_secs.unwrap_or(300)),
            "invalid transcription timeout"
        );
        ensure!(
            self.config.idle_timeout_secs != Some(0)
                && self.config.idle_timeout_secs.unwrap_or(0) <= 86400,
            "invalid idle timeout"
        );
        Ok(())
    }
}

impl Transcript {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.text.len() <= MAX_TRANSCRIPT_BYTES,
            "transcript exceeds size limit"
        );
        ensure!(
            self.language.as_ref().is_none_or(|s| s.len() <= 64),
            "invalid transcript language"
        );
        Ok(())
    }
}
