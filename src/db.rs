use crate::media::Media;
use serde::Deserialize;
use worker::{Result, SqlStorage, SqlStorageValue};
/// ```text
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | id | role      | content          | event_id | reply_to | attempts | created_at |
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | 1  | user      | "what was that…" | evt_1    | NULL     | 0        | 1761600000 |
/// +----+-----------+------------------+----------+----------+----------+------------+
/// | 2  | assistant | "Sunset Lofts…"  | NULL     | evt_1    | 0        | 1761600002 |
/// +----+-----------+------------------+----------+----------+----------+------------+
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
    media       TEXT NOT NULL CHECK (media IN ('link', 'note', 'image', 'video')),
    content     TEXT NOT NULL,
    title       TEXT,
    description TEXT,
    data        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data)),
    attempts    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (message_id, content)
) STRICT;
"#;

/// Finds a `saved` table made before images and videos. SQLite can't change a CHECK in place.
const OLD_SAVED: &str =
    "SELECT 1 FROM sqlite_master WHERE name = 'saved' AND sql NOT LIKE '%''video''%'";

pub struct Db {
    sql: SqlStorage,
}

#[derive(Deserialize)]
pub struct Row {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct Pending {
    pub id: i64,
    pub event_id: String,
    pub content: String,
    /// Failed model calls or sends so far.
    pub attempts: i64,
    /// Items saved from this message before the reply, such as photos.
    pub saved: i64,
}

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        if sql.exec(OLD_SAVED, None)?.raw().next().is_some() {
            sql.exec("ALTER TABLE saved RENAME TO saved_old", None)?;
            sql.exec(SCHEMA, None)?;
            sql.exec(
                "INSERT INTO saved SELECT * FROM saved_old; DROP TABLE saved_old;",
                None,
            )?;
        }
        Ok(Self { sql })
    }

    /// Inserts the user's message. Returns its row id, or `None` if `event_id` was already saved.
    pub fn save_user_message(&self, event_id: &str, content: &str) -> Result<Option<i64>> {
        let insert_query = "INSERT OR IGNORE INTO messages (role, event_id, content) \
            VALUES ('user', ?, ?) RETURNING id";
        let mut rows: Vec<Id> = self
            .sql
            .exec(
                insert_query,
                Some(vec![
                    SqlStorageValue::String(event_id.to_string()),
                    SqlStorageValue::String(content.to_string()),
                ]),
            )?
            .to_array()?;
        Ok(rows.pop().map(|row| row.id))
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
    /// Each saved image or video follows its message as `[#id kind filename]`.
    pub fn load_history(&self, up_to: i64, limit: usize) -> Result<Vec<Row>> {
        let select_query = "SELECT m.role, trim(m.content || coalesce((\
                SELECT group_concat(' [#' || s.id || ' ' || s.media || coalesce(' ' || s.title, '') || ']', '') \
                FROM saved AS s WHERE s.message_id = m.id AND s.media IN ('image', 'video')), '')) AS content \
            FROM messages AS m \
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

    /// Counts one failed model call or send for the user message `event_id`.
    pub fn record_failure(&self, event_id: &str) -> Result<()> {
        let update_query = "UPDATE messages SET attempts = attempts + 1 WHERE event_id = ?";
        self.sql.exec(update_query, Some(vec![event_id.into()]))?;
        Ok(())
    }

    /// Returns the oldest user message that has no reply yet.
    pub fn next_unanswered(&self) -> Result<Option<Pending>> {
        let select_query = "SELECT id, event_id, content, attempts, \
            (SELECT count(*) FROM saved WHERE message_id = m.id) AS saved \
            FROM messages AS m WHERE role = 'user' \
            AND NOT EXISTS (SELECT 1 FROM messages WHERE reply_to = m.event_id) \
            ORDER BY id LIMIT 1";
        let mut rows: Vec<Pending> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    pub fn insert_saved(&self, message_id: i64, item: &dyn Media) -> Result<()> {
        let data = serde_json::to_string(&item.data())?;
        let insert_query =
            "INSERT OR IGNORE INTO saved (message_id, media, content, title, description, data) \
            VALUES (?, ?, ?, ?, ?, ?)";
        self.sql.exec(
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
        Ok(())
    }

    /// Returns the Linq attachment id of saved image or video `id`.
    pub fn attachment_id(&self, id: i64) -> Result<Option<String>> {
        let select_query = "SELECT content FROM saved WHERE id = ? AND media IN ('image', 'video')";
        let mut rows: Vec<Content> = self
            .sql
            .exec(select_query, Some(vec![SqlStorageValue::Integer(id)]))?
            .to_array()?;
        Ok(rows.pop().map(|row| row.content))
    }
}

fn opt_sql(value: Option<&str>) -> SqlStorageValue {
    value.map_or(SqlStorageValue::Null, |s| {
        SqlStorageValue::String(s.to_string())
    })
}
