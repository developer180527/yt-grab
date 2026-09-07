//! All SQL lives here, so the rest of the app deals in values rather than rows.

use crate::rules::SiteRule;
use crate::settings::Settings;
use rusqlite::Connection;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct HistoryItem {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub format_id: String,
    pub audio_only: bool,
    pub output_path: Option<String>,
    pub status: String,
    pub created_at: String,
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            url         TEXT NOT NULL,
            title       TEXT NOT NULL,
            thumbnail   TEXT,
            format_id   TEXT NOT NULL,
            audio_only  INTEGER NOT NULL DEFAULT 0,
            output_path TEXT,
            status      TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS site_rules (
            domain          TEXT PRIMARY KEY,
            cookies_browser TEXT,
            cookies_file    TEXT,
            output_dir      TEXT,
            format_id       TEXT,
            audio_only      INTEGER
        );
        CREATE INDEX IF NOT EXISTS history_url ON history(url);",
    )
}

pub fn unix_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ─── Settings ─────────────────────────────────────────────────────────────────

pub fn load_settings(conn: &Connection) -> Settings {
    conn.query_row("SELECT value FROM settings WHERE key = 'settings'", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    // A blob written by an older build can be missing fields; serde(default)
    // fills them. Only a truly unparseable value falls back wholesale.
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default()
}

pub fn save_settings(conn: &Connection, s: &Settings) -> rusqlite::Result<()> {
    let json = serde_json::to_string(s).unwrap_or_default();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('settings', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![json],
    )?;
    Ok(())
}

// ─── Site rules ───────────────────────────────────────────────────────────────

fn rule_from_row(row: &rusqlite::Row) -> rusqlite::Result<SiteRule> {
    Ok(SiteRule {
        domain: row.get(0)?,
        cookies_browser: row.get(1)?,
        cookies_file: row.get(2)?,
        output_dir: row.get(3)?,
        format_id: row.get(4)?,
        audio_only: row.get::<_, Option<i32>>(5)?.map(|v| v != 0),
    })
}

const RULE_COLUMNS: &str =
    "domain, cookies_browser, cookies_file, output_dir, format_id, audio_only";

pub fn get_rule(conn: &Connection, domain: &str) -> Option<SiteRule> {
    conn.query_row(
        &format!("SELECT {RULE_COLUMNS} FROM site_rules WHERE domain = ?1"),
        rusqlite::params![domain],
        rule_from_row,
    )
    .ok()
}

pub fn list_rules(conn: &Connection) -> rusqlite::Result<Vec<SiteRule>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {RULE_COLUMNS} FROM site_rules ORDER BY domain"))?;
    let rows = stmt.query_map([], rule_from_row)?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn upsert_rule(conn: &Connection, r: &SiteRule) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO site_rules (domain, cookies_browser, cookies_file, output_dir, format_id, audio_only)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(domain) DO UPDATE SET
            cookies_browser = excluded.cookies_browser,
            cookies_file    = excluded.cookies_file,
            output_dir      = excluded.output_dir,
            format_id       = excluded.format_id,
            audio_only      = excluded.audio_only",
        rusqlite::params![
            r.domain,
            r.cookies_browser,
            r.cookies_file,
            r.output_dir,
            r.format_id,
            r.audio_only.map(|v| v as i32),
        ],
    )?;
    Ok(())
}

/// Merges one field into a rule, creating it if absent. This is how accepting a
/// remedy is remembered without clobbering anything else already set.
pub fn patch_rule(
    conn: &Connection,
    domain: &str,
    edit: impl FnOnce(&mut SiteRule),
) -> rusqlite::Result<SiteRule> {
    let mut rule = get_rule(conn, domain).unwrap_or_else(|| SiteRule {
        domain: domain.to_string(),
        ..Default::default()
    });
    edit(&mut rule);
    upsert_rule(conn, &rule)?;
    Ok(rule)
}

pub fn delete_rule(conn: &Connection, domain: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM site_rules WHERE domain = ?1", rusqlite::params![domain])?;
    Ok(())
}

// ─── History ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn insert_history(
    conn: &Connection,
    url: &str,
    title: &str,
    thumbnail: &Option<String>,
    format_id: &str,
    audio_only: bool,
    output_path: Option<&str>,
    status: &str,
) {
    let _ = conn.execute(
        "INSERT INTO history (url, title, thumbnail, format_id, audio_only, output_path, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![url, title, thumbnail, format_id, audio_only as i32, output_path, status, unix_now()],
    );
}

pub fn get_history(conn: &Connection) -> rusqlite::Result<Vec<HistoryItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, title, thumbnail, format_id, audio_only, output_path, status, created_at
         FROM history ORDER BY id DESC LIMIT 200",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(HistoryItem {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            thumbnail: row.get(3)?,
            format_id: row.get(4)?,
            audio_only: row.get::<_, i32>(5)? != 0,
            output_path: row.get(6)?,
            status: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// URLs already downloaded successfully, for marking duplicates in the UI.
pub fn completed_urls(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT url FROM history WHERE status = 'completed'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn delete_history_item(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM history WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

pub fn clear_history(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM history", [])?;
    Ok(())
}

pub fn purge_old_history(conn: &Connection, days: u32) -> rusqlite::Result<()> {
    if days == 0 {
        return Ok(());
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(days as u64 * 86_400)
        .to_string();
    conn.execute(
        "DELETE FROM history WHERE CAST(created_at AS INTEGER) < ?1",
        rusqlite::params![cutoff],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    #[test]
    fn schema_is_idempotent() {
        let c = db();
        init(&c).expect("re-running init on an existing database must be safe");
    }

    #[test]
    fn settings_round_trip() {
        let c = db();
        assert_eq!(load_settings(&c).max_concurrent, Settings::default().max_concurrent);

        let s = Settings {
            default_format: "1080p".into(),
            max_concurrent: 5,
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();

        let back = load_settings(&c);
        assert_eq!(back.default_format, "1080p");
        assert_eq!(back.max_concurrent, 5);
    }

    #[test]
    fn saving_settings_twice_updates_rather_than_duplicating() {
        let c = db();
        let mut s = Settings::default();
        save_settings(&c, &s).unwrap();
        s.default_format = "720p".into();
        save_settings(&c, &s).unwrap();
        assert_eq!(load_settings(&c).default_format, "720p");
    }

    #[test]
    fn patching_a_rule_preserves_fields_it_does_not_touch() {
        let c = db();
        patch_rule(&c, "instagram.com", |r| r.cookies_browser = Some("chrome".into())).unwrap();
        patch_rule(&c, "instagram.com", |r| r.output_dir = Some("/reels".into())).unwrap();

        let r = get_rule(&c, "instagram.com").expect("rule should exist");
        assert_eq!(r.cookies_browser.as_deref(), Some("chrome"), "earlier field survived");
        assert_eq!(r.output_dir.as_deref(), Some("/reels"));
        assert_eq!(list_rules(&c).unwrap().len(), 1, "patch must not create a second row");
    }

    #[test]
    fn rules_can_be_listed_and_deleted() {
        let c = db();
        patch_rule(&c, "b.com", |r| r.audio_only = Some(true)).unwrap();
        patch_rule(&c, "a.com", |r| r.format_id = Some("720p".into())).unwrap();

        let all = list_rules(&c).unwrap();
        assert_eq!(all.iter().map(|r| r.domain.as_str()).collect::<Vec<_>>(), vec!["a.com", "b.com"]);
        assert_eq!(all[1].audio_only, Some(true));

        delete_rule(&c, "a.com").unwrap();
        assert_eq!(list_rules(&c).unwrap().len(), 1);
    }

    #[test]
    fn a_patch_can_clear_a_field_back_to_none() {
        let c = db();
        patch_rule(&c, "x.com", |r| r.cookies_browser = Some("chrome".into())).unwrap();
        patch_rule(&c, "x.com", |r| r.cookies_browser = None).unwrap();
        assert_eq!(get_rule(&c, "x.com").unwrap().cookies_browser, None);
    }

    #[test]
    fn a_rule_round_trips_every_field_including_the_boolean() {
        let c = db();
        upsert_rule(&c, &SiteRule {
            domain: "bandcamp.com".into(),
            cookies_browser: Some("firefox".into()),
            cookies_file: Some("/c.txt".into()),
            output_dir: Some("/music".into()),
            format_id: Some("best".into()),
            audio_only: Some(false),
        }).unwrap();

        let r = get_rule(&c, "bandcamp.com").unwrap();
        assert_eq!(r.cookies_browser.as_deref(), Some("firefox"));
        assert_eq!(r.cookies_file.as_deref(), Some("/c.txt"));
        assert_eq!(r.output_dir.as_deref(), Some("/music"));
        assert_eq!(r.format_id.as_deref(), Some("best"));
        // Distinguishing Some(false) from None matters: one says "video", the
        // other says "the rule has no opinion".
        assert_eq!(r.audio_only, Some(false));
    }

    #[test]
    fn completed_urls_deduplicates_and_excludes_failures() {
        let c = db();
        insert_history(&c, "u1", "t", &None, "best", false, Some("/a"), "completed");
        insert_history(&c, "u1", "t", &None, "720p", false, Some("/b"), "completed");
        insert_history(&c, "u2", "t", &None, "best", false, None, "failed");
        assert_eq!(completed_urls(&c).unwrap(), vec!["u1"]);
    }

    #[test]
    fn history_is_returned_newest_first() {
        let c = db();
        insert_history(&c, "old", "t", &None, "best", false, None, "completed");
        insert_history(&c, "new", "t", &None, "best", false, None, "completed");
        let h = get_history(&c).unwrap();
        assert_eq!(h[0].url, "new");
    }

    #[test]
    fn missing_rule_is_none_not_an_error() {
        assert!(get_rule(&db(), "nope.com").is_none());
    }

    #[test]
    fn history_records_and_purges_by_age() {
        let c = db();
        insert_history(&c, "u1", "t1", &None, "best", false, Some("/f1"), "completed");
        insert_history(&c, "u2", "t2", &None, "best", false, None, "failed");
        assert_eq!(get_history(&c).unwrap().len(), 2);
        assert_eq!(completed_urls(&c).unwrap(), vec!["u1"]);

        // Backdate one row well past the cutoff.
        c.execute("UPDATE history SET created_at = '0' WHERE url = 'u2'", []).unwrap();
        purge_old_history(&c, 7).unwrap();
        let left = get_history(&c).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].url, "u1");
    }

    #[test]
    fn purge_with_zero_days_keeps_everything() {
        let c = db();
        insert_history(&c, "u", "t", &None, "best", false, None, "completed");
        c.execute("UPDATE history SET created_at = '0'", []).unwrap();
        purge_old_history(&c, 0).unwrap();
        assert_eq!(get_history(&c).unwrap().len(), 1, "0 means never expire");
    }
}
