use crate::db::Schedule;
use jiff::{civil::DateTime, tz::TimeZone, SignedDuration, Span, Timestamp};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Deserialize)]
pub enum Tapback {
    #[serde(rename = "👍")]
    Like,
    #[serde(rename = "❤️")]
    Love,
    #[serde(rename = "👎")]
    Dislike,
    #[serde(rename = "😂")]
    Laugh,
    #[serde(rename = "‼️")]
    Emphasize,
    #[serde(rename = "❓")]
    Question,
}

/// A model tool call, read from its `{ "name": ..., "arguments": ... }` function object.
#[derive(Deserialize)]
#[serde(tag = "name", content = "arguments", rename_all = "lowercase")]
pub enum ToolCall {
    React {
        emoji: Tapback,
    },
    Save {
        urls: Vec<String>,
        note: Option<String>,
    },
    Remind(Remind),
    Search(Search),
    Cancel {
        id: i64,
    },
    Settings {
        key: String,
        value: String,
    },
}

#[derive(Deserialize)]
pub struct Search {
    pub query: Option<String>,
    after: Option<DateTime>,
    before: Option<DateTime>,
}

impl Search {
    /// `after` and `before` as Unix seconds, read as local times in `zone`.
    pub fn bounds(&self, zone: &TimeZone) -> Result<(Option<i64>, Option<i64>), String> {
        let to_second = |at: DateTime| -> Result<i64, String> {
            let timestamp = zone.to_timestamp(at).map_err(|error| error.to_string())?;
            Ok(timestamp.as_second())
        };
        let after = match self.after {
            Some(at) => Some(to_second(at)?),
            None => None,
        };
        let before = match self.before {
            Some(at) => Some(to_second(at)?),
            None => None,
        };
        Ok((after, before))
    }
}

const MIN_REPEAT: SignedDuration = SignedDuration::from_mins(5);

#[derive(Deserialize)]
struct RawRemind {
    id: Option<i64>,
    note: String,
    at: Option<DateTime>,
    after: Option<Span>,
    repeat: Option<Span>,
    until: Option<DateTime>,
    time_zone: Option<String>,
}

#[derive(Deserialize)]
#[serde(try_from = "RawRemind")]
pub struct Remind {
    /// The pending reminder to move. `None` sets a new one.
    pub id: Option<i64>,
    pub note: String,
    /// The zone of a clock-time reminder (`at`). `None` for a duration (`after`).
    pub at_zone: Option<TimeZone>,
    pub schedule: Schedule,
}

impl TryFrom<RawRemind> for Remind {
    type Error = String;

    fn try_from(raw: RawRemind) -> Result<Self, Self::Error> {
        let time_zone = raw
            .time_zone
            .as_deref()
            .map(TimeZone::get)
            .transpose()
            .map_err(|error| error.to_string())?;

        if raw.until.is_some() && time_zone.is_none() {
            return Err("time_zone is required with until".to_string());
        }

        let due = match (raw.at, raw.after, time_zone.clone()) {
            (Some(at), None, Some(zone)) => at.to_zoned_datetime(zone),
            (Some(_), None, None) => return Err("time_zone is required with at".to_string()),
            (None, Some(after), zone) => Timestamp::now()
                .to_zoned_datetime(zone.unwrap_or(TimeZone::UTC))
                .checked_add(after),
            _ => return Err("send exactly one of at or after".to_string()),
        }
        .map_err(|error| error.to_string())?;

        let until_at = raw
            .until
            .map(|until| until.to_zoned_datetime(due.time_zone().clone()))
            .transpose()
            .map_err(|error| error.to_string())?
            .map(|until| until.timestamp().as_second());

        if let Some(repeat) = raw.repeat {
            let next = due.checked_add(repeat).map_err(|error| error.to_string())?;
            if due.duration_until(&next) < MIN_REPEAT {
                return Err(format!(
                    "a reminder can repeat at most every {MIN_REPEAT:#}"
                ));
            }
        }

        Ok(Self {
            id: raw.id,
            note: raw.note,
            at_zone: raw.at.and(time_zone),
            schedule: Schedule {
                due_at: due.timestamp().as_second(),
                repeat: raw.repeat,
                until_at,
            },
        })
    }
}

impl ToolCall {
    /// `arguments` is a JSON string in the OpenAI format; some models send an object instead.
    pub fn parse(function: &Value) -> serde_json::Result<Self> {
        let arguments = match &function["arguments"] {
            Value::String(text) => serde_json::from_str(text)?,
            other => other.clone(),
        };

        serde_json::from_value(json!({ "name": function["name"], "arguments": arguments }))
    }

    pub fn definitions() -> Value {
        json!([
            { "type": "function", "function": {
                "name": "react",
                "description": "Tapback the user's newest message.",
                "parameters": { "type": "object", "required": ["emoji"], "properties": {
                    "emoji": { "type": "string", "enum": ["👍", "❤️", "👎", "😂", "‼️", "❓"] },
                }},
            }},
            { "type": "function", "function": {
                "name": "save",
                "description": "Save links and files from the newest messages, or a note to remember. Returns each item's search text.",
                "parameters": { "type": "object", "required": ["urls"], "properties": {
                    "urls": { "type": "array", "items": { "type": "string" }, "description": "URLs exactly as they appear in the newest messages. Use a URL from an earlier message only when the user is now telling you what it is." },
                    "note": { "type": "string", "description": "The user's words, as they wrote them. With urls, they are saved with those links or files. Without urls, they are saved as a note." },
                }},
            }},
            { "type": "function", "function": {
                "name": "search",
                "description": "Find saved links, files, and notes. Returns up to 10 results, best first, each with its URL.",
                "parameters": { "type": "object", "properties": {
                    "query": { "type": "string", "description": "What the user is looking for, in a few words, such as \"hip exercise video\" or \"austin apartment\". Omit it to list the newest saves." },
                    "after": { "type": "string", "description": "Only items saved at or after this local date and time, such as 2026-11-16T00:00:00. Work it out from the \"Now:\" line. Omit it unless the user names a time." },
                    "before": { "type": "string", "description": "Only items saved before this local date and time, such as 2026-11-23T00:00:00. Omit it unless the user names a time." },
                }},
            }},
            { "type": "function", "function": {
                "name": "remind",
                "description": "Set a reminder, or move a pending one.",
                "parameters": { "type": "object", "required": ["note"], "properties": {
                    "id": { "type": "integer", "description": "To move a pending reminder: its id from the \"Pending reminders\" list. Its note stays the same. Omit it to set a new reminder." },
                    "note": { "type": "string", "description": "What to remind the user about, in their words." },
                    "at": { "type": "string", "description": "For a clock time or date (\"at 3\", \"friday 9 am\"): the local date and time to fire, no offset, such as 2026-11-24T09:00:00. Send exactly one of `at` or `after`." },
                    "after": { "type": "string", "description": "For an amount of time from now (\"in 1 minute\", \"in 3 months\"): an ISO 8601 duration. PT30S = 30 seconds, PT1M = 1 minute, PT2H = 2 hours, P3D = 3 days, P3M = 3 months. Send exactly one of `at` or `after`." },
                    "time_zone": { "type": "string", "description": "The user's IANA time zone, such as America/Los_Angeles. Required with `at` or `until`; omit for `after` if unknown." },
                    "repeat": { "type": "string", "description": "ISO 8601 duration between repeats, counted from each due time. P1D = daily, P1W = weekly, P2W = every 2 weeks, P1M = monthly, P1Y = yearly. The shortest repeat is PT5M. Omit for a one-time reminder." },
                    "until": { "type": "string", "description": "For a repeating reminder: the last local date and time it can fire, in `time_zone`, such as 2026-12-31T23:59:00. For \"5 times\", use the time of the fifth fire. Omit to repeat forever." },
                }},
            }},
            { "type": "function", "function": {
                "name": "cancel",
                "description": "Cancel a pending reminder.",
                "parameters": { "type": "object", "required": ["id"], "properties": {
                    "id": { "type": "integer", "description": "Its id from the \"Pending reminders\" list." },
                }},
            }},
            { "type": "function", "function": {
                "name": "settings",
                "description": "Change one of the user's settings.",
                "parameters": { "type": "object", "required": ["key", "value"], "properties": {
                    "key": { "type": "string", "enum": ["time_zone"], "description": "`time_zone`: the user's time zone. Change it when they say where they live or that they moved, with no reminder. Returns the new \"Now:\" line." },
                    "value": { "type": "string", "description": "The new value. For `time_zone`, an IANA time zone, such as America/New_York." },
                }},
            }},
        ])
    }
}
