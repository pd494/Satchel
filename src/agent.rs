use crate::db::Row;
use serde::Serialize;
use serde_json::Value;
use worker::{Ai, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

// TODO: tools (Rig) go here later

/// The model call: history in, one assistant message out. Knows nothing about storage.
pub struct Agent {
    ai: Ai,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: Vec<&'a Row>,
    chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Serialize)]
struct ChatTemplateKwargs {
    enable_thinking: bool,
}

impl Agent {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }

    /// Sends the system prompt plus `history` to Workers AI and returns the assistant's reply text.
    ///
    /// Docs:
    /// - Workers AI model page: <https://developers.cloudflare.com/workers-ai/models/gemma-4-26b-a4b-it/>
    /// - `serde_json::Value`: <https://docs.rs/serde_json/latest/serde_json/enum.Value.html>
    pub async fn reply(&self, history: &[Row]) -> Result<String> {
        let system = Row {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
        };
        let mut messages = vec![&system];
        messages.extend(history);
        let request = ChatRequest {
            messages,
            chat_template_kwargs: ChatTemplateKwargs {
                enable_thinking: false,
            },
        };
        let response: Value = self.ai.run(MODEL, request).await?;

        response["choices"][0]["message"]["content"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .ok_or_else(|| Error::RustError(format!("no reply text in {response}")))
    }
}
