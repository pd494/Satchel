use crate::db::Row;
use crate::media::Link;
use crate::tools::ToolCall;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;
use worker::js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use worker::wasm_bindgen_futures::JsFuture;
use worker::web_sys::Blob;
use worker::{Env, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
/// Decides which links in a burst the user sent to keep. It returns a choice, not text.
const CLEF: &str = "@cf/cloudflare/clef";
const KEEP: &str = "Keep it. The user sent the link alone, with a comment or note about it, with a message about something else, or asked to save it or be reminded of it, even if they also ask a question about it.";
const ASK: &str = "Only answer a question about this link, such as whether it is legit, what it says, who is in it, or what is similar to it. The user did not ask to keep it.";
const EMBED_MODEL: &str = "@cf/google/embeddinggemma-2";
/// The most characters one embedding reads. The model reads about 8,000 tokens.
const MAX_TEXT: usize = 4_000;
const TRANSCRIBE_MODEL: &str = "@cf/openai/whisper-large-v3-turbo";
/// Vectors are stored at this size. Queries must use the same size.
const DIMENSIONS: u32 = 512;
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");
/// A model reply takes about 1 second. The last of the 3 tries has no time limit.
const MODEL_TIMEOUT_MS: u32 = 15_000;
/// Parsed tool calls. A call that fails to parse holds the error text for the model.
pub type Calls = Vec<std::result::Result<ToolCall, String>>;

const JSON: serde_wasm_bindgen::Serializer = serde_wasm_bindgen::Serializer::json_compatible();

#[wasm_bindgen]
extern "C" {
    type AiBinding;

    /// `Ai::run` has no options argument, which AI Gateway routing needs.
    #[wasm_bindgen(method, catch)]
    fn run(
        this: &AiBinding,
        model: &str,
        input: JsValue,
        options: JsValue,
    ) -> std::result::Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = toMarkdown)]
    fn to_markdown(
        this: &AiBinding,
        document: JsValue,
        options: JsValue,
    ) -> std::result::Result<Promise, JsValue>;
}

pub struct Agent {
    ai: AiBinding,
    tools: Value,
    /// `EVAL_RUN` tags each AI Gateway log with the eval run that made it.
    eval_run: Option<String>,
}

impl Agent {
    /// Reads the `AI` binding. Any binding with a matching `run` method works. This skips
    /// `Env::ai`, which accepts only the Workers AI type.
    pub fn from_env(env: &Env) -> Result<Self> {
        let ai = Reflect::get(env, &"AI".into())?;
        if ai.is_undefined() {
            return Err("the AI binding is missing".into());
        }
        Ok(Self {
            ai: ai.unchecked_into(),
            tools: ToolCall::definitions(),
            eval_run: env.var("EVAL_RUN").ok().map(|run| run.to_string()),
        })
    }

    pub fn context(history: &[Row], now: &str) -> Vec<Value> {
        let system = format!("{SYSTEM_PROMPT}\n\n{now}");
        let mut context = vec![json!({ "role": "system", "content": system })];
        for row in history {
            context.push(json!({ "role": row.role, "content": row.content }));
        }
        context
    }

    /// Calls the model once and adds its message to `context`. Returns its trimmed text and its
    /// tool calls. The AI Gateway log gets the message's `event_id` and the tool loop `step`.
    pub async fn run_model(
        &self,
        context: &mut Vec<Value>,
        event_id: &str,
        step: usize,
    ) -> Result<(String, Calls)> {
        let input = json!({
            "messages": context,
            "tools": self.tools,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let mut metadata = json!({ "event_id": event_id, "step": step });
        if let Some(run) = &self.eval_run {
            metadata["eval_run"] = json!(run);
        }
        let options = json!({ "gateway": {
            "id": "satchel",
            "metadata": metadata,
            "requestTimeoutMs": MODEL_TIMEOUT_MS,
            "retries": { "maxAttempts": 3, "retryDelayMs": 500, "backoff": "exponential" },
        } });
        let mut output = self.call(MODEL, &input, Some(&options)).await?;
        let mut response = output["choices"][0]["message"].take();
        let text = Self::clean(response["content"].as_str().unwrap_or_default());
        let calls = Self::split_calls(&mut response);
        context.push(response);
        Ok((text, calls))
    }

    /// Splits `links` into the ones the user sent to keep and the ones they only asked about.
    pub async fn keep(&self, texts: &[&str], links: Vec<Link>) -> Result<(Vec<Link>, Vec<Link>)> {
        if links.is_empty() {
            return Ok((links, Vec::new()));
        }
        let questions: serde_json::Map<String, Value> = links
            .iter()
            .enumerate()
            .map(|(i, link)| {
                let instructions = format!("The user texted these messages to a save-for-later assistant. What does the user want done with the link {}?", link.url);
                let question = json!({ "type": "choice", "instructions": instructions, "criteria": { "keep": KEEP, "ask": ASK } });
                (format!("link{i}"), question)
            })
            .collect();
        let input =
            json!({ "model": "clef", "state": { "messages": texts }, "questions": questions });
        let options = json!({ "gateway": { "id": "satchel" } });
        let output = self.call(CLEF, &input, Some(&options)).await?;
        let mut asks = (0..).map(|i| output["answers"][format!("link{i}")]["choice"] == "ask");
        Ok(links.into_iter().partition(|_| asks.next() != Some(true)))
    }

    /// Gemma sometimes packs several calls into one, as JSON objects back to back, or sends
    /// arguments that are not JSON. Splits them into one call each, with valid JSON. The
    /// model rejects a context that holds invalid arguments.
    fn split_calls(response: &mut Value) -> Calls {
        let mut split = Vec::new();
        for call in response["tool_calls"].as_array().into_iter().flatten() {
            let arguments = &call["function"]["arguments"];
            let parts: Vec<Value> = match arguments.as_str() {
                Some(text) => serde_json::Deserializer::from_str(text)
                    .into_iter()
                    .collect::<serde_json::Result<Vec<Value>>>()
                    .ok()
                    .filter(|parts| !parts.is_empty())
                    .unwrap_or_else(|| vec![json!({})]),
                None => vec![arguments.clone()],
            };
            for (n, part) in parts.into_iter().enumerate() {
                let mut one = call.clone();
                one["id"] = json!(format!("{}-{n}", call["id"].as_str().unwrap_or_default()));
                one["function"]["arguments"] = json!(part.to_string());
                split.push(one);
            }
        }
        let calls = split
            .iter()
            .map(|call| ToolCall::parse(&call["function"]).map_err(|error| error.to_string()))
            .collect();
        if !split.is_empty() {
            response["tool_calls"] = Value::Array(split);
        }
        calls
    }

    /// The file as a `data:` URL, the way the models take images and video.
    pub fn data_url(mime: &str, bytes: &[u8]) -> String {
        let mut url = format!("data:{mime};base64,");
        BASE64.encode_string(bytes, &mut url);
        url
    }

    /// The embedding model's prefix for the text of a saved item.
    pub const SAVED_ITEM: &str = "title: none | text: ";
    /// The embedding model's prefix for a search query.
    pub const SEARCH: &str = "task: search result | query: ";

    /// Embeds `text` (and optional media) as little-endian `f32` bytes. `prefix` is
    /// `SAVED_ITEM` or `SEARCH`.
    pub async fn embed(
        &self,
        prefix: &str,
        text: &str,
        media: Option<&(String, Vec<u8>)>,
    ) -> Result<Vec<u8>> {
        let text: String = text.chars().take(MAX_TEXT).collect();
        let mut content = vec![json!({ "type": "text", "text": format!("{prefix}{text}") })];
        if let Some((mime, bytes)) = media {
            let kind = format!("{}_url", mime.split('/').next().unwrap_or_default());
            content.push(json!({ "type": kind, kind: { "url": Self::data_url(mime, bytes) } }));
        }
        let input = json!({
            "messages": [[{ "role": "user", "content": content }]],
            "dimensions": DIMENSIONS,
            "encoding_format": "base64",
        });
        let output = self.call(EMBED_MODEL, &input, None).await?;
        let vector = output["data"][0]["embedding"]
            .as_str()
            .ok_or("the embedding is missing")?;
        Ok(BASE64.decode(vector).map_err(|error| error.to_string())?)
    }

    pub async fn transcribe(&self, audio: &[u8]) -> Result<String> {
        let input = json!({ "audio": BASE64.encode(audio) });
        let output = self.call(TRANSCRIBE_MODEL, &input, None).await?;
        Ok(output["text"].as_str().unwrap_or_default().into())
    }

    /// Extracts document text (PDF, DOCX, etc.). The blob needs its MIME type.
    pub async fn read_document(&self, name: &str, mime: &str, bytes: &[u8]) -> Result<String> {
        let blob = Blob::new_with_u8_array_sequence(&Array::of1(&Uint8Array::from(bytes)))?
            .slice_with_i32_and_i32_and_content_type(0, bytes.len() as i32, mime)?;
        let doc = Object::new();
        Reflect::set(&doc, &"name".into(), &name.into())?;
        Reflect::set(&doc, &"blob".into(), &blob)?;
        let options = json!({
            "conversionOptions": { "pdf": { "metadata": false }, "output": { "format": "text" } },
        });
        let output = self.ai.to_markdown(doc.into(), options.serialize(&JSON)?)?;
        let output: Value = serde_wasm_bindgen::from_value(JsFuture::from(output).await?)?;
        Ok(output["data"]
            .as_str()
            .ok_or_else(|| output["error"].to_string())?
            .into())
    }

    /// `options` is `None` for media calls, which are too large for AI Gateway.
    async fn call(&self, model: &str, input: &Value, options: Option<&Value>) -> Result<Value> {
        let options = match options {
            Some(options) => options.serialize(&JSON)?,
            None => JsValue::UNDEFINED,
        };
        let output = self.ai.run(model, input.serialize(&JSON)?, options)?;
        let output = JsFuture::from(output).await?;
        Ok(serde_wasm_bindgen::from_value(output)?)
    }

    /// Adds one tool message per result, matched to the tool call ids in the last response.
    pub fn push_results(context: &mut Vec<Value>, results: Vec<String>) {
        let mut ids = Vec::new();
        if let Some(last) = context.last() {
            if let Some(tool_calls) = last["tool_calls"].as_array() {
                for call in tool_calls {
                    ids.push(call["id"].clone());
                }
            }
        }
        for (id, content) in ids.into_iter().zip(results) {
            context.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
        }
    }

    /// Removes what Gemma sometimes leaks into its text: a thinking block before `<channel|>`,
    /// special tokens like `<|file_separator|>`, and zero-width characters.
    fn clean(text: &str) -> String {
        // U+200D stays: it joins emoji such as 🧘‍♀️.
        let mut text = text.replace(['\u{200B}', '\u{200C}', '\u{FEFF}'], "");
        if let Some(end) = text.rfind("<channel|>") {
            text = text[end + "<channel|>".len()..].to_string();
        }
        while let Some(start) = text.find("<|") {
            let Some(len) = text[start..].find('>') else {
                break;
            };
            text.replace_range(start..=start + len, "");
        }
        text.trim().to_string()
    }
}
