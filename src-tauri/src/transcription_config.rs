use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct TranscriptionSettings {
    pub plugin_id: Option<String>,
    pub provider: String,
    pub whisper_executable: Option<PathBuf>,
    pub decoder_executable: Option<PathBuf>,
    pub model: Option<String>,
    pub model_sha256: Option<String>,
    pub language: Option<String>,
    pub idle_timeout_secs: Option<u64>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Configuration {
    pub settings: TranscriptionSettings,
    pub cloud_consents: BTreeSet<(String, String)>,
}

impl TranscriptionSettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.idle_timeout_secs == Some(0) || self.idle_timeout_secs.unwrap_or(0) > 86400 {
            return Err("idle timeout must be null or 1–86400 seconds".into());
        }
        if self.language.as_ref().is_some_and(|s| {
            s.len() > 32 || !s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        }) {
            return Err("invalid language".into());
        }
        for path in [&self.whisper_executable, &self.decoder_executable]
            .into_iter()
            .flatten()
        {
            if !path.is_absolute() || !path.is_file() {
                return Err("engine paths must be existing absolute executables".into());
            }
        }
        if let Some(model) = &self.model {
            let path = std::path::Path::new(model);
            if path.components().count() != 1
                || !matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                return Err("model must be a filename in plugin data directory".into());
            }
        }
        if self
            .model_sha256
            .as_ref()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("invalid model SHA-256".into());
        }
        Ok(())
    }
}

pub(crate) fn effective_auto(global: bool, chat: Option<bool>) -> bool {
    chat.unwrap_or(global)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_defaults_and_nullable_override() {
        assert!(!effective_auto(false, None));
        assert!(effective_auto(true, None));
        assert!(!effective_auto(true, Some(false)));
        assert!(effective_auto(false, Some(true)));
        let config: Configuration = serde_json::from_str("{}").unwrap();
        assert!(config.settings.plugin_id.is_none());
        assert!(config.cloud_consents.is_empty());
        assert!(config.settings.idle_timeout_secs.is_none());
        assert!(!serde_json::to_string(&config).unwrap().contains("api_key"));
    }
}
