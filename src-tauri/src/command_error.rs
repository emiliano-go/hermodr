use postal_core::message_ref::{MessageFailure, MessageRef, OPERATION_FAILED};
use serde::{Deserialize, Serialize};

pub(crate) type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) enum CommandErrorKind {
    #[serde(rename = "postal_error")]
    PostalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct CommandError {
    pub kind: CommandErrorKind,
    #[serde(flatten)]
    pub message: MessageRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    pub diagnostic: Option<String>,
}

impl CommandError {
    pub(crate) fn code(code: &str) -> Self { Self::new(MessageRef::new(code)) }

    pub(crate) fn new(message: MessageRef) -> Self {
        Self { kind: CommandErrorKind::PostalError, message, diagnostic: None }
    }

    pub(crate) fn with_diagnostic(mut self, error: impl std::fmt::Display) -> Self {
        self.diagnostic = Some(format!("{error:#}"));
        self
    }

    pub(crate) fn operation_failed(error: impl std::fmt::Display) -> Self {
        Self::new(MessageRef::new(OPERATION_FAILED)).with_diagnostic(error)
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        MessageFailure::from(error).into()
    }
}

impl From<MessageFailure> for CommandError {
    fn from(failure: MessageFailure) -> Self {
        Self { kind: CommandErrorKind::PostalError, message: failure.message, diagnostic: failure.diagnostic }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self { Self::operation_failed(error) }
}

impl From<String> for CommandError {
    fn from(error: String) -> Self { Self::operation_failed(error) }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.diagnostic.as_deref().unwrap_or(&self.message.code))
    }
}

impl std::error::Error for CommandError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_errors_keep_codes_params_and_outer_context() {
        let source = MessageRef::new("error.text_size_limit")
            .with_param("max_bytes", serde_json::Number::from(65536));
        let direct = CommandError::from(anyhow::Error::new(source.clone()));
        assert_eq!(direct.message, source);
        assert!(direct.diagnostic.is_none());
        let wrapped = CommandError::from(anyhow::Error::new(source.clone()).context("synthetic outer context"));
        assert_eq!(wrapped.message, source);
        assert!(wrapped.diagnostic.unwrap().contains("synthetic outer context"));
        let json = serde_json::to_value(direct).unwrap();
        assert_eq!(json["kind"], "postal_error");
        assert_eq!(json["params"]["max_bytes"], 65536);
        assert!(json.get("message").is_none());
    }

    #[test]
    fn unknown_errors_keep_diagnostics_with_localizable_fallback() {
        let error = CommandError::from(anyhow::anyhow!("synthetic database failure"));
        assert_eq!(error.message.code, OPERATION_FAILED);
        assert_eq!(error.diagnostic.as_deref(), Some("synthetic database failure"));
    }
}
