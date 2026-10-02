use super::*;
use std::collections::{HashMap, HashSet};
use whatsapp_rust::wacore_binary::{Jid, JidExt};

const MAX_SPACES: usize = 1000;
const MAX_ITEMS: usize = 10000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Space {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub order: u32,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SpaceItem {
    pub id: String,
    pub space_id: String,
    pub target: SpaceTarget,
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum SpaceTarget {
    Chat { jid: String },
    Group { jid: String },
    Community { jid: String },
    Channel { jid: String },
    Contact { jid: String },
    FavoriteContact { jid: String },
    Label { label_id: String },
    SavedMessage { chat: String, message_id: String },
    SavedSearch { query: String, chat: Option<String> },
    InboxView { filters: SpaceInboxFilters },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SpaceInboxFilters {
    pub unread: bool,
    pub mentions: bool,
    pub labelled: bool,
    pub muted: bool,
    pub archived: bool,
    pub label: String,
    pub query: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SpaceSnapshot {
    pub spaces: Vec<Space>,
    pub items: Vec<SpaceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SpaceArchive {
    pub version: u32,
    pub snapshot: SpaceSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum SpaceAction {
    Create { id: String, parent_id: Option<String>, name: String, icon: Option<String>, color: Option<String> },
    Rename { id: String, name: String },
    Reparent { id: String, parent_id: Option<String> },
    Reorder { parent_id: Option<String>, ids: Vec<String> },
    Delete { id: String },
    AddItem { id: String, space_id: String, target: SpaceTarget },
    RemoveItem { id: String },
    ReorderItems { space_id: String, ids: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum SpaceSelection { All, Unsorted, Space { space_id: String } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ResolvedSpaceItem {
    pub item_id: String,
    pub chats: Vec<String>,
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SpaceResolution {
    pub chats: Vec<String>,
    pub items: Vec<ResolvedSpaceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct CachedSpaceGroup {
    pub jid: String,
    pub subject: Option<String>,
    pub parent: Option<String>,
    pub community: bool,
    pub announcements: bool,
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS local_spaces (
        id TEXT PRIMARY KEY, parent_id TEXT REFERENCES local_spaces(id) DEFERRABLE INITIALLY DEFERRED,
        name TEXT NOT NULL, icon TEXT, color TEXT, position INTEGER NOT NULL, created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS local_space_items (
        id TEXT PRIMARY KEY, space_id TEXT NOT NULL REFERENCES local_spaces(id) DEFERRABLE INITIALLY DEFERRED,
        target TEXT NOT NULL, position INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS local_space_parent_order ON local_spaces(parent_id,position);
        CREATE INDEX IF NOT EXISTS local_space_item_order ON local_space_items(space_id,position);
        CREATE TABLE IF NOT EXISTS cached_group_catalog (
        jid TEXT PRIMARY KEY, subject TEXT, parent TEXT, community INTEGER NOT NULL,
        announcements INTEGER NOT NULL);")?;
    Ok(())
}

fn text(value: &str, limit: usize, empty: bool) -> Result<()> {
    anyhow::ensure!(value.len() <= limit && (empty || !value.trim().is_empty())
        && !value.chars().any(char::is_control), "Invalid Space text or identifier.");
    Ok(())
}

fn identifier(value: &str) -> Result<()> {
    text(value, 128, false)?;
    anyhow::ensure!(value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "Invalid Space ID.");
    Ok(())
}

fn address(value: &str) -> Result<Jid> {
    text(value, 256, false)?;
    let jid: Jid = value.parse()?;
    anyhow::ensure!(!jid.user.is_empty() && jid.device == 0 && jid.agent == 0 && jid.integrator == 0
        && (jid.is_pn() || jid.is_lid() || jid.is_group() || value.ends_with("@newsletter") || jid.is_broadcast_list()),
        "Invalid local Space address.");
    Ok(jid)
}

fn validate_target(target: &SpaceTarget) -> Result<()> {
    match target {
        SpaceTarget::Chat { jid } => { address(jid)?; }
        SpaceTarget::Group { jid } | SpaceTarget::Community { jid } => {
            anyhow::ensure!(address(jid)?.is_group(), "Space group reference is not a group.");
        }
        SpaceTarget::Channel { jid } => {
            address(jid)?;
            anyhow::ensure!(jid.ends_with("@newsletter"), "Space channel reference is not a channel.");
        }
        SpaceTarget::Contact { jid } | SpaceTarget::FavoriteContact { jid } => {
            let jid = address(jid)?;
            anyhow::ensure!(jid.is_pn() || jid.is_lid(), "Space contact reference is not a contact.");
        }
        SpaceTarget::Label { label_id } => text(label_id, 256, false)?,
        SpaceTarget::SavedMessage { chat, message_id } => { address(chat)?; text(message_id, 256, false)?; }
        SpaceTarget::SavedSearch { query, chat } => {
            text(query, 4096, false)?;
            if let Some(chat) = chat { address(chat)?; }
        }
        SpaceTarget::InboxView { filters } => {
            text(&filters.label, 256, true)?;
            text(&filters.query, 4096, true)?;
        }
    }
    Ok(())
}

fn canonical_target(conn: &Connection, mut target: SpaceTarget) -> Result<SpaceTarget> {
    validate_target(&target)?;
    let jid = match &mut target {
        SpaceTarget::Chat { jid } | SpaceTarget::Group { jid } | SpaceTarget::Community { jid }
        | SpaceTarget::Channel { jid } | SpaceTarget::Contact { jid } | SpaceTarget::FavoriteContact { jid } => Some(jid),
        SpaceTarget::SavedMessage { chat, .. } => Some(chat),
        SpaceTarget::SavedSearch { chat, .. } => chat.as_mut(),
        _ => None,
    };
    if let Some(jid) = jid {
        *jid = names::canonical_chat(conn, &address(jid)?.to_non_ad().to_string())?.into_owned();
    }
    Ok(target)
}

fn validate_snapshot(snapshot: &SpaceSnapshot) -> Result<()> {
    anyhow::ensure!(snapshot.spaces.len() <= MAX_SPACES && snapshot.items.len() <= MAX_ITEMS, "Space metadata exceeds local bounds.");
    let spaces: HashMap<_, _> = snapshot.spaces.iter().map(|space| (space.id.as_str(), space)).collect();
    anyhow::ensure!(spaces.len() == snapshot.spaces.len(), "Duplicate Space IDs.");
    let mut positions: HashMap<Option<&str>, Vec<u32>> = HashMap::new();
    for space in &snapshot.spaces {
        identifier(&space.id)?;
        text(&space.name, 256, false)?;
        anyhow::ensure!(space.name == space.name.trim() && (0..=9_007_199_254_740_991).contains(&space.created_at), "Invalid Space name or creation time.");
        if let Some(icon) = &space.icon { identifier(icon)?; }
        if let Some(color) = &space.color {
            anyhow::ensure!(matches!(color.len(), 7 | 9) && color.starts_with('#')
                && color[1..].bytes().all(|b| b.is_ascii_hexdigit()), "Invalid Space color.");
        }
        let mut seen = HashSet::new();
        let mut parent = space.parent_id.as_deref();
        seen.insert(space.id.as_str());
        while let Some(id) = parent {
            anyhow::ensure!(seen.insert(id), "Space nesting contains a cycle.");
            parent = spaces.get(id).ok_or_else(|| anyhow::anyhow!("Space parent is missing."))?.parent_id.as_deref();
        }
        positions.entry(space.parent_id.as_deref()).or_default().push(space.order);
    }
    for orders in positions.values_mut() { validate_orders(orders)?; }
    let mut ids = HashSet::new();
    let mut targets = HashSet::new();
    let mut positions: HashMap<&str, Vec<u32>> = HashMap::new();
    for item in &snapshot.items {
        identifier(&item.id)?;
        anyhow::ensure!(ids.insert(&item.id) && spaces.contains_key(item.space_id.as_str()), "Duplicate item ID or missing Space.");
        validate_target(&item.target)?;
        anyhow::ensure!(targets.insert((&item.space_id, serde_json::to_string(&item.target)?)), "Duplicate reference in one Space.");
        positions.entry(&item.space_id).or_default().push(item.order);
    }
    for orders in positions.values_mut() { validate_orders(orders)?; }
    Ok(())
}

fn validate_orders(orders: &mut [u32]) -> Result<()> {
    orders.sort_unstable();
    anyhow::ensure!(orders.iter().copied().eq(0..orders.len() as u32), "Space ordering must be a complete contiguous sequence.");
    Ok(())
}

fn read_snapshot(conn: &Connection) -> Result<SpaceSnapshot> {
    let mut stmt = conn.prepare("SELECT id,parent_id,name,icon,color,position,created_at FROM local_spaces ORDER BY parent_id,position,id")?;
    let spaces = stmt.query_map([], |row| Ok(Space { id: row.get(0)?, parent_id: row.get(1)?, name: row.get(2)?,
        icon: row.get(3)?, color: row.get(4)?, order: row.get(5)?, created_at: row.get(6)? }))?.collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare("SELECT id,space_id,target,position FROM local_space_items ORDER BY space_id,position,id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, u32>(3)?)))?;
    let mut items = Vec::new();
    for row in rows { let (id, space_id, target, order) = row?; items.push(SpaceItem { id, space_id, target: serde_json::from_str(&target)?, order }); }
    let snapshot = SpaceSnapshot { spaces, items };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn write_snapshot(conn: &Connection, snapshot: &SpaceSnapshot) -> Result<()> {
    validate_snapshot(snapshot)?;
    conn.execute("DELETE FROM local_space_items", [])?;
    conn.execute("DELETE FROM local_spaces", [])?;
    let mut stmt = conn.prepare("INSERT INTO local_spaces(id,parent_id,name,icon,color,position,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
    for space in &snapshot.spaces {
        stmt.execute(params![space.id, space.parent_id, space.name, space.icon, space.color, space.order, space.created_at])?;
    }
    let mut stmt = conn.prepare("INSERT INTO local_space_items(id,space_id,target,position) VALUES (?1,?2,?3,?4)")?;
    for item in &snapshot.items { stmt.execute(params![item.id, item.space_id, serde_json::to_string(&item.target)?, item.order])?; }
    Ok(())
}

fn next_order(snapshot: &SpaceSnapshot, parent: &Option<String>) -> u32 {
    snapshot.spaces.iter().filter(|space| &space.parent_id == parent).count() as u32
}

fn compact(snapshot: &mut SpaceSnapshot) {
    snapshot.spaces.sort_by(|a, b| a.parent_id.cmp(&b.parent_id).then(a.order.cmp(&b.order)).then(a.id.cmp(&b.id)));
    let mut counts = HashMap::new();
    for space in &mut snapshot.spaces { let next = counts.entry(space.parent_id.clone()).or_insert(0); space.order = *next; *next += 1; }
    snapshot.items.sort_by(|a, b| a.space_id.cmp(&b.space_id).then(a.order.cmp(&b.order)).then(a.id.cmp(&b.id)));
    let mut counts = HashMap::new();
    for item in &mut snapshot.items { let next = counts.entry(item.space_id.clone()).or_insert(0); item.order = *next; *next += 1; }
}

fn space_mut<'a>(snapshot: &'a mut SpaceSnapshot, id: &str) -> Result<&'a mut Space> {
    snapshot.spaces.iter_mut().find(|space| space.id == id).ok_or_else(|| anyhow::anyhow!("Space no longer exists."))
}

fn reorder_spaces(snapshot: &mut SpaceSnapshot, parent: Option<String>, ids: Vec<String>) -> Result<()> {
    let mut expected: Vec<_> = snapshot.spaces.iter().filter(|space| space.parent_id == parent).map(|space| space.id.clone()).collect();
    let mut sorted = ids.clone(); expected.sort(); sorted.sort();
    anyhow::ensure!(expected == sorted, "Reorder must name every sibling exactly once.");
    if let Some(parent) = &parent { space_mut(snapshot, parent)?; }
    for (order, id) in ids.iter().enumerate() { space_mut(snapshot, id)?.order = order as u32; }
    Ok(())
}

fn reorder_items(snapshot: &mut SpaceSnapshot, space_id: String, ids: Vec<String>) -> Result<()> {
    space_mut(snapshot, &space_id)?;
    let mut expected: Vec<_> = snapshot.items.iter().filter(|item| item.space_id == space_id).map(|item| item.id.clone()).collect();
    let mut sorted = ids.clone(); expected.sort(); sorted.sort();
    anyhow::ensure!(expected == sorted, "Reorder must name every Space item exactly once.");
    for (order, id) in ids.iter().enumerate() {
        snapshot.items.iter_mut().find(|item| &item.id == id).unwrap().order = order as u32;
    }
    Ok(())
}

fn apply_action(conn: &Connection, snapshot: &mut SpaceSnapshot, action: SpaceAction, now: i64) -> Result<()> {
    match action {
        SpaceAction::Create { id, parent_id, name, icon, color } => {
            let order = next_order(snapshot, &parent_id);
            snapshot.spaces.push(Space { id, parent_id, name: name.trim().into(), icon, color, order, created_at: now });
        }
        SpaceAction::Rename { id, name } => space_mut(snapshot, &id)?.name = name.trim().into(),
        SpaceAction::Reparent { id, parent_id } => {
            if space_mut(snapshot, &id)?.parent_id != parent_id {
                let order = next_order(snapshot, &parent_id);
                let space = space_mut(snapshot, &id)?; space.parent_id = parent_id; space.order = order;
            }
        }
        SpaceAction::Reorder { parent_id, ids } => reorder_spaces(snapshot, parent_id, ids)?,
        SpaceAction::Delete { id } => {
            let parent = space_mut(snapshot, &id)?.parent_id.clone();
            snapshot.spaces.retain(|space| space.id != id);
            compact(snapshot);
            let mut order = next_order(snapshot, &parent);
            for child in snapshot.spaces.iter_mut().filter(|space| space.parent_id.as_deref() == Some(&id)) {
                child.parent_id = parent.clone(); child.order = order; order += 1;
            }
            snapshot.items.retain(|item| item.space_id != id);
        }
        SpaceAction::AddItem { id, space_id, target } => {
            let target = canonical_target(conn, target)?;
            for item in snapshot.items.iter().filter(|item| item.space_id == space_id) {
                anyhow::ensure!(canonical_target(conn, item.target.clone())? != target, "Duplicate reference in one Space.");
            }
            let order = snapshot.items.iter().filter(|item| item.space_id == space_id).count() as u32;
            snapshot.items.push(SpaceItem { id, space_id, target, order });
        }
        SpaceAction::RemoveItem { id } => {
            anyhow::ensure!(snapshot.items.iter().any(|item| item.id == id), "Space item no longer exists.");
            snapshot.items.retain(|item| item.id != id);
        }
        SpaceAction::ReorderItems { space_id, ids } => reorder_items(snapshot, space_id, ids)?,
    }
    compact(snapshot);
    validate_snapshot(snapshot)
}

impl MessageStore {
    pub fn spaces_snapshot(&self) -> Result<SpaceSnapshot> { read_snapshot(&self.conn.lock().unwrap()) }

    pub fn spaces_action(&self, action: SpaceAction, now: i64) -> Result<SpaceSnapshot> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let mut snapshot = read_snapshot(&tx)?;
        apply_action(&tx, &mut snapshot, action, now)?;
        write_snapshot(&tx, &snapshot)?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn export_spaces(&self) -> Result<SpaceArchive> {
        Ok(SpaceArchive { version: 1, snapshot: self.spaces_snapshot()? })
    }

    pub fn import_spaces(&self, mut archive: SpaceArchive) -> Result<SpaceSnapshot> {
        anyhow::ensure!(archive.version == 1, "Unsupported Space metadata version.");
        validate_snapshot(&archive.snapshot)?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let mut snapshot = read_snapshot(&tx)?;
        let offset = next_order(&snapshot, &None);
        for space in &mut archive.snapshot.spaces { if space.parent_id.is_none() { space.order += offset; } }
        for item in &mut archive.snapshot.items { item.target = canonical_target(&tx, item.target.clone())?; }
        snapshot.spaces.extend(archive.snapshot.spaces);
        snapshot.items.extend(archive.snapshot.items);
        validate_snapshot(&snapshot)?;
        write_snapshot(&tx, &snapshot)?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn cache_space_groups(&self, groups: &[CachedSpaceGroup]) -> Result<()> {
        anyhow::ensure!(groups.len() <= MAX_ITEMS, "Group catalog exceeds local bounds.");
        let mut seen = HashSet::new();
        for group in groups {
            anyhow::ensure!(address(&group.jid)?.is_group() && seen.insert(&group.jid), "Invalid or duplicate cached group.");
            if let Some(subject) = &group.subject {
                anyhow::ensure!(subject.len() <= 4096 && !subject.contains('\0'), "Invalid cached group subject.");
            }
            if let Some(parent) = &group.parent {
                anyhow::ensure!(address(parent)?.is_group() && parent != &group.jid && !group.community, "Invalid cached community parent.");
                anyhow::ensure!(groups.iter().find(|candidate| &candidate.jid == parent).is_none_or(|candidate| candidate.community),
                    "Cached subgroup parent is not a community.");
            }
            anyhow::ensure!(!group.announcements || group.parent.is_some(), "Announcement group has no community parent.");
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        tx.execute("DELETE FROM cached_group_catalog", [])?;
        for group in groups {
            tx.execute("INSERT INTO cached_group_catalog(jid,subject,parent,community,announcements) VALUES (?1,?2,?3,?4,?5)",
                params![group.jid, group.subject, group.parent, group.community, group.announcements])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn cached_space_groups(&self) -> Result<Vec<CachedSpaceGroup>> {
        read_groups(&self.conn.lock().unwrap())
    }
}

fn read_groups(conn: &Connection) -> Result<Vec<CachedSpaceGroup>> {
    let mut stmt = conn.prepare("SELECT jid,subject,parent,community,announcements FROM cached_group_catalog ORDER BY jid")?;
    let rows = stmt.query_map([], |row| Ok(CachedSpaceGroup { jid: row.get(0)?, subject: row.get(1)?, parent: row.get(2)?,
        community: row.get(3)?, announcements: row.get(4)? }))?.collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

const PUBLIC_CONTENT: &str = "m.deleted=0 AND m.revoked=0 AND m.spoiler=0
    AND m.media_once_kind IS NULL AND COALESCE(m.media_kind,'') NOT IN ('view_once','unknown')
    AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id)
    AND NOT EXISTS(SELECT 1 FROM hidden_chats h WHERE h.jid=m.chat)";

struct Resolver<'a> {
    conn: &'a Connection,
    chats: &'a [ChatSummary],
    known: HashSet<String>,
    hidden: HashSet<String>,
    groups: Vec<CachedSpaceGroup>,
    labels: HashSet<String>,
    chat_labels: HashMap<String, HashSet<String>>,
    aliases: HashMap<String, String>,
    keyword_hits: HashSet<String>,
    now: i64,
}

fn strings(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<String>> {
    Ok(conn.prepare(sql)?.query_map(params, |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn canonical(conn: &Connection, jid: &str) -> Result<String> { Ok(names::canonical_chat(conn, jid)?.into_owned()) }

fn query_whitespace(ch: char) -> bool { ch.is_whitespace() || ch == '\u{feff}' }

fn scoped_label_query(query: &str) -> Option<(String, String)> {
    let mut previous = None;
    for (index, ch) in query.char_indices() {
        let boundary = previous.is_none_or(|(_, ch)| query_whitespace(ch));
        let start = previous.map(|(index, _)| index).unwrap_or(0);
        previous = Some((index, ch));
        if !boundary || !query.get(index..index + 6).is_some_and(|token| token.eq_ignore_ascii_case("label:")) { continue; }
        let value = &query[index + 6..];
        let quoted = value.as_bytes().first().filter(|quote| matches!(**quote, b'"' | b'\''))
            .and_then(|quote| value[1..].find(*quote as char)).filter(|end| *end > 0);
        let (label, consumed) = if let Some(end) = quoted { (&value[1..end + 1], end + 2) } else {
            let end = value.find(query_whitespace).unwrap_or(value.len());
            if end == 0 { continue; }
            (&value[..end], end)
        };
        let remainder = format!("{} {}", &query[..start], &query[index + 6 + consumed..]);
        return Some((label.trim_matches(query_whitespace).into(), remainder.trim_matches(query_whitespace).into()));
    }
    None
}

impl<'a> Resolver<'a> {
    fn new(conn: &'a Connection, chats: &'a [ChatSummary], aliases: &[(String, String)], now: i64,
        keyword_counts: &HashMap<String, u32>) -> Result<Self> {
        let known = strings(conn, "SELECT jid FROM chats UNION SELECT jid FROM names
            UNION SELECT jid FROM contact_identity UNION SELECT jid FROM cached_group_catalog", [])?
            .iter().map(|jid| canonical(conn, jid)).collect::<Result<_>>()?;
        let hidden = strings(conn, "SELECT jid FROM hidden_chats", [])?
            .iter().map(|jid| canonical(conn, jid)).collect::<Result<_>>()?;
        let labels = strings(conn, "SELECT id FROM labels_catalog WHERE deleted=0 AND name<>''", [])?.into_iter().collect();
        let mut chat_labels: HashMap<String, HashSet<String>> = HashMap::new();
        let mut stmt = conn.prepare("SELECT a.chat,a.label_id FROM chat_labels a JOIN labels_catalog l ON l.id=a.label_id
            WHERE a.labeled=1 AND l.deleted=0 AND l.name<>''")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows { let (chat, label) = row?; chat_labels.entry(canonical(conn, &chat)?).or_default().insert(label); }
        let aliases = aliases.iter().map(|(jid, name)| Ok((canonical(conn, jid)?, name.clone()))).collect::<Result<_>>()?;
        let mut keyword_hits = HashSet::new();
        for (jid, count) in keyword_counts {
            address(jid)?;
            if *count > 0 { keyword_hits.insert(canonical(conn, jid)?); }
        }
        Ok(Self { conn, chats, known, hidden, groups: read_groups(conn)?, labels, chat_labels, aliases, keyword_hits, now })
    }

    fn visible(&self, jid: &str) -> Result<Option<String>> {
        let jid = canonical(self.conn, jid)?;
        Ok((!self.hidden.contains(&jid)).then_some(jid))
    }

    fn ordered(&self, ids: Vec<String>) -> Result<Vec<String>> {
        let rank: HashMap<_, _> = self.chats.iter().enumerate().map(|(index, chat)| (chat.chat.as_str(), index)).collect();
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for id in ids { if let Some(id) = self.visible(&id)? { if seen.insert(id.clone()) { result.push(id); } } }
        result.sort_by(|a, b| rank.get(a.as_str()).copied().unwrap_or(usize::MAX).cmp(&rank.get(b.as_str()).copied().unwrap_or(usize::MAX))
            .then(a.cmp(b)));
        Ok(result)
    }

    fn reference(&self, jid: &str) -> Result<(Vec<String>, Option<String>)> {
        let jid = canonical(self.conn, jid)?;
        if self.hidden.contains(&jid) || !self.known.contains(&jid) {
            return Ok((Vec::new(), Some("Conversation is unavailable locally.".into())));
        }
        if self.groups.iter().any(|group| group.jid == jid && group.community) {
            return Ok((Vec::new(), Some("Community parents have no conversation; use a community reference.".into())));
        }
        Ok((vec![jid], None))
    }

    fn community(&self, jid: &str) -> Result<(Vec<String>, Option<String>)> {
        if self.hidden.contains(jid) || !self.groups.iter().any(|group| group.jid == jid && group.community) {
            return Ok((Vec::new(), Some("Community hierarchy is unavailable in the local cache.".into())));
        }
        Ok((self.groups.iter().filter(|group| group.parent.as_deref() == Some(jid))
            .map(|group| group.jid.clone()).collect(), None))
    }

    fn label(&self, label: &str) -> Result<(Vec<String>, Option<String>)> {
        if !self.labels.contains(label) { return Ok((Vec::new(), Some("Label is unavailable locally.".into()))); }
        let sql = format!("SELECT chat FROM chat_labels WHERE label_id=?1 AND labeled=1
            UNION SELECT m.chat FROM message_labels a JOIN messages m ON m.chat=a.chat AND m.id=a.message_id
            WHERE a.label_id=?1 AND a.labeled=1 AND m.system_kind IS NULL AND ({PUBLIC_CONTENT})");
        Ok((strings(self.conn, &sql, [label])?, None))
    }

    fn message(&self, chat: &str, id: &str) -> Result<(Vec<String>, Option<String>)> {
        let chat = canonical(self.conn, chat)?;
        let sql = format!("SELECT m.chat FROM messages m WHERE m.chat=?1 AND m.id=?2 AND ({PUBLIC_CONTENT})
            AND COALESCE(m.system_kind,'')<>'UNAVAILABLE_MESSAGE'");
        let chats = strings(self.conn, &sql, params![chat, id])?;
        let unavailable = chats.is_empty().then(|| "Saved message is missing or private.".into());
        Ok((chats, unavailable))
    }

    fn search(&self, query: &str, chat: &Option<String>) -> Result<(Vec<String>, Option<String>)> {
        let chat = chat.as_ref().map(|chat| canonical(self.conn, chat)).transpose()?;
        if chat.as_ref().is_some_and(|chat| self.hidden.contains(chat) || !self.known.contains(chat)) {
            return Ok((Vec::new(), Some("Search conversation is unavailable locally.".into())));
        }
        let parsed = chat.as_ref().and_then(|_| scoped_label_query(query));
        let mut label_ids = None;
        if let Some((label, _)) = &parsed {
            let mut stmt = self.conn.prepare("SELECT id,name FROM labels_catalog WHERE deleted=0 AND name<>''")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            let mut ids = Vec::new();
            for row in rows { let (id, name) = row?; if name.to_lowercase() == label.to_lowercase() { ids.push(id); } }
            if ids.is_empty() { return Ok((Vec::new(), Some("Search label is unavailable locally.".into()))); }
            label_ids = Some(serde_json::to_string(&ids)?);
        }
        let query = parsed.as_ref().map(|(_, query)| query.as_str())
            .unwrap_or_else(|| if chat.is_some() { query } else { query.trim_matches(query_whitespace) });
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_").to_lowercase();
        let pattern = format!("%{escaped}%");
        let sql = format!("SELECT DISTINCT m.chat FROM messages m WHERE ({PUBLIC_CONTENT}) AND m.system_kind IS NULL
            AND (?1 IS NULL OR m.chat=?1) AND (lower(m.text) LIKE ?2 ESCAPE '\\' OR lower(m.link_urls) LIKE ?2 ESCAPE '\\')
            AND (?3 IS NULL OR EXISTS(SELECT 1 FROM message_labels a WHERE a.chat=m.chat AND a.message_id=m.id
                AND a.labeled=1 AND a.label_id IN (SELECT value FROM json_each(?3))))");
        Ok((strings(self.conn, &sql, params![chat, pattern, label_ids])?, None))
    }

    fn inbox(&self, filters: &SpaceInboxFilters) -> Result<(Vec<String>, Option<String>)> {
        if !filters.label.is_empty() && !self.labels.contains(&filters.label) {
            return Ok((Vec::new(), Some("Inbox label is unavailable locally.".into())));
        }
        let query = filters.query.trim().to_lowercase();
        let chats = self.chats.iter().filter(|chat| {
            let labels = self.chat_labels.get(&chat.chat);
            let categories = [chat.unread_count > 0 || chat.marked_unread,
                chat.mention_count > 0 || self.keyword_hits.contains(&chat.chat),
                labels.is_some_and(|labels| !labels.is_empty()), chat.muted_until < 0 || chat.muted_until > self.now, chat.archived];
            let selected = [filters.unread, filters.mentions, filters.labelled, filters.muted, filters.archived];
            let matches = if selected.iter().any(|value| *value) {
                selected.iter().zip(categories).all(|(selected, category)| !selected || category)
            } else { categories.iter().any(|category| *category) };
            let name = self.aliases.get(&chat.chat).or(chat.display_name.as_ref()).unwrap_or(&chat.chat);
            matches && (filters.label.is_empty() || labels.is_some_and(|labels| labels.contains(&filters.label)))
                && (query.is_empty() || name.to_lowercase().contains(&query) || chat.chat.to_lowercase().contains(&query))
        }).map(|chat| chat.chat.clone()).collect();
        Ok((chats, None))
    }

    fn resolve(&self, target: &SpaceTarget) -> Result<(Vec<String>, Option<String>)> {
        match target {
            SpaceTarget::Chat { jid } | SpaceTarget::Group { jid } | SpaceTarget::Channel { jid }
            | SpaceTarget::Contact { jid } | SpaceTarget::FavoriteContact { jid } => self.reference(jid),
            SpaceTarget::Community { jid } => self.community(jid),
            SpaceTarget::Label { label_id } => self.label(label_id),
            SpaceTarget::SavedMessage { chat, message_id } => self.message(chat, message_id),
            SpaceTarget::SavedSearch { query, chat } => self.search(query, chat),
            SpaceTarget::InboxView { filters } => self.inbox(filters),
        }
    }
}

fn selected_items<'a>(snapshot: &'a SpaceSnapshot, id: &str) -> Result<Vec<&'a SpaceItem>> {
    anyhow::ensure!(snapshot.spaces.iter().any(|space| space.id == id), "Space no longer exists.");
    let mut pending = vec![id];
    let mut items = Vec::new();
    while let Some(id) = pending.pop() {
        items.extend(snapshot.items.iter().filter(|item| item.space_id == id));
        let mut children: Vec<_> = snapshot.spaces.iter().filter(|space| space.parent_id.as_deref() == Some(id)).collect();
        children.sort_by_key(|space| space.order);
        pending.extend(children.iter().rev().map(|space| space.id.as_str()));
    }
    Ok(items)
}

impl MessageStore {
    pub fn resolve_spaces(&self, selection: &SpaceSelection, now: i64, aliases: &[(String, String)]) -> Result<SpaceResolution> {
        self.resolve_spaces_with_keywords(selection, now, aliases, &HashMap::new())
    }

    pub fn resolve_spaces_with_keywords(&self, selection: &SpaceSelection, now: i64, aliases: &[(String, String)],
        keyword_counts: &HashMap<String, u32>) -> Result<SpaceResolution> {
        let chats = self.chats()?;
        if matches!(selection, SpaceSelection::All) {
            return Ok(SpaceResolution { chats: chats.into_iter().map(|chat| chat.chat).collect(), items: Vec::new() });
        }
        let conn = self.conn.lock().unwrap();
        let snapshot = read_snapshot(&conn)?;
        let resolver = Resolver::new(&conn, &chats, aliases, now, keyword_counts)?;
        let selected = match selection {
            SpaceSelection::Space { space_id } => selected_items(&snapshot, space_id)?,
            _ => snapshot.items.iter().collect(),
        };
        let mut items = Vec::new();
        let mut assigned = HashSet::new();
        let mut resolved = Vec::new();
        for item in selected {
            let (matches, unavailable) = resolver.resolve(&item.target)?;
            let matches = resolver.ordered(matches)?;
            for chat in &matches { if assigned.insert(chat.clone()) { resolved.push(chat.clone()); } }
            items.push(ResolvedSpaceItem { item_id: item.id.clone(), chats: matches, unavailable });
        }
        if matches!(selection, SpaceSelection::Unsorted) {
            resolved = chats.into_iter().filter(|chat| !assigned.contains(&chat.chat)).map(|chat| chat.chat).collect();
        }
        Ok(SpaceResolution { chats: resolved, items })
    }
}

#[cfg(test)]
#[path = "spaces_tests.rs"]
mod tests;
