use super::*;

pub fn message_urls(text: &str, preview: Option<&str>) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut urls = Vec::new();
    for (start, _) in lower.match_indices("http") {
        let rest = &text[start..];
        if !(lower[start..].starts_with("https://") || lower[start..].starts_with("http://")) { continue; }
        let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`')).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim_end_matches(['.', ',', ';']).to_string();
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while candidate.ends_with(close) && candidate.matches(close).count() > candidate.matches(open).count() {
                candidate.pop();
            }
        }
        push_url(&mut urls, &candidate);
    }
    if let Some(preview) = preview { push_url(&mut urls, preview); }
    urls
}

fn push_url(urls: &mut Vec<String>, candidate: &str) {
    let Some(scheme) = candidate.find("://") else { return };
    let candidate = format!("{}{}", candidate[..scheme].to_ascii_lowercase(), &candidate[scheme..]);
    let Ok(url) = url::Url::parse(&candidate) else { return };
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() && !urls.contains(&candidate) {
        urls.push(candidate);
    }
}

pub(super) fn refresh(conn: &Connection, chat: &str, id: &str) -> Result<()> {
    let content = conn.query_row(
        "SELECT text, CASE WHEN EXISTS(SELECT 1 FROM edited e WHERE e.chat = m.chat AND e.id = m.id)
         THEN NULL ELSE preview_url END FROM messages m WHERE chat = ?1 AND id = ?2",
        params![chat, id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    ).optional()?;
    if let Some((text, preview)) = content {
        conn.execute("UPDATE messages SET link_urls = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, serde_json::to_string(&message_urls(&text, preview.as_deref()))?])?;
    }
    Ok(())
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('messages') WHERE name = 'link_urls')", [], |row| row.get(0))?;
    if !exists { conn.execute_batch("ALTER TABLE messages ADD COLUMN link_urls TEXT NOT NULL DEFAULT '[]';")?; }
    let mut cursor = 0;
    loop {
        let rows = conn.prepare("SELECT rowid, chat, id FROM messages WHERE rowid > ?1
            AND (lower(text) LIKE '%http%' OR preview_url IS NOT NULL) ORDER BY rowid LIMIT 256")?
            .query_map([cursor], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() { break; }
        for (rowid, chat, id) in rows { refresh(conn, &chat, &id)?; cursor = rowid; }
    }
    super::gallery::create_indexes(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_need_no_preview_and_preserve_order_balanced_paths_and_safe_schemes() {
        assert_eq!(message_urls("See (HTTPS://first.test/a_(b)), then http://second.test/?a=1&b=2. https://first.test/a_(b)", None),
            ["https://first.test/a_(b)", "http://second.test/?a=1&b=2"]);
        assert_eq!(message_urls("https://one.test and https://two.test", Some("https://one.test")), ["https://one.test", "https://two.test"]);
        assert!(message_urls("javascript:alert(1) file:///tmp/a http:// https://", Some("mailto:a@test")).is_empty());
        assert_eq!(message_urls("preview only", Some("https://card.test")), ["https://card.test"]);
        assert_eq!(message_urls("https://例子.测试/你好#章 https://example.test/path#section", None), ["https://例子.测试/你好#章", "https://example.test/path#section"]);
    }

    #[test]
    fn link_index_tracks_edits_replays_revoke_delete_and_caption_urls() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut message = StoredMessage { header: MessageHeader { chat: "a@s".into(), id: "one".into(), timestamp: 100, ..Default::default() },
            text: "https://old.test".into(), link: LinkCard { url: Some("https://old.test".into()), ..Default::default() }, ..Default::default() };
        store.insert_message(&message).unwrap();
        let filter = super::super::gallery::GalleryFilter { kind: Some(super::super::gallery::GalleryKind::Link), ..Default::default() };
        assert_eq!(store.gallery_page(&filter, None, 20).unwrap().items[0].urls, ["https://old.test"]);
        store.update_message_content("a@s", "one", "https://edited.test").unwrap();
        store.insert_message(&message).unwrap();
        assert_eq!(store.gallery_page(&filter, None, 20).unwrap().items[0].urls, ["https://edited.test"]);
        store.update_message_content("a@s", "one", "no link left").unwrap();
        store.insert_message(&message).unwrap();
        assert!(store.gallery_page(&filter, None, 20).unwrap().items.is_empty());
        message.header.id = "caption".into();
        message.media.kind = Some("image".into());
        message.link = LinkCard::default();
        store.insert_message(&message).unwrap();
        assert_eq!(store.gallery_page(&filter, None, 20).unwrap().items.len(), 1);
        store.revoke_message("a@s", "caption").unwrap();
        assert!(store.gallery_page(&filter, None, 20).unwrap().items.is_empty());
        message.header.id = "preview-only".into();
        message.media = Media::default();
        message.text = "A preview without URL text".into();
        message.link.url = Some("https://preview.synthetic.test".into());
        store.insert_message(&message).unwrap();
        assert_eq!(store.search_messages("a@s", "preview.synthetic.test", 10).unwrap()[0].header.id, "preview-only");
    }
}
