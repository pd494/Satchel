use crate::db::Row;
use serde_json::{json, Value};
use worker::{Ai, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

// TODO: tools (Rig) go here later

/// The model call: history in, one assistant message out. Knows nothing about storage.
pub struct Agent {
    ai: Ai,
}

impl Agent {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }

    pub async fn response(&self, history: &[Row]) -> Result<String> {
        let system = Row {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
        };
        let mut messages = vec![&system];
        messages.extend(history);
        let request = json!({
            "messages": messages,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let response: Value = self.ai.run(MODEL, request).await?;

        response["choices"][0]["message"]["content"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .ok_or_else(|| Error::RustError(format!("no reply text in {response}")))
    }
}
