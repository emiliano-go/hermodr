use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[cfg_attr(feature = "wire-types", ts(rename = "PluginReply"))]
pub(crate) enum Reply {
    Ready { name: String },
    Ack { seq: u64 },
    Log { level: String, message: String },
    Call { id: serde_json::Value },
    Event {},
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) enum HostMessage<E = serde_json::Value> {
    Hello { api_version: u32, capabilities: Vec<String> },
    Event { seq: u64, event: E },
    Error { id: serde_json::Value, error: String },
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn host_messages_keep_v1_keys_and_event_payload() {
        let event = serde_json::json!({"kind":"messageHint", "from_me":true, "id":"synthetic"});
        for (message, expected) in [
            (HostMessage::Hello { api_version: 1, capabilities: vec!["events:read".into()] },
                serde_json::json!({"type":"hello", "api_version":1, "capabilities":["events:read"]})),
            (HostMessage::Event { seq: 7, event: event.clone() },
                serde_json::json!({"type":"event", "seq":7, "event":event})),
            (HostMessage::Error { id: serde_json::json!("request"), error: "refused".into() },
                serde_json::json!({"type":"error", "id":"request", "error":"refused"})),
        ] {
            assert_eq!(serde_json::to_value(message).unwrap(), expected);
        }
    }
}

pub(crate) async fn line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok((!bytes.is_empty() || oversized).then_some(bytes));
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let count = newline.map_or(chunk.len(), |i| i + 1);
        if !oversized && bytes.len() + count <= MAX_LINE {
            bytes.extend_from_slice(&chunk[..count]);
        } else {
            oversized = true;
            bytes.clear();
        }
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(bytes));
        }
    }
}
