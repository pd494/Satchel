use serde::{Deserialize, Serialize};
use worker::{Result, SqlStorage, SqlStorageValue};
/// ```text
/// +----+-----------+------------------+----------+----------+------------+
/// | id | role      | content          | event_id | reply_to | created_at |
/// +----+-----------+------------------+----------+----------+------------+
/// | 1  | user      | "what was that…" | evt_1    | NULL     | 1761600000 |
/// +----+-----------+------------------+----------+----------+------------+
/// | 2  | assistant | "Sunset Lofts…"  | NULL     | evt_1    | 1761600002 |
/// +----+-----------+------------------+----------+----------+------------+
/// ```
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content    TEXT NOT NULL,
    event_id   TEXT UNIQUE,
    reply_to   TEXT UNIQUE REFERENCES messages(event_id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((role = 'user') = (event_id IS NOT NULL)),
    CHECK ((role = 'assistant') = (reply_to IS NOT NULL))
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

#[derive(Deserialize)]
pub struct Pending {
    pub id: i64,
    pub event_id: String,
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        Ok(Self { sql })
    }

    /// Inserts the user's message; returns `false` if `event_id` was already saved.
    pub fn save_user_message(&self, event_id: &str, content: &str) -> Result<bool> {
        let insert_query =
            "INSERT OR IGNORE INTO messages (role, event_id, content) VALUES ('user', ?, ?)";
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

    /// Returns the oldest user message that has no reply yet.
    pub fn next_unanswered(&self) -> Result<Option<Pending>> {
        let select_query = "SELECT id, event_id FROM messages AS m WHERE role = 'user' \
            AND NOT EXISTS (SELECT 1 FROM messages WHERE reply_to = m.event_id) \
            ORDER BY id LIMIT 1";
        let mut rows: Vec<Pending> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }
}
