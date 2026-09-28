use serde::Deserialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Reply {
    Ready { name: String },
    Ack { seq: u64 },
    Log { level: String, message: String },
    Call { id: serde_json::Value },
    Event {},
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
