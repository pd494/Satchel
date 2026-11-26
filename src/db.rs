use crate::media::Media;
use jiff::{tz::TimeZone, Span, Timestamp, ZonedDateTime};
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
/// `sent` on a user message means it was answered, by its own reply or a later one.
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
    context     TEXT,
    vector      BLOB,
    data        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data)),
    attempts    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    due_at      INTEGER,
    repeat      TEXT,
    until_at    INTEGER,
    UNIQUE (content)
) STRICT;

CREATE INDEX IF NOT EXISTS saved_due ON saved (due_at) WHERE due_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
"#;

pub struct Db {
    sql: SqlStorage,
}

/// Every pending reminder, with the links and files saved from its message.
const REMINDERS: &str = "SELECT r.id, r.content, r.due_at, r.repeat, r.until_at, \
    (SELECT group_concat(u.content, char(10)) FROM saved AS u \
    WHERE u.message_id = r.message_id AND u.media != 'note') AS urls \
    FROM saved AS r WHERE r.due_at IS NOT NULL";

/// The columns of `Saved`.
const SAVED: &str = "id, media, content, context, description, created_at, vector";

#[derive(Deserialize)]
struct NextDue {
    due_at: Option<i64>,
}

/// A row of the `saved` table, as `SAVED` selects it.
#[derive(Deserialize)]
pub struct Saved {
    pub id: i64,
    pub media: String,
    /// The URL of a link or file, or the text of a note.
    pub content: String,
    /// The user's words about the item.
    pub context: Option<String>,
    /// The search text from `set_read`.
    pub description: Option<String>,
    pub created_at: i64,
    /// Little-endian `f32` bytes from `Agent::embed`.
    #[serde(with = "serde_bytes")]
    pub vector: Option<Vec<u8>>,
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
    pub created_at: i64,
    /// The stored reply that failed to send, if any.
    pub response: Option<String>,
}

#[derive(Deserialize)]
struct Setting {
    value: String,
}

#[derive(Deserialize)]
pub struct Schedule {
    pub due_at: i64,
    pub repeat: Option<Span>,
    pub until_at: Option<i64>,
}

impl Schedule {
    /// When this schedule is due, in `zone`.
    pub fn due(&self, zone: &TimeZone) -> Result<ZonedDateTime> {
        Ok(Timestamp::from_second(self.due_at)
            .map_err(|error| error.to_string())?
            .to_zoned_datetime(zone.clone()))
    }
}

#[derive(Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub content: String,
    #[serde(flatten)]
    pub schedule: Schedule,
    pub urls: Option<String>,
}

impl Reminder {
    /// Returns the next schedule after this fire, or `None` if the reminder is done.
    pub fn next_fire(&self, zone: &TimeZone) -> Result<Option<Schedule>> {
        let Some(repeat) = self.schedule.repeat else {
            return Ok(None);
        };
        let now = Timestamp::now().as_second();
        let mut next = self.schedule.due(zone)?;

        loop {
            next = next
                .checked_add(repeat)
                .map_err(|error| error.to_string())?;
            let next_at = next.timestamp().as_second();

            if self
                .schedule
                .until_at
                .is_some_and(|until_at| next_at > until_at)
            {
                return Ok(None);
            }
            if next_at > now {
                return Ok(Some(Schedule {
                    due_at: next_at,
                    repeat: self.schedule.repeat,
                    until_at: self.schedule.until_at,
                }));
            }
        }
    }
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        Ok(Self { sql })
    }

    /// Returns every pending reminder, soonest first.
    pub fn reminders(&self) -> Result<Vec<Reminder>> {
        let select_query = format!("{REMINDERS} ORDER BY r.due_at");
        self.sql.exec(&select_query, None)?.to_array()
    }

    /// Returns the reminders due at or before `now`, soonest first.
    pub fn due_reminders(&self, now: i64) -> Result<Vec<Reminder>> {
        let select_query = format!("{REMINDERS} AND r.due_at <= ? ORDER BY r.due_at");
        self.sql
            .exec(&select_query, Some(vec![now.into()]))?
            .to_array()
    }

    /// Returns the pending reminder `id`, if there is one.
    pub fn reminder(&self, id: i64) -> Result<Option<Reminder>> {
        let select_query = format!("{REMINDERS} AND r.id = ?");
        let mut rows: Vec<Reminder> = self
            .sql
            .exec(&select_query, Some(vec![id.into()]))?
            .to_array()?;
        Ok(rows.pop())
    }

    /// Returns when the soonest pending reminder is due.
    pub fn next_due(&self) -> Result<Option<i64>> {
        let select_query = "SELECT min(due_at) AS due_at FROM saved WHERE due_at IS NOT NULL";
        Ok(self.sql.exec(select_query, None)?.one::<NextDue>()?.due_at)
    }

    /// Sets when saved item `id` fires and how it repeats. A `None` schedule ends the reminder.
    pub fn set_reminder(&self, id: i64, schedule: Option<&Schedule>) -> Result<()> {
        let update_query = "UPDATE saved SET due_at = ?, repeat = ?, until_at = ? WHERE id = ?";
        let (due_at, repeat, until_at) = schedule.map_or((None, None, None), |schedule| {
            (
                Some(schedule.due_at),
                schedule.repeat.map(|repeat| repeat.to_string()),
                schedule.until_at,
            )
        });
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

    /// Marks the user message `event_id` and its reply, if any, as sent.
    pub fn mark_sent(&self, event_id: &str) -> Result<()> {
        let update_query = "UPDATE messages SET sent = 1 WHERE event_id = ?1 OR reply_to = ?1";
        self.sql.exec(update_query, Some(vec![event_id.into()]))?;
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

    /// Returns every user message that is not answered yet, oldest first.
    pub fn unanswered(&self) -> Result<Vec<UnansweredMessage>> {
        let select_query = "SELECT u.id, u.event_id, u.content, u.attempts, u.created_at, \
            a.content AS response \
            FROM messages AS u LEFT JOIN messages AS a ON a.reply_to = u.event_id \
            WHERE u.role = 'user' AND u.sent = 0 ORDER BY u.id";
        self.sql.exec(select_query, None)?.to_array()
    }

    /// Adds the user's words to the item saved with `content`, unless a retry already
    /// added them. Returns `None` if nothing was saved with `content`.
    pub fn add_context(&self, content: &str, note: Option<&str>) -> Result<Option<Saved>> {
        let update_query = format!("UPDATE saved SET context = \
            CASE WHEN ?1 IS NULL OR instr(char(10) || context || char(10), char(10) || ?1 || char(10)) > 0 \
            THEN context \
            ELSE COALESCE(context || char(10), '') || ?1 END \
            WHERE content = ?2 RETURNING {SAVED}");
        let values = vec![note.into(), content.into()];
        Ok(self
            .sql
            .exec(&update_query, Some(values))?
            .to_array()?
            .pop())
    }

    /// Stores the search text of saved item `id`, as its description, and its embedding.
    pub fn set_read(&self, id: i64, text: &str, vector: Vec<u8>) -> Result<()> {
        let update_query = "UPDATE saved SET description = ?, vector = ? WHERE id = ?";
        let values = vec![text.into(), SqlStorageValue::Blob(vector), id.into()];
        self.sql.exec(update_query, Some(values))?;
        Ok(())
    }

    /// Saves `item` and returns its row id. Saving the same item again, from any message, keeps
    /// the first row.
    pub fn insert_saved(&self, message_id: i64, item: &dyn Media) -> Result<i64> {
        let data = serde_json::to_string(&item.data())?;
        let insert_query = format!(
            "INSERT INTO saved (message_id, media, content, title, description, data) \
            VALUES (?, ?, ?, ?, ?, ?) \
            ON CONFLICT (content) DO UPDATE SET message_id = excluded.message_id RETURNING {SAVED}"
        );
        let row: Saved = self
            .sql
            .exec(
                &insert_query,
                Some(vec![
                    message_id.into(),
                    item.get_type().into(),
                    item.content().into(),
                    item.title().into(),
                    item.description().into(),
                    data.into(),
                ]),
            )?
            .one()?;
        Ok(row.id)
    }

    /// Returns the items with search text saved at or after `after` and before `before`,
    /// newest first.
    pub fn searchable(&self, after: Option<i64>, before: Option<i64>) -> Result<Vec<Saved>> {
        let select_query = format!(
            "SELECT {SAVED} FROM saved WHERE vector IS NOT NULL AND description IS NOT NULL \
            AND (?1 IS NULL OR created_at >= ?1) AND (?2 IS NULL OR created_at < ?2) \
            ORDER BY created_at DESC, id DESC"
        );
        self.sql
            .exec(&select_query, Some(vec![after.into(), before.into()]))?
            .to_array()
    }
}
