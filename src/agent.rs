use crate::db::{Row, Saved};
use crate::media::Link;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fmt::Write;
use wasm_bindgen::JsValue;
use worker::{console_error, Ai, Date, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const CLEF: &str = "@cf/cloudflare/clef-flash";
const KEEP_LINK: f64 = 0.1;
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");

// TODO: tools (Rig) go here later

pub struct Agent {
    ai: Ai,
}

pub(crate) enum Intent {
    /// `confidence` is Clef's 0–1 certainty that this is a save.
    Save {
        confidence: f64,
    },
    Search,
    Remind,
    Chat,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: Vec<&'a Row>,
    chat_template_kwargs: ChatTemplateKwargs,
}

/// Workers AI turns Rust maps into JS `Map`s, which Clef rejects. `questions` has dynamic keys
/// (`link.0`, ...), so it is built as a plain JS object up front and passed through as is.
#[derive(Serialize)]
struct ClefRequest<'a> {
    model: &'static str,
    state: &'a str,
    #[serde(with = "serde_wasm_bindgen::preserve")]
    questions: JsValue,
}

#[derive(Serialize)]
struct ChatTemplateKwargs {
    enable_thinking: bool,
}

impl Agent {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }

    pub async fn response(&self, history: &[Row], saved: &[Saved]) -> Result<String> {
        let system = Row {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
        };
        let saved = Row {
            role: "system".into(),
            content: saved_prompt(saved),
        };
        let mut messages = vec![&system, &saved];
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
    async fn ask_clef(&self, state: &str, questions: &Value) -> Result<Value> {
        let questions = questions.serialize(&serde_wasm_bindgen::Serializer::json_compatible())?;
        let request = ClefRequest {
            model: "clef-flash",
            state,
            questions,
        };
        let out: Value = self.ai.run(CLEF, request).await?;
        Ok(out["answers"].clone())
    }

    pub async fn classify_intent(&self, content: &str) -> Result<Intent> {
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

        let answers = self.ask_clef(content, &questions).await?;
        let intent = &answers["intent"];
        match intent["choice"].as_str() {
            Some("save") => Ok(Intent::Save {
                confidence: intent["confidence"].as_f64().unwrap_or(1.0),
            }),
            Some("search") => Ok(Intent::Search),
            Some("remind") => Ok(Intent::Remind),
            Some("chat") => Ok(Intent::Chat),
            other => Err(Error::RustError(format!("unknown intent: {other:?}"))),
        }
    }

    /// Keeps every link typed with a scheme, and each bare one unless Clef is sure it isn't a
    /// website. Clef sees the text as typed: `node.js` reads as a file, `https://node.js` doesn't.
    /// If Clef fails, keeps them all: a wrong link costs less than a lost one.
    pub async fn check_links(&self, content: &str, found: Vec<(&str, Link)>) -> Vec<Link> {
        let questions: Map<String, Value> = found
            .iter()
            .enumerate()
            .filter(|(_, (typed, _))| !typed.contains("://"))
            .map(|(i, (typed, _))| {
                let instructions = format!("Is `{typed}` meant as a website address?");
                (
                    format!("link.{i}"),
                    json!({ "type": "noul", "instructions": instructions }),
                )
            })
            .collect();
        if questions.is_empty() {
            return found.into_iter().map(|(_, link)| link).collect();
        }
        let answers = match self.ask_clef(content, &Value::Object(questions)).await {
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
            .map(|(_, (_, link))| link)
            .collect()
    }
}

/// Lists the user's saved items for the model, so it can answer "what was that…?".
fn saved_prompt(saved: &[Saved]) -> String {
    let mut prompt = format!("Today is {}.\n", Date::now());
    if saved.is_empty() {
        prompt.push_str("The user has saved nothing yet.");
        return prompt;
    }
    prompt.push_str(
        "The user's saved items, newest first. Whenever you mention a saved link, include its full URL.\n",
    );
    for item in saved {
        let _ = write!(
            prompt,
            "- {} {}: {}",
            item.saved_on, item.media, item.content
        );
        for extra in [&item.title, &item.description].into_iter().flatten() {
            let _ = write!(prompt, " | {extra}");
        }
        prompt.push('\n');
    }
    prompt
}
