use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RetentionLimit {
    Inherit,
    Unlimited,
    Limited(u32),
}

impl RetentionLimit {
    pub fn value(self) -> Option<u32> {
        match self { Self::Limited(value) => Some(value), _ => None }
    }

    pub(super) fn mode(self) -> &'static str {
        match self { Self::Inherit => "inherit", Self::Unlimited => "unlimited", Self::Limited(_) => "limited" }
    }

    pub(super) fn from_row(row: &rusqlite::Row<'_>, mode: usize, value: usize) -> rusqlite::Result<Self> {
        match row.get::<_, String>(mode)?.as_str() {
            "inherit" => Ok(Self::Inherit),
            "unlimited" => Ok(Self::Unlimited),
            "limited" => Ok(Self::Limited(row.get(value)?)),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompatibleLimit {
    Explicit(RetentionLimit),
    Legacy(Option<u32>),
}

fn deserialize<'de, D: Deserializer<'de>>(deserializer: D, inherited: bool, zero_unlimited: bool) -> Result<RetentionLimit, D::Error> {
    use RetentionLimit::*;
    match CompatibleLimit::deserialize(deserializer)? {
        CompatibleLimit::Explicit(Inherit) if !inherited => Err(serde::de::Error::custom("global limits cannot inherit")),
        CompatibleLimit::Explicit(limit) => Ok(limit),
        CompatibleLimit::Legacy(None) => Ok(if inherited { Inherit } else { Unlimited }),
        CompatibleLimit::Legacy(Some(0)) if zero_unlimited => Ok(Unlimited),
        CompatibleLimit::Legacy(Some(value)) => Ok(Limited(value)),
    }
}

pub(super) fn global_age<'de, D: Deserializer<'de>>(d: D) -> Result<RetentionLimit, D::Error> { deserialize(d, false, false) }
pub(super) fn global_count<'de, D: Deserializer<'de>>(d: D) -> Result<RetentionLimit, D::Error> { deserialize(d, false, true) }
pub(super) fn chat_limit<'de, D: Deserializer<'de>>(d: D) -> Result<RetentionLimit, D::Error> { deserialize(d, true, true) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ChatRetention, Retention};

    #[test]
    fn legacy_policies_keep_their_meaning_and_serialize_explicitly() {
        let global: Retention = serde_json::from_str(r#"{"max_age_hours":24,"max_messages_per_chat":null}"#).unwrap();
        assert_eq!(global.max_age_hours, RetentionLimit::Limited(24));
        assert_eq!(global.max_messages_per_chat, RetentionLimit::Unlimited);
        let chat: ChatRetention = serde_json::from_str(r#"{"max_age_hours":null,"max_messages":0,"on_demand":true}"#).unwrap();
        assert_eq!(chat.max_age_hours, RetentionLimit::Inherit);
        assert_eq!(chat.max_messages, RetentionLimit::Unlimited);
        let encoded = serde_json::to_string(&chat).unwrap();
        assert!(encoded.contains(r#""kind":"inherit""#));
        assert!(encoded.contains(r#""kind":"unlimited""#));
        assert_eq!(serde_json::from_str::<ChatRetention>(&encoded).unwrap(), chat);
        assert!(serde_json::from_str::<Retention>(r#"{"max_age_hours":{"kind":"inherit"},"max_messages_per_chat":null}"#).is_err());
        let zero: Retention = serde_json::from_str(r#"{"max_age_hours":0,"max_messages_per_chat":0}"#).unwrap();
        assert_eq!(zero.max_age_hours, RetentionLimit::Limited(0));
        assert_eq!(zero.max_messages_per_chat, RetentionLimit::Unlimited);
        assert_ne!(RetentionLimit::Limited(0), RetentionLimit::Unlimited);
    }
}
