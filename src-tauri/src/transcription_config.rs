use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use crate::command_error::{CommandError, CommandResult};
use postal_core::message_ref::MessageRef;

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
    pub(crate) fn validate(&self) -> CommandResult<()> {
        if self.idle_timeout_secs == Some(0) || self.idle_timeout_secs.unwrap_or(0) > 86400 {
            return Err(CommandError::new(MessageRef::new("error.transcription_idle_timeout_invalid")
                .with_param("min", serde_json::Number::from(1)).with_param("max", serde_json::Number::from(86400))));
        }
        if self.language.as_ref().is_some_and(|s| {
            s.len() > 32 || !s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        }) {
            return Err(CommandError::new(MessageRef::new("error.transcription_language_invalid").with_param("max", serde_json::Number::from(32))));
        }
        for path in [&self.whisper_executable, &self.decoder_executable]
            .into_iter()
            .flatten()
        {
            if !path.is_absolute() || !path.is_file() {
                return Err(CommandError::code("error.transcription_engine_invalid"));
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
                return Err(CommandError::code("error.transcription_model_filename_invalid"));
            }
        }
        if self
            .model_sha256
            .as_ref()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(CommandError::code("error.transcription_model_hash_invalid"));
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

    #[test]
    fn configuration_guard_codes_keep_source_constraints_and_no_new_saved_fields() {
        let mut settings = TranscriptionSettings::default();
        settings.idle_timeout_secs = Some(0);
        let error = settings.validate().unwrap_err();
        assert_eq!(error.message.code, "error.transcription_idle_timeout_invalid");
        assert_eq!(serde_json::to_value(error).unwrap()["params"]["max"], 86400);
        settings.idle_timeout_secs = Some(86400);
        assert!(settings.validate().is_ok());
        settings.language = Some("bad_language".into());
        assert_eq!(settings.validate().unwrap_err().message.code, "error.transcription_language_invalid");
        settings.language = None;
        settings.model = Some("../synthetic".into());
        assert_eq!(settings.validate().unwrap_err().message.code, "error.transcription_model_filename_invalid");
        settings.model = None;
        settings.model_sha256 = Some("x".repeat(64));
        assert_eq!(settings.validate().unwrap_err().message.code, "error.transcription_model_hash_invalid");
        let stored = serde_json::to_value(Configuration::default()).unwrap();
        assert!(stored.get("failures").is_none() && stored.get("error_message").is_none());
    }
}
