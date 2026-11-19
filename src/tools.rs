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
        ])
    }
}
