use jiff::{civil::DateTime, Span};
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
    Remind {
        note: String,
        at: Option<DateTime>,
        after: Option<Span>,
        repeat: Option<Span>,
        until: Option<DateTime>,
        time_zone: String,
    },
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
                "description": "Save links and files from the newest message, and/or a note to remember.",
                "parameters": { "type": "object", "required": ["urls"], "properties": {
                    "urls": { "type": "array", "items": { "type": "string" }, "description": "URLs exactly as they appear in the newest message." },
                    "note": { "type": "string", "description": "Text to remember, in the user's words." },
                }},
            }},
            { "type": "function", "function": {
                "name": "remind",
                "description": "Set a reminder.",
                "parameters": { "type": "object", "required": ["note", "time_zone"], "properties": {
                    "note": { "type": "string", "description": "What to remind the user about, in their words." },
                    "at": { "type": "string", "description": "For a clock time or date (\"at 3\", \"friday 9 am\"): the local date and time to fire, no offset, such as 2026-11-24T09:00:00. Send exactly one of `at` or `after`." },
                    "after": { "type": "string", "description": "For an amount of time from now (\"in 1 minute\", \"in 3 months\"): an ISO 8601 duration. PT30S = 30 seconds, PT1M = 1 minute, PT2H = 2 hours, P3D = 3 days, P3M = 3 months. Send exactly one of `at` or `after`." },
                    "time_zone": { "type": "string", "description": "The user's IANA time zone, such as America/Los_Angeles. `at` is in this zone." },
                    "repeat": { "type": "string", "description": "ISO 8601 duration between repeats, counted from each due time. P1D = daily, P1W = weekly, P2W = every 2 weeks, P1M = monthly, P1Y = yearly. The shortest repeat is PT5M. Omit for a one-time reminder." },
                    "until": { "type": "string", "description": "For a repeating reminder: the last local date and time it can fire, in `time_zone`, such as 2026-12-31T23:59:00. For \"5 times\", use the time of the fifth fire. Omit to repeat forever." },
                }},
            }},
        ])
    }
}
