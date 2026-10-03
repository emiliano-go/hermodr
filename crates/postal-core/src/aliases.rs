//! Local, per-account contact aliases.
//!
//! An alias is a name the user invents for a contact so they can be addressed
//! as `@alias` in the composer. It is strictly an addressing aid: it is never
//! shown as somebody's name, never written to the phone, and never mixed into
//! the learned-names table, so it can never override a display name.
//!
//! It lives in its own file rather than beside the messages because the message
//! store moves to `:memory:` when history is turned off, and an alias must
//! outlive that setting. A fresh file also needs nothing from the versioned
//! migration work: the schema is one idempotent `CREATE TABLE IF NOT EXISTS`.

use std::{path::Path, sync::Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Longest alias accepted, so the mention token stays readable in the composer.
const MAX_ALIAS_LEN: usize = 32;

/// Tokens the composer gives its own meaning to, so they cannot be an alias.
const RESERVED: [&str; 2] = ["all", "all-override"];

/// The account's contact aliases, in their own store.
pub struct AliasStore {
    conn: Mutex<Connection>,
}

impl AliasStore {
    /// Opens (or creates) the alias store at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_key(path, None)
    }

    pub fn open_with_key(path: &Path, key: Option<&crate::database_crypto::DatabaseKey>) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let conn = crate::database_crypto::open_database(path, key, rusqlite::OpenFlags::default())
            .with_context(|| format!("opening alias store at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // `key` is the lower-cased alias, so `A` and `a` cannot both be
        // assigned to a contact. Uniqueness is what makes `@alias` resolve to
        // exactly one person, so it is enforced here rather than in the UI.
        // The primary key is `(jid, key)` so one contact can hold many aliases.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS contact_aliases (
                 jid   TEXT NOT NULL,
                 alias TEXT NOT NULL,
                 key   TEXT NOT NULL,
                 PRIMARY KEY (jid, key)
             );
             CREATE UNIQUE INDEX IF NOT EXISTS contact_aliases_key
                 ON contact_aliases (key);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Every alias, as `(jid, alias)`, in the order they were added.
    pub fn all(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid, alias FROM contact_aliases ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Gives `alias` to the first of `jids`, which are all the same contact.
    ///
    /// An alias the contact already holds under one of its other address forms
    /// is the same alias rather than a conflict, so it is accepted silently.
    /// An alias another contact holds is rejected, which is what keeps
    /// `@alias` pointing at exactly one person.
    pub fn add(&self, jids: &[String], alias: &str) -> Result<()> {
        let alias = validate(alias)?;
        let Some(target) = jids.first() else {
            anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_contact"));
        };
        let key = key_of(alias);
        let conn = self.conn.lock().unwrap();
        // `optional` so a genuine read failure surfaces as one, rather than
        // being taken for "nobody holds it" and failing the insert instead.
        let owner: Option<String> = conn
            .query_row(
                "SELECT jid FROM contact_aliases WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(owner) = owner {
            if jids.contains(&owner) {
                return Ok(());
            }
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_conflict"));
        }
        conn.execute(
            "INSERT INTO contact_aliases (jid, alias, key) VALUES (?1, ?2, ?3)",
            params![target, alias, key],
        )?;
        Ok(())
    }

    /// Drops one alias from one address form; a form that never held it is fine.
    pub fn remove(&self, jid: &str, alias: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM contact_aliases WHERE jid = ?1 AND key = ?2",
            params![jid, key_of(alias)],
        )?;
        Ok(())
    }
}

/// The lower-cased alias the unique index is keyed by, matching what the
/// composer does when it looks one up.
fn key_of(alias: &str) -> String {
    alias.trim().to_lowercase()
}

/// Rejects what the composer could not read back as a mention token.
///
/// The composer recognises a mention as `@` plus a run of non-spaces, so a
/// token with a space in it would stop resolving the moment it was typed. An
/// all-digit alias is refused for a different reason: the wire already writes
/// mentions as `@<number>`, so digits would be ambiguous with one.
fn validate(alias: &str) -> Result<&str> {
    let alias = alias.trim();
    if alias.is_empty() {
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_empty"));
    }
    if alias.chars().count() > MAX_ALIAS_LEN {
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_length").with_param("limit", serde_json::Number::from(MAX_ALIAS_LEN)));
    }
    if alias.chars().any(char::is_whitespace) {
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_spaces"));
    }
    if alias.chars().all(|c| c.is_ascii_digit()) {
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_digits"));
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(alias)) {
        anyhow::bail!(crate::message_ref::MessageRef::new("error.alias_reserved").with_param("alias", alias));
    }
    Ok(alias)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> AliasStore {
        // The real schema is used, so a change to it never breaks the tests.
        AliasStore::open(Path::new(":memory:")).unwrap()
    }

    fn jids(jid: &str) -> Vec<String> {
        vec![jid.to_string()]
    }

    fn aliases_of(s: &AliasStore, jid: &str) -> Vec<String> {
        let mut out: Vec<String> = s
            .all()
            .unwrap()
            .into_iter()
            .filter(|(j, _)| j == jid)
            .map(|(_, a)| a)
            .collect();
        out.sort();
        out
    }

    fn no_aliases() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn a_contact_can_hold_several_aliases() {
        let s = store();
        s.add(&jids("a@s"), "boss").unwrap();
        s.add(&jids("a@s"), "dottik").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["boss", "dottik"]);
    }

    #[test]
    fn each_alias_can_be_removed() {
        let s = store();
        s.add(&jids("a@s"), "boss").unwrap();
        s.add(&jids("a@s"), "dottik").unwrap();
        s.remove("a@s", "boss").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["dottik"]);
    }

    #[test]
    fn removing_an_alias_only_touches_its_own_contact() {
        let s = store();
        s.add(&jids("a@s"), "boss").unwrap();
        s.add(&jids("b@s"), "chief").unwrap();
        s.remove("b@s", "chief").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["boss"]);
    }

    #[test]
    fn aliases_are_unique_case_insensitively() {
        let s = store();
        s.add(&jids("a@s"), "Boss").unwrap();
        let refused = s.add(&jids("b@s"), "boss").unwrap_err();
        assert_eq!(
            refused.downcast_ref::<crate::message_ref::MessageRef>().unwrap().code,
            "error.alias_conflict"
        );
        assert_eq!(aliases_of(&s, "b@s"), no_aliases());
    }

    #[test]
    fn readding_the_same_alias_to_one_contact_is_a_noop() {
        let s = store();
        s.add(&jids("a@s"), "Boss").unwrap();
        s.add(&jids("a@s"), "boss").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["Boss"]);
    }

    #[test]
    fn the_same_contact_under_two_address_forms_is_not_a_conflict() {
        let s = store();
        s.add(&jids("1@lid"), "boss").unwrap();
        s.add(
            &["598@s.whatsapp.net".to_string(), "1@lid".to_string()],
            "boss",
        )
        .unwrap();
        assert_eq!(
            s.all().unwrap(),
            [("1@lid".to_string(), "boss".to_string())]
        );
    }

    #[test]
    fn aliases_reject_what_a_mention_token_cannot_carry() {
        let s = store();
        for (alias, code) in [
            ("", "error.alias_empty"),
            ("dottik j", "error.alias_spaces"),
            ("  ", "error.alias_empty"),
            ("59891954564", "error.alias_digits"),
            ("all", "error.alias_reserved"),
            ("All-Override", "error.alias_reserved"),
            (&"x".repeat(MAX_ALIAS_LEN + 1), "error.alias_length"),
        ] {
            let refused = s.add(&jids("a@s"), alias).unwrap_err();
            let reference = refused.downcast_ref::<crate::message_ref::MessageRef>().unwrap();
            assert_eq!(reference.code, code, "{alias:?}");
            if code == "error.alias_length" {
                assert_eq!(serde_json::to_value(reference).unwrap()["params"]["limit"], MAX_ALIAS_LEN);
            }
        }
        assert_eq!(aliases_of(&s, "a@s"), no_aliases());
    }

    #[test]
    fn an_alias_of_maximum_length_is_accepted() {
        let s = store();
        s.add(&jids("a@s"), &"x".repeat(MAX_ALIAS_LEN)).unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["x".repeat(MAX_ALIAS_LEN)]);
    }

    #[test]
    fn surrounding_space_is_trimmed_rather_than_refused() {
        let s = store();
        s.add(&jids("a@s"), "  dottik  ").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), ["dottik"]);
        s.remove("a@s", "DotTik").unwrap();
        assert_eq!(aliases_of(&s, "a@s"), no_aliases());
    }

    #[test]
    fn non_latin_aliases_fold_by_their_own_case_rules() {
        let s = store();
        s.add(&jids("a@s"), "Dóttik").unwrap();
        // A byte-wise fold would treat this as a different alias, leaving two
        // people able to answer to what reads as the same name.
        let refused = s.add(&jids("b@s"), "DÓTTIK").unwrap_err();
        assert_eq!(refused.downcast_ref::<crate::message_ref::MessageRef>().unwrap().code, "error.alias_conflict");
        assert_eq!(aliases_of(&s, "b@s"), no_aliases());
    }
}

impl crate::store::AliasWorker {
    pub(crate) async fn all(&self) -> Result<Vec<(String, String)>> {
        self.run(move |store| store.all()).await
    }

    pub(crate) async fn add(&self, jids: &[String], alias: &str) -> Result<()> {
        let jids = jids.to_vec();
        let alias = alias.to_owned();
        self.run(move |store| store.add(&jids, &alias)).await
    }

    pub(crate) async fn remove(&self, jid: &str, alias: &str) -> Result<()> {
        let jid = jid.to_owned();
        let alias = alias.to_owned();
        self.run(move |store| store.remove(&jid, &alias)).await
    }
}
