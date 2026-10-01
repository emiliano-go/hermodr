use super::*;

fn trim_keyword(text: &str) -> &str {
    text.trim_matches(|c: char| (c.is_whitespace() && c != '\u{0085}') || c == '\u{FEFF}')
}

fn terms(raw: &[String]) -> Result<Vec<String>> {
    anyhow::ensure!(
        raw.len() <= 50,
        "Each keyword list supports up to 50 entries."
    );
    let mut result = Vec::new();
    for term in raw {
        let term = trim_keyword(term);
        anyhow::ensure!(
            term.chars().count() <= 100,
            "Each keyword can contain up to 100 characters."
        );
        let term = term.to_lowercase();
        if !term.is_empty() && !result.contains(&term) {
            result.push(term);
        }
    }
    Ok(result)
}

fn matches(body: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| body.contains(term))
}

const ELIGIBLE_SQL: &str = "m.from_me = 0 AND m.deleted = 0 AND m.revoked = 0
    AND m.system_kind IS NULL AND m.spoiler = 0 AND m.media_once_kind IS NULL
    AND COALESCE(m.media_kind, '') NOT IN ('view_once', 'unknown')
    AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat = m.chat AND v.id = m.id)
    AND NOT EXISTS (SELECT 1 FROM hidden_chats h WHERE h.jid = m.chat)";

fn keyword_hit(text: &str, kind: Option<&str>, highlight: &[String], hide: &[String]) -> bool {
    if kind.is_some_and(|kind| trim_keyword(text) == format!("[{kind}]")) {
        return false;
    }
    let body = text.to_lowercase();
    matches(&body, highlight) && !matches(&body, hide)
}

impl MessageStore {
    pub fn keyword_mentions(
        &self,
        highlight: &[String],
        hide: &[String],
    ) -> Result<std::collections::HashMap<String, i64>> {
        let (highlight, hide) = (terms(highlight)?, terms(hide)?);
        let mut counts = std::collections::HashMap::new();
        if highlight.is_empty() {
            return Ok(counts);
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT m.chat, m.text, m.media_kind FROM messages m
            WHERE {ELIGIBLE_SQL} AND m.read = 0 AND m.mentioned = 0"
        ))?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (chat, text, kind) = row?;
            if keyword_hit(&text, kind.as_deref(), &highlight, &hide) {
                *counts.entry(chat).or_insert(0) += 1;
            }
        }
        Ok(counts)
    }

    pub fn keyword_matches(
        &self,
        chat: Option<&str>,
        unread_only: bool,
        highlight: &[String],
        hide: &[String],
    ) -> Result<Vec<StoredMessage>> {
        let (highlight, hide) = (terms(highlight)?, terms(hide)?);
        let mut found = Vec::new();
        if highlight.is_empty() {
            return Ok(found);
        }
        let conn = self.conn.lock().unwrap();
        let chat = chat
            .map(|chat| names::canonical_chat(&conn, chat).map(|chat| chat.to_string()))
            .transpose()?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages m
            LEFT JOIN names n ON n.jid = m.sender WHERE {ELIGIBLE_SQL}
              AND (?1 IS NULL OR m.chat = ?1) AND (?2 = 0 OR m.read = 0)
            ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC, m.chat ASC"
        ))?;
        let rows = stmt.query_map(params![chat, unread_only], message_row)?;
        for row in rows {
            let message = row?;
            if keyword_hit(
                &message.text,
                message.media.kind.as_deref(),
                &highlight,
                &hide,
            ) {
                found.push(message);
                if found.len() == 500 {
                    break;
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
#[path = "keywords_tests.rs"]
mod tests;
