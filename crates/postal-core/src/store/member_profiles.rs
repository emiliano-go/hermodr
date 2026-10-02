use super::contact_identity::ContactIdentity;
use super::*;
use whatsapp_rust::wacore_binary::JidExt;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberNote {
    pub text: String,
    pub warnings: u32,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberMessageStats {
    pub total: u64,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
    pub media_total: u64,
    pub reactions_sent: u64,
    pub times_mentioned: u64,
    pub mention_contexts_recorded: u64,
    pub group_mention_contexts_recorded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberRosterEntry {
    pub jid: String,
    pub admin: bool,
    pub owner: bool,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct CachedMemberGroup {
    pub chat: String,
    pub subject: Option<String>,
    pub observed_at: i64,
    pub present: Option<bool>,
    pub admin: Option<bool>,
    pub owner: Option<bool>,
    pub label: Option<String>,
    pub own_admin: Option<bool>,
    pub member_observed_at: Option<i64>,
    pub complete_snapshot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberJoinEvidence {
    pub timestamp: i64,
    pub actor: Option<String>,
    pub kind: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberSignals {
    pub online: Option<bool>,
    pub last_seen: Option<i64>,
    pub presence_at: Option<i64>,
    pub typing: Option<String>,
    pub typing_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberProfileLocal {
    pub jid: String,
    pub addresses: Vec<String>,
    pub identity: ContactIdentity,
    pub pn_jid: Option<String>,
    pub lid_jid: Option<String>,
    pub scope_chat: Option<String>,
    pub stats: MemberMessageStats,
    pub note: MemberNote,
    pub group: Option<CachedMemberGroup>,
    pub join: Option<MemberJoinEvidence>,
    pub mutual_groups: Vec<CachedMemberGroup>,
    pub signals: MemberSignals,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS member_notes (
        chat TEXT NOT NULL, jid TEXT NOT NULL, note TEXT NOT NULL, warnings INTEGER NOT NULL,
        updated_at INTEGER NOT NULL, PRIMARY KEY(chat,jid));
        CREATE TABLE IF NOT EXISTS member_group_profiles (
        chat TEXT PRIMARY KEY, subject TEXT, observed_at INTEGER NOT NULL, own_admin INTEGER, complete INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS member_group_rosters (
        chat TEXT NOT NULL, jid TEXT NOT NULL, admin INTEGER, owner INTEGER,
        label TEXT, observed_at INTEGER NOT NULL, PRIMARY KEY(chat,jid));
        CREATE TABLE IF NOT EXISTS member_live_profiles (jid TEXT PRIMARY KEY, payload TEXT NOT NULL, observed_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS member_profile_signals (
        kind TEXT NOT NULL, chat TEXT NOT NULL, jid TEXT NOT NULL, online INTEGER, last_seen INTEGER,
        state TEXT, observed_at INTEGER NOT NULL, PRIMARY KEY(kind,chat,jid));
        CREATE TABLE IF NOT EXISTS member_mention_contexts (chat TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY(chat,id));
        CREATE TABLE IF NOT EXISTS member_mentions (chat TEXT NOT NULL, id TEXT NOT NULL, jid TEXT NOT NULL, PRIMARY KEY(chat,id,jid));
        CREATE TABLE IF NOT EXISTS member_mention_groups (chat TEXT NOT NULL, id TEXT NOT NULL, group_jid TEXT NOT NULL, PRIMARY KEY(chat,id,group_jid));
        CREATE TRIGGER IF NOT EXISTS member_context_delete AFTER DELETE ON messages BEGIN
            DELETE FROM member_mentions WHERE chat=OLD.chat AND id=OLD.id;
            DELETE FROM member_mention_contexts WHERE chat=OLD.chat AND id=OLD.id;
            DELETE FROM member_mention_groups WHERE chat=OLD.chat AND id=OLD.id;
        END;
        CREATE TRIGGER IF NOT EXISTS member_context_private AFTER UPDATE OF spoiler,revoked,deleted,media_kind,media_once_kind,system_kind ON messages
        WHEN NEW.spoiler<>0 OR NEW.revoked<>0 OR NEW.deleted<>0 OR NEW.media_once_kind IS NOT NULL
            OR NEW.media_kind IN ('view_once','unknown') OR NEW.system_kind='UNAVAILABLE_MESSAGE' BEGIN
            DELETE FROM member_mentions WHERE chat=NEW.chat AND id=NEW.id;
            DELETE FROM member_mention_contexts WHERE chat=NEW.chat AND id=NEW.id;
            DELETE FROM member_mention_groups WHERE chat=NEW.chat AND id=NEW.id;
        END;
        CREATE TRIGGER IF NOT EXISTS member_context_once AFTER INSERT ON view_once BEGIN
            DELETE FROM member_mentions WHERE chat=NEW.chat AND id=NEW.id;
            DELETE FROM member_mention_contexts WHERE chat=NEW.chat AND id=NEW.id;
            DELETE FROM member_mention_groups WHERE chat=NEW.chat AND id=NEW.id;
        END;")?;
    Ok(())
}

pub(crate) fn member_address(value: &str) -> Result<String> {
    let jid: whatsapp_rust::prelude::Jid = value.parse()?;
    anyhow::ensure!(
        (jid.is_pn() || jid.is_lid())
            && jid.device == 0
            && jid.agent == 0
            && jid.integrator == 0
            && !jid.user.is_empty()
            && jid.user.len() <= 32
            && jid.user.bytes().all(|byte| byte.is_ascii_digit()),
        "Invalid member address."
    );
    Ok(jid.to_string())
}

fn profile_scope(chat: Option<&str>) -> Result<String> {
    let Some(chat) = chat else {
        return Ok(String::new());
    };
    let group: whatsapp_rust::prelude::Jid = chat.parse()?;
    anyhow::ensure!(
        (group.is_group() || group.is_pn() || group.is_lid())
            && !group.user.is_empty()
            && group.device == 0
            && group.agent == 0
            && group.integrator == 0,
        "Profile scope must be a bare chat address."
    );
    Ok(group.to_string())
}

const VISIBLE_MESSAGE: &str =
    "m.system_kind IS NULL AND m.spoiler=0 AND m.revoked=0 AND m.deleted=0
    AND m.media_once_kind IS NULL AND COALESCE(m.media_kind,'') NOT IN ('view_once','unknown')
    AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id)";

fn member_context_eligible(conn: &Connection, chat: &str, id: &str) -> Result<bool> {
    conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM messages m WHERE m.chat=?1 AND m.id=?2 AND {VISIBLE_MESSAGE})"),
        params![chat,id], |row|row.get(0)).map_err(Into::into)
}

fn member_stats(
    conn: &Connection,
    encoded: &str,
    scope: &str,
    is_own: bool,
) -> Result<MemberMessageStats> {
    let mut stats = conn.query_row(&format!("SELECT COUNT(*),MIN(timestamp),MAX(timestamp),
        COALESCE(SUM(media_kind IN ('image','video','round_video','gif','audio','document','sticker','music')),0)
        FROM messages m WHERE sender IN (SELECT value FROM json_each(?1)) AND (?2='' OR chat=?2) AND {VISIBLE_MESSAGE}"),
        params![encoded,scope], |row| Ok(MemberMessageStats { total:row.get::<_,i64>(0)? as u64,first_at:row.get(1)?,last_at:row.get(2)?,media_total:row.get::<_,i64>(3)? as u64,..Default::default() }))?;
    stats.reactions_sent = conn.query_row("SELECT COUNT(*) FROM reactions r WHERE (sender IN (SELECT value FROM json_each(?1)) OR (?3 AND sender='@me'))
        AND (?2='' OR chat=?2) AND emoji<>'' AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat=r.chat AND v.id=r.target)
        AND NOT EXISTS(SELECT 1 FROM messages m WHERE m.chat=r.chat AND m.id=r.target
            AND (m.spoiler<>0 OR m.revoked<>0 OR m.deleted<>0 OR m.media_once_kind IS NOT NULL
                OR m.media_kind IN ('view_once','unknown') OR m.system_kind='UNAVAILABLE_MESSAGE'))", params![encoded,scope,is_own], |row|row.get::<_,i64>(0).map(|value|value as u64))?;
    stats.times_mentioned = conn.query_row(&format!("SELECT COUNT(DISTINCT m.chat||char(0)||m.id) FROM member_mentions x
        JOIN messages m ON m.chat=x.chat AND m.id=x.id WHERE x.jid IN (SELECT value FROM json_each(?1))
        AND (?2='' OR m.chat=?2) AND {VISIBLE_MESSAGE}"), params![encoded,scope], |row|row.get::<_,i64>(0).map(|value|value as u64))?;
    stats.mention_contexts_recorded = conn.query_row(&format!("SELECT COUNT(*) FROM member_mention_contexts x
        JOIN messages m ON m.chat=x.chat AND m.id=x.id WHERE (?1='' OR m.chat=?1) AND {VISIBLE_MESSAGE}"), [scope], |row|row.get::<_,i64>(0).map(|value|value as u64))?;
    stats.group_mention_contexts_recorded = conn.query_row(&format!("SELECT COUNT(DISTINCT m.chat||char(0)||m.id) FROM member_mention_groups x
        JOIN messages m ON m.chat=x.chat AND m.id=x.id WHERE x.group_jid=m.chat AND (?1='' OR m.chat=?1) AND {VISIBLE_MESSAGE}"), [scope], |row|row.get::<_,i64>(0).map(|value|value as u64))?;
    Ok(stats)
}

fn mutual_groups(
    conn: &Connection,
    encoded: &str,
    own_encoded: &str,
) -> Result<Vec<CachedMemberGroup>> {
    let mut query = conn.prepare("SELECT DISTINCT p.chat FROM member_group_profiles p
        WHERE EXISTS(SELECT 1 FROM member_group_rosters r WHERE r.chat=p.chat AND r.jid IN (SELECT value FROM json_each(?1)))
        AND EXISTS(SELECT 1 FROM member_group_rosters r WHERE r.chat=p.chat AND r.jid IN (SELECT value FROM json_each(?2))) ORDER BY p.chat")?;
    let ids = query
        .query_map(params![encoded, own_encoded], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids
        .into_iter()
        .map(|chat| cached_group(conn, &chat, encoded))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    conn.execute("INSERT INTO member_notes(chat,jid,note,warnings,updated_at)
        SELECT CASE WHEN chat=?1 THEN ?2 ELSE chat END,CASE WHEN jid=?1 THEN ?2 ELSE jid END,note,warnings,updated_at
        FROM member_notes WHERE chat=?1 OR jid=?1 ON CONFLICT(chat,jid) DO UPDATE SET
            note=excluded.note,warnings=excluded.warnings,updated_at=excluded.updated_at WHERE excluded.updated_at>member_notes.updated_at", params![from,to])?;
    conn.execute("DELETE FROM member_notes WHERE chat=?1 OR jid=?1", [from])?;
    conn.execute("INSERT INTO member_group_rosters(chat,jid,admin,owner,label,observed_at)
        SELECT chat,?2,admin,owner,label,observed_at FROM member_group_rosters WHERE jid=?1
        ON CONFLICT(chat,jid) DO UPDATE SET admin=excluded.admin,owner=excluded.owner,label=excluded.label,
            observed_at=excluded.observed_at WHERE excluded.observed_at>member_group_rosters.observed_at", params![from,to])?;
    conn.execute("DELETE FROM member_group_rosters WHERE jid=?1", [from])?;
    conn.execute("INSERT INTO member_live_profiles(jid,payload,observed_at) SELECT ?2,payload,observed_at FROM member_live_profiles WHERE jid=?1
        ON CONFLICT(jid) DO UPDATE SET payload=excluded.payload,observed_at=excluded.observed_at
            WHERE excluded.observed_at>member_live_profiles.observed_at", params![from,to])?;
    conn.execute("DELETE FROM member_live_profiles WHERE jid=?1", [from])?;
    conn.execute("INSERT INTO member_profile_signals(kind,chat,jid,online,last_seen,state,observed_at)
        SELECT kind,CASE WHEN chat=?1 THEN ?2 ELSE chat END,CASE WHEN jid=?1 THEN ?2 ELSE jid END,online,last_seen,state,observed_at
        FROM member_profile_signals WHERE chat=?1 OR jid=?1 ON CONFLICT(kind,chat,jid) DO UPDATE SET online=excluded.online,
            last_seen=excluded.last_seen,state=excluded.state,observed_at=excluded.observed_at WHERE excluded.observed_at>member_profile_signals.observed_at", params![from,to])?;
    conn.execute(
        "DELETE FROM member_profile_signals WHERE chat=?1 OR jid=?1",
        [from],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO member_mentions(chat,id,jid)
        SELECT CASE WHEN chat=?1 THEN ?2 ELSE chat END,id,CASE WHEN jid=?1 THEN ?2 ELSE jid END
        FROM member_mentions WHERE chat=?1 OR jid=?1",
        params![from, to],
    )?;
    conn.execute(
        "DELETE FROM member_mentions WHERE chat=?1 OR jid=?1",
        [from],
    )?;
    for (table, columns) in [
        ("member_mention_contexts", "chat,id"),
        ("member_mention_groups", "chat,id,group_jid"),
    ] {
        let tail = columns.strip_prefix("chat,").unwrap();
        conn.execute(&format!("INSERT OR IGNORE INTO {table}({columns}) SELECT ?2,{tail} FROM {table} WHERE chat=?1"),params![from,to])?;
        conn.execute(&format!("DELETE FROM {table} WHERE chat=?1"), [from])?;
    }
    Ok(())
}

impl MessageStore {
    pub fn member_profile_local(
        &self,
        jid: &str,
        chat: Option<&str>,
        own: &str,
        now: i64,
    ) -> Result<MemberProfileLocal> {
        let jid = member_address(jid)?;
        let scope = profile_scope(chat)?;
        let forms = super::names::name_forms(self, &jid)?;
        let own_forms = if own.is_empty() {
            Vec::new()
        } else {
            super::names::name_forms(self, own)?
        };
        let is_own = forms.iter().any(|form| own_forms.contains(form));
        let mut identity = self.contact_identity(&jid)?;
        identity.own = is_own;
        let encoded = serde_json::to_string(&forms)?;
        let own_encoded = serde_json::to_string(&own_forms)?;
        let conn = self.conn.lock().unwrap();
        let stats = member_stats(&conn, &encoded, &scope, is_own)?;
        let note = conn.query_row("SELECT note,warnings,updated_at FROM member_notes WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?2))
            ORDER BY updated_at DESC LIMIT 1", params![scope,encoded], |row| Ok(MemberNote { text: row.get(0)?, warnings: row.get(1)?, updated_at: Some(row.get(2)?) }))
            .optional()?.unwrap_or_default();
        let group = if scope.ends_with("@g.us") {
            cached_group(&conn, &scope, &encoded)?
        } else {
            None
        };
        let join = if scope.ends_with("@g.us") {
            latest_join(&conn, &scope, &encoded)?
        } else {
            None
        };
        let mutual_groups = mutual_groups(&conn, &encoded, &own_encoded)?;
        let signals = member_signals(&conn, &encoded, &scope, now)?;
        Ok(MemberProfileLocal {
            jid,
            pn_jid: forms
                .iter()
                .find(|form| form.ends_with("@s.whatsapp.net"))
                .cloned(),
            lid_jid: forms.iter().find(|form| form.ends_with("@lid")).cloned(),
            addresses: forms,
            identity,
            scope_chat: (!scope.is_empty()).then_some(scope),
            stats,
            note,
            group,
            join,
            mutual_groups,
            signals,
        })
    }

    pub fn set_member_note(
        &self,
        jid: &str,
        chat: Option<&str>,
        note: &str,
        warnings: u32,
        at: i64,
    ) -> Result<MemberNote> {
        let jid = member_address(jid)?;
        let scope = profile_scope(chat)?;
        anyhow::ensure!(
            note.len() <= 16384
                && note.chars().count() <= 4096
                && !note.contains('\0')
                && warnings <= 100000,
            "Member note or warning count exceeds the local limit."
        );
        let forms = super::names::name_forms(self, &jid)?;
        let canonical = forms
            .iter()
            .find(|form| form.ends_with("@s.whatsapp.net"))
            .unwrap_or(&jid);
        let encoded = serde_json::to_string(&forms)?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        tx.execute(
            "DELETE FROM member_notes WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?2))",
            params![scope, encoded],
        )?;
        tx.execute(
            "INSERT INTO member_notes(chat,jid,note,warnings,updated_at) VALUES(?1,?2,?3,?4,?5)",
            params![scope, canonical, note, warnings, at],
        )?;
        tx.commit()?;
        Ok(MemberNote {
            text: note.into(),
            warnings,
            updated_at: Some(at),
        })
    }

    pub fn record_group_profile_snapshot(
        &self,
        chat: &str,
        subject: Option<&str>,
        roster: &[MemberRosterEntry],
        own_admin: bool,
        at: i64,
    ) -> Result<()> {
        let chat = profile_scope(Some(chat))?;
        anyhow::ensure!(chat.ends_with("@g.us"), "Roster snapshots require a group.");
        anyhow::ensure!(
            roster.len() <= 4096 && subject.is_none_or(|value| value.len() <= 4096),
            "Group profile snapshot is too large."
        );
        for member in roster {
            member_address(&member.jid)?;
            anyhow::ensure!(
                member
                    .label
                    .as_ref()
                    .is_none_or(|label| label.len() <= 4096),
                "Member tag is too large."
            );
        }
        let mut conn = self.conn.lock().unwrap();
        let old: Option<i64> = conn
            .query_row(
                "SELECT observed_at FROM member_group_profiles WHERE chat=?1",
                [&chat],
                |row| row.get(0),
            )
            .optional()?;
        if old.is_some_and(|old| old > at) {
            return Ok(());
        }
        let tx = conn.savepoint()?;
        tx.execute("INSERT INTO member_group_profiles(chat,subject,observed_at,own_admin,complete) VALUES(?1,?2,?3,?4,1)
            ON CONFLICT(chat) DO UPDATE SET subject=excluded.subject,observed_at=excluded.observed_at,own_admin=excluded.own_admin,complete=1", params![chat,subject,at,own_admin])?;
        tx.execute("DELETE FROM member_group_rosters WHERE chat=?1", [&chat])?;
        for member in roster {
            tx.execute("INSERT OR REPLACE INTO member_group_rosters(chat,jid,admin,owner,label,observed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![chat,member.jid,member.admin,member.owner,member.label,at])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_member_change(
        &self,
        chat: &str,
        jid: &str,
        action: &str,
        label: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let chat = profile_scope(Some(chat))?;
        anyhow::ensure!(
            chat.ends_with("@g.us"),
            "Observed membership changes require a group."
        );
        let jid = member_address(jid)?;
        anyhow::ensure!(
            label.is_none_or(|label| label.len() <= 4096),
            "Member tag is too large."
        );
        anyhow::ensure!(
            matches!(action, "remove" | "promote" | "demote" | "label" | "add"),
            "Unsupported observed member change."
        );
        let forms = serde_json::to_string(&super::names::name_forms(self, &jid)?)?;
        let conn = self.conn.lock().unwrap();
        let observed: Option<i64> = conn
            .query_row(
                "SELECT observed_at FROM member_group_profiles WHERE chat=?1",
                [&chat],
                |row| row.get(0),
            )
            .optional()?;
        if observed.is_some_and(|observed| observed > at) {
            return Ok(());
        }
        conn.execute("INSERT INTO member_group_profiles(chat,subject,observed_at,own_admin,complete) VALUES(?1,NULL,?2,NULL,0)
            ON CONFLICT(chat) DO UPDATE SET observed_at=MAX(observed_at,excluded.observed_at)", params![chat,at])?;
        match action {
            "remove" => {
                conn.execute("DELETE FROM member_group_rosters WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?2)) AND observed_at<=?3", params![chat,forms,at])?;
            }
            "promote" | "demote" => {
                conn.execute("UPDATE member_group_rosters SET admin=?3,observed_at=?4 WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?2)) AND observed_at<=?4", params![chat,forms,action=="promote",at])?;
            }
            "label" => {
                conn.execute("UPDATE member_group_rosters SET label=?3,observed_at=?4 WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?2)) AND observed_at<=?4", params![chat,forms,label,at])?;
            }
            "add" => {
                conn.execute("INSERT INTO member_group_rosters(chat,jid,admin,owner,label,observed_at) VALUES(?1,?2,NULL,NULL,NULL,?3)
                ON CONFLICT(chat,jid) DO NOTHING", params![chat,jid,at])?;
            }
            _ => unreachable!(),
        }
        if matches!(action, "promote" | "demote" | "label") {
            let admin = if action == "label" {
                None
            } else {
                Some(action == "promote")
            };
            conn.execute("INSERT INTO member_group_rosters(chat,jid,admin,owner,label,observed_at)
                SELECT ?1,?2,?3,NULL,?4,?5 WHERE NOT EXISTS(SELECT 1 FROM member_group_rosters WHERE chat=?1 AND jid IN (SELECT value FROM json_each(?6)))",
                params![chat,jid,admin,label,at,forms])?;
        }
        Ok(())
    }

    pub fn record_member_mentions(&self, chat: &str, id: &str, targets: &[String]) -> Result<()> {
        let chat = profile_scope(Some(chat))?;
        anyhow::ensure!(
            !id.is_empty() && id.len() <= 256 && !id.contains('\0') && targets.len() <= 4096,
            "Invalid observed mention context."
        );
        for target in targets {
            member_address(target)?;
        }
        let mut conn = self.conn.lock().unwrap();
        if !member_context_eligible(&conn, &chat, id)? {
            return Ok(());
        }
        let tx = conn.savepoint()?;
        tx.execute(
            "INSERT OR IGNORE INTO member_mention_contexts(chat,id) VALUES(?1,?2)",
            params![chat, id],
        )?;
        for target in targets {
            tx.execute(
                "INSERT OR IGNORE INTO member_mentions(chat,id,jid) VALUES(?1,?2,?3)",
                params![chat, id, target],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_member_group_mentions(
        &self,
        chat: &str,
        id: &str,
        groups: &[String],
    ) -> Result<()> {
        let chat = profile_scope(Some(chat))?;
        anyhow::ensure!(
            !id.is_empty() && id.len() <= 256 && !id.contains('\0') && groups.len() <= 4096,
            "Invalid observed group mention context."
        );
        for group in groups {
            anyhow::ensure!(
                profile_scope(Some(group))?.ends_with("@g.us"),
                "Invalid mentioned group."
            );
        }
        let mut conn = self.conn.lock().unwrap();
        if !member_context_eligible(&conn, &chat, id)? {
            return Ok(());
        }
        let tx = conn.savepoint()?;
        for group in groups {
            tx.execute(
                "INSERT OR IGNORE INTO member_mention_groups(chat,id,group_jid) VALUES(?1,?2,?3)",
                params![chat, id, group],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_member_signal(
        &self,
        jid: &str,
        chat: Option<&str>,
        online: Option<bool>,
        last_seen: Option<i64>,
        typing: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let jid = member_address(jid)?;
        let (kind, scope) = if typing.is_some() {
            ("typing", profile_scope(chat)?)
        } else {
            ("presence", String::new())
        };
        anyhow::ensure!(
            typing.is_none_or(|value| matches!(value, "typing" | "recording" | "paused")),
            "Unsupported typing state."
        );
        self.conn.lock().unwrap().execute("INSERT INTO member_profile_signals(kind,chat,jid,online,last_seen,state,observed_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
            ON CONFLICT(kind,chat,jid) DO UPDATE SET online=excluded.online,last_seen=excluded.last_seen,state=excluded.state,observed_at=excluded.observed_at
            WHERE excluded.observed_at>=member_profile_signals.observed_at", params![kind,scope,jid,online,last_seen,typing,at])?;
        Ok(())
    }

    pub(crate) fn member_live_cache(&self, jid: &str) -> Result<Option<(String, i64)>> {
        let forms = serde_json::to_string(&super::names::name_forms(self, &member_address(jid)?)?)?;
        self.conn.lock().unwrap().query_row("SELECT payload,observed_at FROM member_live_profiles WHERE jid IN (SELECT value FROM json_each(?1)) ORDER BY observed_at DESC LIMIT 1",
            [forms], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(Into::into)
    }

    pub(crate) fn cache_member_live(&self, jid: &str, payload: &str, at: i64) -> Result<()> {
        let jid = member_address(jid)?;
        anyhow::ensure!(
            payload.len() <= 65536,
            "Member live profile cache is too large."
        );
        self.conn.lock().unwrap().execute("INSERT INTO member_live_profiles(jid,payload,observed_at) VALUES(?1,?2,?3)
            ON CONFLICT(jid) DO UPDATE SET payload=excluded.payload,observed_at=excluded.observed_at WHERE excluded.observed_at>=member_live_profiles.observed_at", params![jid,payload,at])?;
        Ok(())
    }
}

fn cached_group(conn: &Connection, chat: &str, forms: &str) -> Result<Option<CachedMemberGroup>> {
    conn.query_row("SELECT p.subject,p.observed_at,p.own_admin,r.admin,r.owner,r.label,r.jid,r.observed_at,p.complete FROM member_group_profiles p
        LEFT JOIN member_group_rosters r ON r.chat=p.chat AND r.jid IN (SELECT value FROM json_each(?2))
        WHERE p.chat=?1 ORDER BY r.observed_at DESC LIMIT 1", params![chat,forms], |row| {
            Ok(CachedMemberGroup { chat: chat.into(),subject: row.get(0)?,observed_at:row.get(1)?,own_admin:row.get(2)?,
                present: if row.get::<_,Option<String>>(6)?.is_some() { Some(true) } else if row.get::<_,bool>(8)? { Some(false) } else { None },
                admin:row.get(3)?,owner:row.get(4)?,label:row.get(5)?,member_observed_at:row.get(7)?,complete_snapshot:row.get(8)? })
        }).optional().map_err(Into::into)
}

impl StoreWorker {
    pub(crate) async fn record_member_group_mentions(
        &self,
        chat: &str,
        id: &str,
        groups: Vec<String>,
    ) -> Result<()> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |store| store.record_member_group_mentions(&chat, &id, &groups))
            .await
    }
    pub(crate) async fn member_profile_local(
        &self,
        jid: &str,
        chat: Option<&str>,
        own: &str,
        now: i64,
    ) -> Result<MemberProfileLocal> {
        let (jid, chat, own) = (jid.to_owned(), chat.map(str::to_owned), own.to_owned());
        self.run(move |store| store.member_profile_local(&jid, chat.as_deref(), &own, now))
            .await
    }
    pub(crate) async fn set_member_note(
        &self,
        jid: &str,
        chat: Option<&str>,
        note: &str,
        warnings: u32,
        at: i64,
    ) -> Result<MemberNote> {
        let (jid, chat, note) = (jid.to_owned(), chat.map(str::to_owned), note.to_owned());
        self.run(move |store| store.set_member_note(&jid, chat.as_deref(), &note, warnings, at))
            .await
    }
    pub(crate) async fn record_group_profile_snapshot(
        &self,
        chat: &str,
        subject: Option<&str>,
        roster: Vec<MemberRosterEntry>,
        own_admin: bool,
        at: i64,
    ) -> Result<()> {
        let (chat, subject) = (chat.to_owned(), subject.map(str::to_owned));
        self.run(move |store| {
            store.record_group_profile_snapshot(&chat, subject.as_deref(), &roster, own_admin, at)
        })
        .await
    }
    pub(crate) async fn record_member_change(
        &self,
        chat: &str,
        jid: &str,
        action: &str,
        label: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let (chat, jid, action, label) = (
            chat.to_owned(),
            jid.to_owned(),
            action.to_owned(),
            label.map(str::to_owned),
        );
        self.run(move |store| {
            store.record_member_change(&chat, &jid, &action, label.as_deref(), at)
        })
        .await
    }
    pub(crate) async fn record_member_mentions(
        &self,
        chat: &str,
        id: &str,
        targets: Vec<String>,
    ) -> Result<()> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |store| store.record_member_mentions(&chat, &id, &targets))
            .await
    }
    pub(crate) async fn record_member_signal(
        &self,
        jid: &str,
        chat: Option<&str>,
        online: Option<bool>,
        last_seen: Option<i64>,
        typing: Option<&str>,
        at: i64,
    ) -> Result<()> {
        let (jid, chat, typing) = (
            jid.to_owned(),
            chat.map(str::to_owned),
            typing.map(str::to_owned),
        );
        self.run(move |store| {
            store.record_member_signal(
                &jid,
                chat.as_deref(),
                online,
                last_seen,
                typing.as_deref(),
                at,
            )
        })
        .await
    }
    pub(crate) async fn member_live_cache(&self, jid: &str) -> Result<Option<(String, i64)>> {
        let jid = jid.to_owned();
        self.run(move |store| store.member_live_cache(&jid)).await
    }
    pub(crate) async fn cache_member_live(&self, jid: &str, payload: &str, at: i64) -> Result<()> {
        let (jid, payload) = (jid.to_owned(), payload.to_owned());
        self.run(move |store| store.cache_member_live(&jid, &payload, at))
            .await
    }
}

fn latest_join(conn: &Connection, chat: &str, forms: &str) -> Result<Option<MemberJoinEvidence>> {
    conn.query_row("SELECT id,timestamp,sender,system_kind FROM messages m WHERE chat=?1
        AND system_kind IN ('GROUP_PARTICIPANT_ADD','GROUP_PARTICIPANT_INVITE','GROUP_PARTICIPANT_ACCEPT','GROUP_PARTICIPANT_ADD_REQUEST_JOIN','GROUP_PARTICIPANT_LINKED_GROUP_JOIN','GROUP_PARTICIPANT_JOINED_GROUP_AND_PARENT_GROUP')
        AND spoiler=0 AND revoked=0 AND deleted=0 AND media_once_kind IS NULL
        AND COALESCE(media_kind,'') NOT IN ('view_once','unknown')
        AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id)
        AND json_valid(system_params) AND EXISTS(SELECT 1 FROM json_each(m.system_params) WHERE value IN (SELECT value FROM json_each(?2)))
        ORDER BY timestamp DESC,sort_order DESC LIMIT 1", params![chat,forms], |row| {
            let actor:String=row.get(2)?;
            Ok(MemberJoinEvidence { message_id:row.get(0)?,timestamp:row.get(1)?,actor:member_address(&actor).ok(),kind:row.get(3)? })
        }).optional().map_err(Into::into)
}

fn member_signals(conn: &Connection, forms: &str, scope: &str, now: i64) -> Result<MemberSignals> {
    let presence:Option<(Option<bool>,Option<i64>,i64)>=conn.query_row("SELECT online,last_seen,observed_at FROM member_profile_signals WHERE kind='presence' AND jid IN (SELECT value FROM json_each(?1)) ORDER BY observed_at DESC LIMIT 1", [forms], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    let typing:Option<(Option<String>,i64)>=conn.query_row("SELECT state,observed_at FROM member_profile_signals WHERE kind='typing' AND chat=?1 AND jid IN (SELECT value FROM json_each(?2)) ORDER BY observed_at DESC LIMIT 1", params![scope,forms], |row|Ok((row.get(0)?,row.get(1)?))).optional()?;
    let mut signals = MemberSignals::default();
    if let Some((online, seen, at)) = presence {
        signals.presence_at = Some(at);
        if now >= at && now - at <= 30 {
            signals.online = online;
            signals.last_seen = seen;
        }
    }
    if let Some((state, at)) = typing {
        signals.typing_at = Some(at);
        if now >= at && now - at <= 15 {
            signals.typing = state.filter(|value| value != "paused");
        }
    }
    Ok(signals)
}

#[cfg(test)]
#[path = "member_profiles_tests.rs"]
mod tests;
