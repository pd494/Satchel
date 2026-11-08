use crate::db::Row;
use crate::media::Link;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use wasm_bindgen::JsValue;
use worker::{console_error, Ai, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const CLEF: &str = "@cf/cloudflare/clef-flash";
const KEEP_LINK: f64 = 0.1;
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");

// TODO: tools (Rig) go here later

pub struct Agent {
    ai: Ai,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Intent {
    Save,
    Search,
    Remind,
    Chat,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: Vec<&'a Row>,
    chat_template_kwargs: ChatTemplateKwargs,
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

    /// Asks Clef `questions` about `state` and returns its `answers`.
    async fn ask_clef(&self, state: &str, questions: Value) -> Result<Value> {
        let request = json!({ "model": "clef-flash", "state": state, "questions": questions })
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())?;
        let out: Value = self.ai.run(CLEF, Js(request)).await?;
        Ok(out["answers"].clone())
    }

    /// Returns the intent and Clef's 0–1 confidence in it.
    pub async fn classify_intent(&self, content: &str) -> Result<(Intent, f64)> {
        let questions = json!({
                "intent": {
                    "type": "choice",
                    "instructions": "The user is texting Satchel, an app that saves what they send and finds it later. What does this message do?",
                    "criteria": {
                        "save": "Shares a link, photo, or note, or asks to keep or remember something. Sharing a link counts even with casual comments around it.",
                        "search": "Asks about or wants back something they saved before",
                        "remind": "Asks to be reminded at a later time",
                        "chat": "Greeting, thanks, reaction, or off-topic, with nothing to keep and nothing to find"
                    }
                }
        });

        let answer = self.ask_clef(content, questions).await?["intent"].take();
        let confidence = answer["confidence"].as_f64().unwrap_or(1.0);
        Ok((
            serde_json::from_value(answer["choice"].clone())?,
            confidence,
        ))
    }

    /// Keeps every link typed with a scheme, and each bare one unless Clef is sure it isn't a
    /// website. Clef sees the text as typed: `node.js` reads as a file, `https://node.js` doesn't.
    /// If Clef fails, keeps them all: a wrong link costs less than a lost one.
    pub async fn check_links(&self, content: &str, found: Vec<Link>) -> Vec<Link> {
        let questions: Map<String, Value> = found
            .iter()
            .enumerate()
            .filter(|(_, link)| !link.typed.contains("://"))
            .map(|(i, link)| {
                let instructions = format!("Is `{}` meant as a website address?", link.typed);
                (
                    format!("link.{i}"),
                    json!({ "type": "noul", "instructions": instructions }),
                )
            })
            .collect();
        if questions.is_empty() {
            return found;
        }
        let answers = match self.ask_clef(content, Value::Object(questions)).await {
            Ok(answers) => answers,
            Err(error) => {
                console_error!("check_links failed, keeping all: {error}");
                Value::Null
            }
        };
        found
            .into_iter()
            .enumerate()
            .filter(|(i, _)| {
                answers[format!("link.{i}").as_str()]["noul"]
                    .as_f64()
                    .is_none_or(|p| p >= KEEP_LINK)
            })
            .map(|(_, link)| link)
            .collect()
    }
}
