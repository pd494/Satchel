use crate::media::Media;
use serde::Deserialize;
use worker::{Result, SqlStorage, SqlStorageValue};
/// ```text
/// +----+-----------+------------------+----------+----------+----------+------+------------+
/// | id | role      | content          | event_id | reply_to | attempts | sent | created_at |
/// +----+-----------+------------------+----------+----------+----------+------+------------+
/// | 1  | user      | "what was that…" | evt_1    | NULL     | 1        | 0    | 1761600000 |
/// +----+-----------+------------------+----------+----------+----------+------+------------+
/// | 2  | assistant | "Sunset Lofts…"  | NULL     | evt_1    | 0        | 1    | 1761600002 |
/// +----+-----------+------------------+----------+----------+----------+------+------------+
/// ```
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content    TEXT NOT NULL,
    event_id   TEXT UNIQUE,
    reply_to   TEXT UNIQUE REFERENCES messages(event_id),
    attempts   INTEGER NOT NULL DEFAULT 0,
    sent       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((role = 'user') = (event_id IS NOT NULL)),
    CHECK ((role = 'assistant') = (reply_to IS NOT NULL))
) STRICT;

CREATE TABLE IF NOT EXISTS saved (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages(id),
    media       TEXT NOT NULL CHECK (media IN ('link', 'note', 'image', 'video', 'voice', 'contact', 'location', 'file')),
    content     TEXT NOT NULL,
    title       TEXT,
    description TEXT,
    data        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data)),
    attempts    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    due_at      INTEGER,
    repeat      TEXT,
    until_at    INTEGER,
    UNIQUE (message_id, content)
) STRICT;

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
"#;

pub struct Db {
    sql: SqlStorage,
}

#[derive(Deserialize)]
pub struct Row {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct UnansweredMessage {
    pub id: i64,
    pub event_id: String,
    pub content: String,
    /// Runs started so far.
    pub attempts: i64,
    /// The stored reply that failed to send, if any.
    pub response: Option<String>,
}

#[derive(Deserialize)]
struct Setting {
    value: String,
}

#[derive(Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub content: String,
    pub due_at: i64,
    pub repeat: Option<String>,
    pub until_at: Option<i64>,
    pub urls: Option<String>,
}

const SELECT_REMINDERS: &str = "SELECT r.id, r.content, r.due_at, r.repeat, r.until_at, \
    (SELECT group_concat(u.content, char(10)) FROM saved AS u \
    WHERE u.message_id = r.message_id AND u.media != 'note') AS urls FROM saved AS r";

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        Ok(Self { sql })
    }

    /// Returns the earliest reminder time, or `None` if no reminder is pending.
    pub fn next_due_at(&self) -> Result<Option<i64>> {
        let select_query =
            format!("{SELECT_REMINDERS} WHERE r.due_at IS NOT NULL ORDER BY r.due_at LIMIT 1");
        let mut rows: Vec<Reminder> = self.sql.exec(&select_query, None)?.to_array()?;
        Ok(rows.pop().map(|row| row.due_at))
    }

    /// Returns every reminder whose time has come, oldest first.
    pub fn due_reminders(&self) -> Result<Vec<Reminder>> {
        let select_query =
            format!("{SELECT_REMINDERS} WHERE r.due_at <= unixepoch() ORDER BY r.due_at");
        self.sql.exec(&select_query, None)?.to_array()
    }

    /// Returns the newest pending reminder whose note is `note`, ignoring case.
    pub fn pending_reminder(&self, note: &str) -> Result<Option<Reminder>> {
        let select_query = format!(
            "{SELECT_REMINDERS} WHERE r.due_at IS NOT NULL AND lower(r.content) = lower(?) \
            ORDER BY r.id DESC LIMIT 1"
        );
        let mut rows: Vec<Reminder> = self
            .sql
            .exec(&select_query, Some(vec![note.into()]))?
            .to_array()?;
        Ok(rows.pop())
    }

    /// Sets when saved item `id` fires and how it repeats. A `None` time ends the reminder.
    pub fn set_reminder(
        &self,
        id: i64,
        due_at: Option<i64>,
        repeat: Option<String>,
        until_at: Option<i64>,
    ) -> Result<()> {
        let update_query = "UPDATE saved SET due_at = ?, repeat = ?, until_at = ? WHERE id = ?";
        self.sql.exec(
            update_query,
            Some(vec![
                due_at.into(),
                repeat.into(),
                until_at.into(),
                id.into(),
            ]),
        )?;
        Ok(())
    }

    /// Returns the user's IANA time zone name, if they gave one.
    pub fn time_zone(&self) -> Result<Option<String>> {
        let select_query = "SELECT value FROM settings WHERE key = 'time_zone'";
        let mut rows: Vec<Setting> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop().map(|row| row.value))
    }

    pub fn set_time_zone(&self, name: &str) -> Result<()> {
        let upsert_query = "INSERT INTO settings (key, value) VALUES ('time_zone', ?) \
            ON CONFLICT (key) DO UPDATE SET value = excluded.value";
        self.sql.exec(upsert_query, Some(vec![name.into()]))?;
        Ok(())
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
    /// The reply is unsent until `mark_sent`, and `inbox` sends it again until then.
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

    pub fn mark_sent(&self, reply_to: &str) -> Result<()> {
        let update_query = "UPDATE messages SET sent = 1 WHERE reply_to = ?";
        self.sql.exec(update_query, Some(vec![reply_to.into()]))?;
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

    /// Counts one run for the user message `event_id`, before the run starts.
    pub fn record_attempt(&self, event_id: &str) -> Result<()> {
        let update_query = "UPDATE messages SET attempts = attempts + 1 WHERE event_id = ?";
        self.sql.exec(update_query, Some(vec![event_id.into()]))?;
        Ok(())
    }

    /// Returns the oldest user message that has no reply yet, or whose reply is unsent.
    pub fn next_unanswered(&self) -> Result<Option<UnansweredMessage>> {
        let select_query = "SELECT u.id, u.event_id, u.content, u.attempts, a.content AS response \
            FROM messages AS u LEFT JOIN messages AS a ON a.reply_to = u.event_id \
            WHERE u.role = 'user' AND (a.id IS NULL OR a.sent = 0) \
            ORDER BY u.id LIMIT 1";
        let mut rows: Vec<UnansweredMessage> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    /// Saves `item` and returns its row id. Saving the same item from the same message again
    /// keeps the first row.
    pub fn insert_saved(&self, message_id: i64, item: &dyn Media) -> Result<i64> {
        let data = serde_json::to_string(&item.data())?;
        let insert_query =
            "INSERT INTO saved (message_id, media, content, title, description, data) \
            VALUES (?, ?, ?, ?, ?, ?) \
            ON CONFLICT (message_id, content) DO UPDATE SET content = content RETURNING id";
        let row: serde_json::Value = self
            .sql
            .exec(
                insert_query,
                Some(vec![
                    SqlStorageValue::Integer(message_id),
                    SqlStorageValue::String(item.get_type().to_string()),
                    SqlStorageValue::String(item.content()),
                    opt_sql(item.title()),
                    opt_sql(item.description()),
                    SqlStorageValue::String(data),
                ]),
            )?
            .one()?;
        Ok(row["id"]
            .as_i64()
            .ok_or_else(|| "insert_saved returned no id".to_string())?)
    }
}

fn opt_sql(value: Option<&str>) -> SqlStorageValue {
    value.map_or(SqlStorageValue::Null, |s| {
        SqlStorageValue::String(s.to_string())
    })
}
