use serde::{Deserialize, Serialize};
use worker::{Result, SqlStorage};
/// ```text
/// +----+-----------+------------------+----------+----------+------------+
/// | id | role      | content          | event_id | reply_to | created_at |
/// +----+-----------+------------------+----------+----------+------------+
/// | 1  | user      | "what was that…" | evt_1    | NULL     | 1761600000 |
/// +----+-----------+------------------+----------+----------+------------+
/// | 2  | assistant | "Sunset Lofts…"  | NULL     | evt_1    | 1761600002 |
/// +----+-----------+------------------+----------+----------+------------+
/// ```
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content    TEXT NOT NULL,
    event_id   TEXT UNIQUE,
    reply_to   TEXT UNIQUE REFERENCES messages(event_id),
    space_id   TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((role = 'user') = (event_id IS NOT NULL)),
    CHECK ((role = 'assistant') = (reply_to IS NOT NULL)),
    CHECK ((role = 'user') = (space_id IS NOT NULL))
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

/// A user message that has no reply yet.
#[derive(Deserialize)]
pub struct Unanswered {
    pub id: i64,
    pub event_id: String,
    pub space_id: String,
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        Ok(Self { sql })
    }

    /// Inserts the user's message; a retried `event_id` is ignored.
    pub fn save_user_message(&self, event_id: &str, space_id: &str, content: &str) -> Result<()> {
        let insert_query = "INSERT OR IGNORE INTO messages (role, event_id, space_id, content) VALUES ('user', ?, ?, ?)";
        self.sql.exec(
            insert_query,
            vec![event_id.into(), space_id.into(), content.into()],
        )?;
        Ok(())
    }

    /// Inserts the assistant's reply, linked to the user message it answers via `reply_to`.
    pub fn save_agent_message(&self, reply_to: &str, content: &str) -> Result<()> {
        let insert_query =
            "INSERT INTO messages (role, reply_to, content) VALUES ('assistant', ?, ?)";
        self.sql
            .exec(insert_query, vec![reply_to.into(), content.into()])?;
        Ok(())
    }

    /// Returns the oldest user message with no reply, if any.
    pub fn next_unanswered(&self) -> Result<Option<Unanswered>> {
        let select_query = "SELECT id, event_id, space_id FROM messages AS m \
            WHERE role = 'user' AND NOT EXISTS (SELECT 1 FROM messages WHERE reply_to = m.event_id) \
            ORDER BY id LIMIT 1";
        let mut rows: Vec<Unanswered> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    /// Returns up to `limit` messages ending at message `through_id`, oldest first.
    pub fn load_history(&self, through_id: i64, limit: usize) -> Result<Vec<Row>> {
        let select_query =
            "SELECT role, content FROM messages WHERE id <= ? ORDER BY id DESC LIMIT ?";
        let mut rows: Vec<Row> = self
            .sql
            .exec(select_query, vec![through_id.into(), (limit as i64).into()])?
            .to_array()?;
        rows.reverse();
        Ok(rows)
    }
}
