use super::*;

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    for name in [
        "favorite_updated_ms",
        "recent_updated_ms",
        "recent_sent_ms",
        "recent_removed_ms",
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('stickers') WHERE name=?1)",
            [name],
            |row| row.get(0),
        )?;
        if !exists {
            conn.execute_batch(&format!(
                "ALTER TABLE stickers ADD COLUMN {name} INTEGER NOT NULL DEFAULT 0;"
            ))?;
        }
    }
    conn.execute("UPDATE stickers SET recent_sent_ms=recent_at*1000,recent_updated_ms=recent_at*1000
        WHERE recent_at IS NOT NULL AND recent_sent_ms=0 AND recent_updated_ms=0 AND recent_removed_ms=0", [])?;
    Ok(())
}
