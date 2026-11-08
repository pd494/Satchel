use crate::db::{Row, Saved};
use crate::media::Bare;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fmt::Write;
use worker::{console_error, Ai, Date, Error, Result, Url};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const CLEF: &str = "@cf/cloudflare/clef-flash";
/// Lowest Clef `noul` that still keeps a bare match as a link.
const KEEP_LINK: f64 = 0.1;
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");

// TODO: tools (Rig) go here later

pub struct Agent {
    ai: Ai,
}

#[derive(PartialEq, Eq)]
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

    pub async fn classify_intent(&self, content: &str) -> Result<Intent> {
        let body = json!({
            "model": "clef-flash",
            "state": content,
            "questions": {
                "intent": {
                    "type": "choice",
                    "instructions": "What is the user trying to do?",
                    "criteria": {
                        "save": "Store a link, note, or media for later",
                        "search": "Find something they saved before",
                        "remind": "Set a reminder or schedule something",
                        "chat": "Greeting or off-topic; not save/search"
                    }
                }
            }
        });

        let out: Value = self.ai.run(CLEF, body).await?;
        let choice = out["answers"]["intent"]["choice"].as_str();
        match choice {
            Some("save") => Ok(Intent::Save),
            Some("search") => Ok(Intent::Search),
            Some("remind") => Ok(Intent::Remind),
            Some("chat") => Ok(Intent::Chat),
            other => Err(Error::RustError(format!("unknown intent: {other:?}"))),
        }
    }

    /// Keeps each bare match unless Clef is sure it isn't a website.
    /// If Clef fails, keeps them all: a wrong link costs less than a lost one.
    pub async fn check_links(&self, content: &str, bare: Vec<Bare>) -> Vec<Url> {
        if bare.is_empty() {
            return Vec::new();
        }
        let questions: Map<String, Value> = bare
            .iter()
            .enumerate()
            .map(|(i, candidate)| {
                let instructions = format!("Is `{}` meant as a website address?", candidate.text);
                (
                    format!("link.{i}"),
                    json!({ "type": "noul", "instructions": instructions }),
                )
            })
            .collect();
        let body = json!({ "model": "clef-flash", "state": content, "questions": questions });

        let answers = match self.ai.run::<_, Value>(CLEF, body).await {
            Ok(out) => out["answers"].clone(),
            Err(error) => {
                console_error!("check_links failed, keeping all: {error}");
                Value::Null
            }
        };
        bare.into_iter()
            .enumerate()
            .filter(|(i, _)| {
                answers[format!("link.{i}").as_str()]["noul"]
                    .as_f64()
                    .is_none_or(|p| p >= KEEP_LINK)
            })
            .map(|(_, candidate)| candidate.url)
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
        "The user's saved items, newest first. When they ask about one, reply with its URL.\n",
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
