use crate::media::Media;
use serde::{Deserialize, Serialize};
use std::fmt;
use worker::{Result, SqlStorage, SqlStorageValue};
/// ```text
/// messages
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | id | role      | content          | event_id | reply_to | attempts | created_at |
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | 1  | user      | "what was that…" | evt_1    | NULL     | 0        | 1761600000 |
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | 2  | assistant | "Sunset Lofts…"  | NULL     | evt_1    | 0        | 1761600002 |
/// +----+-----------+------------------+----------+----------+----------+------------+
///
/// saved
/// +----+------------+-------+------------------------+------------------+-------------+--------------------------+----------+------------+
/// | id | message_id | media | content                | title            | description | data                     | attempts | created_at |
/// +----+------------+-------+------------------------+------------------+-------------+--------------------------+----------+------------+
/// | 1  | 1          | link  | "https://amazon.com/…" | "fairlife Choc…" | "Grocery…"  | {"image_url":"https://…"} | 0        | 1761600000 |
/// +----+------------+-------+------------------------+------------------+-------------+--------------------------+----------+------------+
/// ```
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content    TEXT NOT NULL,
    event_id   TEXT UNIQUE,
    reply_to   TEXT UNIQUE REFERENCES messages(event_id),
    attempts   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((role = 'user') = (event_id IS NOT NULL)),
    CHECK ((role = 'assistant') = (reply_to IS NOT NULL))
) STRICT;

CREATE TABLE IF NOT EXISTS saved (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages(id),
    media       TEXT NOT NULL CHECK (media IN ('link', 'note')),
    content     TEXT NOT NULL,
    title       TEXT,
    description TEXT,
    data        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data)),
    attempts    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (message_id, content)
) STRICT;
"#;

pub struct Db {
    sql: SqlStorage,
}

#[derive(Deserialize, Serialize)]
pub struct Row {
    pub role: String,
    pub content: String,
}

/// One `saved` row, as the agent sees it.
#[derive(Deserialize)]
pub struct Saved {
    pub media: String,
    pub content: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// `YYYY-MM-DD`, in UTC.
    pub saved_on: String,
}

/// Reads as `2026-11-08 link: https://… | title | description`, one line of the agent's prompt.
impl fmt::Display for Saved {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}: {}", self.saved_on, self.media, self.content)?;
        for extra in [&self.title, &self.description].into_iter().flatten() {
            write!(f, " | {extra}")?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Pending {
    pub id: i64,
    pub event_id: String,
    pub content: String,
    /// Failed model calls so far.
    pub attempts: i64,
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        Ok(Self { sql })
    }

    /// Inserts the user's message; returns `false` if `event_id` was already saved.
    pub fn save_user_message(&self, event_id: &str, content: &str) -> Result<bool> {
        let insert_query = "INSERT OR IGNORE INTO messages (role, event_id, content) \
            VALUES ('user', ?, ?)";
        let cursor = self.sql.exec(
            insert_query,
            Some(vec![
                SqlStorageValue::String(event_id.to_string()),
                SqlStorageValue::String(content.to_string()),
            ]),
        )?;
        Ok(cursor.rows_written() > 0)
    }

    /// Inserts the assistant's reply, linked to the user message it answers via `reply_to`.
    pub fn save_agent_message(&self, reply_to: &str, content: &str) -> Result<()> {
        let insert_query =
            "INSERT INTO messages (role, reply_to, content) VALUES ('assistant', ?, ?)";
        self.sql.exec(
            insert_query,
            Some(vec![
                SqlStorageValue::String(reply_to.to_string()),
                SqlStorageValue::String(content.to_string()),
            ]),
        )?;
        Ok(())
    }

    /// Returns the last `limit` messages up to and including row `up_to`, oldest first.
    pub fn load_history(&self, up_to: i64, limit: usize) -> Result<Vec<Row>> {
        let select_query = "SELECT m.role, m.content FROM messages AS m \
            LEFT JOIN messages AS u ON u.event_id = m.reply_to \
            WHERE COALESCE(u.id, m.id) <= ? \
            ORDER BY COALESCE(u.id, m.id) DESC, m.id DESC LIMIT ?";
        let mut rows: Vec<Row> = self
            .sql
            .exec(
                select_query,
                Some(vec![
                    SqlStorageValue::Integer(up_to),
                    SqlStorageValue::Integer(limit as i64),
                ]),
            )?
            .to_array()?;
        rows.reverse();
        Ok(rows)
    }

    /// Counts one failed model call for the user message `event_id`.
    pub fn record_failure(&self, event_id: &str) -> Result<()> {
        let update_query = "UPDATE messages SET attempts = attempts + 1 WHERE event_id = ?";
        self.sql.exec(update_query, Some(vec![event_id.into()]))?;
        Ok(())
    }

    /// Returns the oldest user message that has no reply yet.
    pub fn next_unanswered(&self) -> Result<Option<Pending>> {
        let select_query =
            "SELECT id, event_id, content, attempts FROM messages AS m WHERE role = 'user' \
            AND NOT EXISTS (SELECT 1 FROM messages WHERE reply_to = m.event_id) \
            ORDER BY id LIMIT 1";
        let mut rows: Vec<Pending> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    /// Inserts one saved row for `item`, skipping duplicates for this message.
    pub fn insert_saved(&self, message_id: i64, item: &dyn Media) -> Result<bool> {
        let data = serde_json::to_string(&item.data())?;
        let insert_query =
            "INSERT OR IGNORE INTO saved (message_id, media, content, title, description, data) \
            VALUES (?, ?, ?, ?, ?, ?)";
        let cursor = self.sql.exec(
            insert_query,
            Some(vec![
                SqlStorageValue::Integer(message_id),
                SqlStorageValue::String(item.get_type().to_string()),
                SqlStorageValue::String(item.content()),
                opt_sql(item.title()),
                opt_sql(item.description()),
                SqlStorageValue::String(data),
            ]),
        )?;
        Ok(cursor.rows_written() > 0)
    }

    /// Returns the newest `limit` saved rows, newest first.
    pub fn load_saved(&self, limit: usize) -> Result<Vec<Saved>> {
        let select_query = "SELECT media, content, title, description, \
            date(created_at, 'unixepoch') AS saved_on \
            FROM saved ORDER BY id DESC LIMIT ?";
        self.sql
            .exec(
                select_query,
                Some(vec![SqlStorageValue::Integer(limit as i64)]),
            )?
            .to_array()
    }
}

fn opt_sql(value: Option<&str>) -> SqlStorageValue {
    value.map_or(SqlStorageValue::Null, |s| {
        SqlStorageValue::String(s.to_string())
    })
}
