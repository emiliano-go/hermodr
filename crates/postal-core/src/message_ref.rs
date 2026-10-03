use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const OPERATION_FAILED: &str = "error.operation_failed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum MessageParam {
    String(String),
    Number(#[cfg_attr(feature = "wire-types", ts(type = "number"))] serde_json::Number),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageRef {
    pub code: String,
    pub params: BTreeMap<String, MessageParam>,
}

impl MessageRef {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into(), params: BTreeMap::new() }
    }

    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<MessageParam>) -> Self {
        self.params.insert(name.into(), value.into());
        self
    }
}

impl std::fmt::Display for MessageRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for MessageRef {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageFailure {
    #[serde(flatten)]
    pub message: MessageRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    pub diagnostic: Option<String>,
}

impl From<anyhow::Error> for MessageFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::from(&error)
    }
}

impl From<&anyhow::Error> for MessageFailure {
    fn from(error: &anyhow::Error) -> Self {
        match error.downcast_ref::<MessageRef>() {
            Some(message) => Self { message: message.clone(),
                diagnostic: error.chain().nth(1).map(|_| format!("{error:#}")) },
            None => Self { message: MessageRef::new(OPERATION_FAILED), diagnostic: Some(format!("{error:#}")) },
        }
    }
}

impl From<String> for MessageParam {
    fn from(value: String) -> Self { Self::String(value) }
}

impl From<&str> for MessageParam {
    fn from(value: &str) -> Self { Self::String(value.into()) }
}

impl From<serde_json::Number> for MessageParam {
    fn from(value: serde_json::Number) -> Self { Self::Number(value) }
}

impl From<bool> for MessageParam {
    fn from(value: bool) -> Self { Self::Boolean(value) }
}
