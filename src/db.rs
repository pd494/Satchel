use crate::media::Media;
use jiff::{tz::TimeZone, Span, Timestamp, ZonedDateTime};
use serde::{Deserialize, Serialize};
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

CREATE VIEW IF NOT EXISTS chat AS
    SELECT m.id, COALESCE(u.id, m.id) AS turn, m.role, m.content, m.created_at
    FROM messages AS m LEFT JOIN messages AS u ON u.event_id = m.reply_to;

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content, content = 'messages', content_rowid = 'id', tokenize = 'porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, content) VALUES (new.id, new.content);
END;

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
    key     TEXT PRIMARY KEY,
    value   TEXT,
    said_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS summary (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    through INTEGER NOT NULL,
    text    TEXT NOT NULL
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

const SAVED: &str = "id, media, content, context, description, created_at, vector";

const ROW: &str = "SELECT chat.id, chat.turn, chat.role, chat.content, chat.created_at FROM chat";

const CHAT_HITS: i64 = 5;
const NEIGHBORS: i64 = 3;

#[derive(Deserialize)]
struct NextDue {
    due_at: Option<i64>,
}

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

/// A user text and the reply to it share one turn: the id of the text.
#[derive(Clone, Copy, Deserialize, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct Turn(i64);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Deserialize)]
pub struct Row {
    id: i64,
    pub turn: Turn,
    pub role: Role,
    pub content: String,
    pub created_at: i64,
}

/// The summary of every turn up to and including `through`.
#[derive(Deserialize)]
pub struct Summary {
    pub through: Turn,
    pub text: String,
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
pub struct Setting {
    pub key: String,
    pub value: String,
    pub said_at: i64,
}

pub enum SettingChange {
    TimeZone(TimeZone),
    Set { key: String, value: String },
    Remove { key: String },
}

impl UnansweredMessage {
    pub fn turn(&self) -> Turn {
        Turn(self.id)
    }
}

impl SettingChange {
    /// An empty value removes the key.
    pub fn parse(key: &str, value: &str) -> std::result::Result<Self, String> {
        let key = key.trim().to_lowercase();
        let value = value.trim();
        if key.is_empty() {
            return Err("the key is empty".to_string());
        }
        if key == "time_zone" {
            if value.is_empty() {
                return Err("time_zone cannot be removed. Send the new zone.".to_string());
            }
            let zone = TimeZone::get(value).map_err(|error| error.to_string())?;
            return Ok(Self::TimeZone(zone));
        }
        if value.is_empty() {
            return Ok(Self::Remove { key });
        }
        Ok(Self::Set {
            key,
            value: value.to_string(),
        })
    }
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

    pub fn settings(&self) -> Result<Vec<Setting>> {
        let select_query =
            "SELECT key, value, said_at FROM settings WHERE value IS NOT NULL ORDER BY key";
        self.sql.exec(select_query, None)?.to_array()
    }

    /// Returns the user's IANA time zone name, if they gave one.
    pub fn time_zone(&self) -> Result<Option<String>> {
        for setting in self.settings()? {
            if setting.key == "time_zone" {
                return Ok(Some(setting.value));
            }
        }
        Ok(None)
    }

    /// A change said before the stored one changes nothing. The same value keeps its first date.
    pub fn change_setting(&self, change: &SettingChange, said_at: i64) -> Result<()> {
        let (key, value) = match change {
            SettingChange::TimeZone(zone) => {
                let name = zone.iana_name().ok_or("the time zone has no IANA name")?;
                ("time_zone", Some(name))
            }
            SettingChange::Set { key, value } => (key.as_str(), Some(value.as_str())),
            SettingChange::Remove { key } => (key.as_str(), None),
        };
        let upsert_query = "INSERT INTO settings (key, value, said_at) VALUES (?, ?, ?) \
            ON CONFLICT (key) DO UPDATE SET value = excluded.value, said_at = excluded.said_at \
            WHERE excluded.said_at >= settings.said_at AND excluded.value IS NOT settings.value";
        self.sql.exec(
            upsert_query,
            Some(vec![key.into(), value.into(), said_at.into()]),
        )?;
        Ok(())
    }

    pub fn summary(&self) -> Result<Option<Summary>> {
        let select_query = "SELECT through, text FROM summary";
        let mut rows: Vec<Summary> = self.sql.exec(select_query, None)?.to_array()?;
        Ok(rows.pop())
    }

    /// Writes nothing if a reset deleted the rows during the model call. Returns whether it
    /// wrote. The facts get the date of the first row.
    pub fn save_compaction(
        &self,
        rows: &[Row],
        summary: &str,
        facts: &[SettingChange],
    ) -> Result<bool> {
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
            return Ok(false);
        };
        let select_query = format!("{ROW} WHERE id = ? AND created_at = ?");
        let found: Vec<Row> = self
            .sql
            .exec(
                &select_query,
                Some(vec![last.id.into(), last.created_at.into()]),
            )?
            .to_array()?;
        if found.is_empty() {
            return Ok(false);
        }

        for fact in facts {
            self.change_setting(fact, first.created_at)?;
        }
        let upsert_query = "INSERT INTO summary (id, through, text) VALUES (1, ?, ?) \
            ON CONFLICT (id) DO UPDATE SET through = excluded.through, text = excluded.text";
        self.sql
            .exec(upsert_query, Some(vec![last.turn.0.into(), summary.into()]))?;
        Ok(true)
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

    /// Returns the messages in the turns after `after`, up to and including `up_to`, oldest
    /// first. `None` leaves that side open.
    pub fn chat(&self, after: Option<Turn>, up_to: Option<Turn>) -> Result<Vec<Row>> {
        let select_query = format!(
            "{ROW} WHERE (?1 IS NULL OR turn > ?1) \
            AND (?2 IS NULL OR turn <= ?2) ORDER BY turn, id"
        );
        let values = vec![
            after.map(|turn| turn.0).into(),
            up_to.map(|turn| turn.0).into(),
        ];
        self.sql.exec(&select_query, Some(values))?.to_array()
    }

    /// Finds past messages in the turns before `before_turn`. Each match comes with `NEIGHBORS`
    /// messages on each side. Matches that touch become one block. Returns the blocks oldest
    /// first.
    pub fn search_chat(
        &self,
        query: Option<&str>,
        after: Option<i64>,
        before: Option<i64>,
        before_turn: Turn,
    ) -> Result<Vec<Vec<Row>>> {
        // An FTS5 query: each word, quoted, as a prefix, joined by OR.
        let mut words = Vec::new();
        for word in query
            .unwrap_or_default()
            .split(|c: char| !c.is_alphanumeric())
        {
            if !word.is_empty() {
                words.push(format!("\"{word}\"*"));
            }
        }
        if words.is_empty() && after.is_none() && before.is_none() {
            return Ok(Vec::new());
        }

        let filter = "turn < ?1 AND (?2 IS NULL OR created_at >= ?2) \
            AND (?3 IS NULL OR created_at < ?3)";
        let select_query = if words.is_empty() {
            format!("{ROW} WHERE {filter} ORDER BY id DESC LIMIT {CHAT_HITS}")
        } else {
            format!(
                "{ROW} JOIN messages_fts AS f ON f.rowid = chat.id \
                WHERE {filter} AND messages_fts MATCH ?4 ORDER BY f.rank LIMIT {CHAT_HITS}"
            )
        };
        let mut values = vec![before_turn.0.into(), after.into(), before.into()];
        if !words.is_empty() {
            values.push(words.join(" OR ").into());
        }
        let hits: Vec<Row> = self.sql.exec(&select_query, Some(values))?.to_array()?;

        // Each hit and its neighbors, oldest first. Ranges that overlap or touch are joined.
        let mut ranges: Vec<(i64, i64)> = Vec::new();
        for hit in &hits {
            ranges.push((hit.id - NEIGHBORS, hit.id + NEIGHBORS));
        }
        ranges.sort();
        let mut joined: Vec<(i64, i64)> = Vec::new();
        for (from, to) in ranges {
            match joined.last_mut() {
                Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
                _ => joined.push((from, to)),
            }
        }

        let block_query = format!("{ROW} WHERE id BETWEEN ? AND ? AND turn < ? ORDER BY id");
        let mut blocks = Vec::new();
        for (from, to) in joined {
            let values = vec![from.into(), to.into(), before_turn.0.into()];
            let block: Vec<Row> = self.sql.exec(&block_query, Some(values))?.to_array()?;
            blocks.push(block);
        }
        Ok(blocks)
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
