use crate::db::Row;
use crate::media::Link;
use serde::Serialize;
use serde_json::{json, Map, Value};
use wasm_bindgen::JsValue;
use worker::{Ai, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const CLEF: &str = "@cf/cloudflare/clef-flash";
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");

// TODO: tools (Rig) go here later

pub struct Agent {
    ai: Ai,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: Vec<ChatMessage<'a>>,
    chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// A JS value handed to Workers AI as is. Workers AI would turn a `json!` object into a JS `Map`,
/// which Clef rejects, so the request is converted to a plain JS object first.
#[derive(Serialize)]
struct Js(#[serde(with = "serde_wasm_bindgen::preserve")] JsValue);

#[derive(Serialize)]
struct ChatTemplateKwargs {
    enable_thinking: bool,
}

impl Agent {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }

    pub async fn response(&self, history: &[Row]) -> Result<String> {
        let system = ChatMessage {
            role: "system",
            content: SYSTEM_PROMPT,
        };
        let mut messages = vec![system];
        messages.extend(history.iter().map(|row| ChatMessage {
            role: &row.role,
            content: &row.content,
        }));
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

    async fn ask_clef(&self, state: &str, questions: Value) -> Result<Value> {
        let request = json!({ "model": "clef-flash", "state": state, "questions": questions })
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())?;
        let out: Value = self.ai.run(CLEF, Js(request)).await?;
        Ok(out["answers"].clone())
    }

    /// Returns Clef's 0–1 odds that each suspicious link is a website. `None` for any other link.
    pub async fn website_odds(&self, content: &str, links: &[Link]) -> Result<Vec<Option<f64>>> {
        let questions: Map<String, Value> = links
            .iter()
            .enumerate()
            .filter(|(_, link)| link.suspicious)
            .map(|(i, link)| {
                let instructions = format!("Is `{}` meant as a website address?", link.typed);
                (
                    format!("link.{i}"),
                    json!({ "type": "noul", "instructions": instructions }),
                )
            })
            .collect();
        if questions.is_empty() {
            return Ok(vec![None; links.len()]);
        }
        let answers = self.ask_clef(content, Value::Object(questions)).await?;
        Ok((0..links.len())
            .map(|i| answers[format!("link.{i}").as_str()]["noul"].as_f64())
            .collect())
    }
}
