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
    attempts   INTEGER NOT NULL DEFAULT 0,
    sent_at    INTEGER,
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

/// A user message whose reply has not been sent yet; `reply` is set once one is saved.
#[derive(Deserialize)]
pub struct Pending {
    pub id: i64,
    pub event_id: String,
    pub space_id: String,
    pub reply: Option<String>,
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

    /// Marks the reply to `reply_to` as delivered.
    pub fn mark_sent(&self, reply_to: &str) -> Result<()> {
        let update_query = "UPDATE messages SET sent_at = unixepoch() WHERE reply_to = ?";
        self.sql.exec(update_query, vec![reply_to.into()])?;
        Ok(())
    }

    /// Counts one more try at replying to message `id` and returns the new total.
    pub fn record_attempt(&self, id: i64) -> Result<i64> {
        let update_query =
            "UPDATE messages SET attempts = attempts + 1 WHERE id = ? RETURNING attempts";
        #[derive(Deserialize)]
        struct Attempts {
            attempts: i64,
        }
        let row: Attempts = self.sql.exec(update_query, vec![id.into()])?.one()?;
        Ok(row.attempts)
    }

    /// Returns the oldest user message whose reply has not been sent, if any.
    pub fn next_pending(&self) -> Result<Option<Pending>> {
        let select_query = "SELECT u.id, u.event_id, u.space_id, a.content AS reply \
            FROM messages AS u LEFT JOIN messages AS a ON a.reply_to = u.event_id \
            WHERE u.role = 'user' AND a.sent_at IS NULL ORDER BY u.id LIMIT 1";
        let mut rows: Vec<Pending> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    /// Returns up to `limit` messages through user message `through_id`, oldest first, with each
    /// reply placed right after the message it answers.
    pub fn load_history(&self, through_id: i64, limit: usize) -> Result<Vec<Row>> {
        let select_query = "SELECT m.role, m.content FROM messages AS m \
            LEFT JOIN messages AS u ON u.event_id = m.reply_to \
            WHERE COALESCE(u.id, m.id) <= ? ORDER BY COALESCE(u.id, m.id) DESC, m.id DESC LIMIT ?";
        let mut rows: Vec<Row> = self
            .sql
            .exec(select_query, vec![through_id.into(), (limit as i64).into()])?
            .to_array()?;
        rows.reverse();
        Ok(rows)
    }
}
