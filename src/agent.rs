use crate::media::Link;
use crate::tools::{ToolCall, ToolResult};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
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
/// Scores how well each passage answers a query. It writes no text.
const RERANK_MODEL: &str = "@cf/baai/bge-reranker-base";
const READ_MEDIA: &str = "Describe what this shows in one or two short lines: the main subjects (people, mascots, animals, food, objects), the place, and any event. Then write every piece of visible text, exactly as written, including scores and numbers, one item per line.";
/// The most characters one embedding reads. The model reads about 8,000 tokens.
const MAX_TEXT: usize = 4_000;
const TRANSCRIBE_MODEL: &str = "@cf/openai/whisper-large-v3-turbo";
/// Vectors are stored at this size. Queries must use the same size.
const DIMENSIONS: u32 = 512;
/// A model reply takes about 1 second. The last of the 3 tries has no time limit.
const MODEL_TIMEOUT_MS: u32 = 15_000;
/// A compaction or a media reading. Each writes a long text, and no one waits on it.
const LONG_TEXT_TIMEOUT_MS: u32 = 60_000;
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

/// One reranker score: `id` is the passage's place in the input.
#[derive(Deserialize)]
struct Ranked {
    id: usize,
    score: f32,
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

    /// Calls the model once and adds its message to `context`. Returns its trimmed text and its
    /// tool calls. `metadata` tags the AI Gateway log.
    pub async fn run_model(
        &self,
        context: &mut Vec<Value>,
        mut metadata: Value,
        response_schema: Option<&Value>,
    ) -> Result<(String, Calls)> {
        let mut input = json!({
            "messages": context,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        // None: the model gets the tools and answers with text or tool calls.
        // Some: the model gets no tools and answers with JSON in the schema's shape.
        let timeout_ms = match response_schema {
            Some(response_schema) => {
                input["response_format"] =
                    json!({ "type": "json_schema", "json_schema": response_schema });
                LONG_TEXT_TIMEOUT_MS
            }
            None => {
                input["tools"] = self.tools.clone();
                MODEL_TIMEOUT_MS
            }
        };
        if let Some(run) = &self.eval_run {
            metadata["eval_run"] = json!(run);
        }
        let options = Self::gateway(metadata, timeout_ms);
        let mut output = self.call(MODEL, &input, Some(&options)).await?;
        let mut response = output["choices"][0]["message"].take();
        let text = match &response["content"] {
            Value::String(text) => Self::clean(text),
            Value::Null => String::new(),
            json => json.to_string(),
        };
        let calls = Self::split_calls(&mut response);
        context.push(response);
        Ok((text, calls))
    }

    fn gateway(metadata: Value, timeout_ms: u32) -> Value {
        json!({ "gateway": {
            "id": "satchel",
            "metadata": metadata,
            "requestTimeoutMs": timeout_ms,
            "retries": { "maxAttempts": 3, "retryDelayMs": 500, "backoff": "exponential" },
        } })
    }

    /// Asks Clef about each of `links`. Returns the links to save, and the links the user only
    /// asked about, which Satchel reads to answer.
    pub async fn save_or_read(
        &self,
        texts: &[&str],
        links: Vec<Link>,
    ) -> Result<(Vec<Link>, Vec<Link>)> {
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

    pub const SAVED_ITEM: &str = "title: none | text: ";
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

    /// What a saved image or video shows, then every piece of its visible text. Search reads this
    /// text, so a screenshot's gate or a scoreboard's score can be found and quoted later.
    pub async fn read_media(&self, (mime, bytes): &(String, Vec<u8>)) -> Result<String> {
        let kind = if mime.starts_with("video/") {
            "video_url"
        } else {
            "image_url"
        };
        let input = json!({
            "messages": [{ "role": "user", "content": [
                { "type": "text", "text": READ_MEDIA },
                { "type": kind, kind: { "url": Self::data_url(mime, bytes) } },
            ]}],
            "max_tokens": 400,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let options = Self::gateway(json!({ "step": "read_media" }), LONG_TEXT_TIMEOUT_MS);
        let output = self.call(MODEL, &input, Some(&options)).await?;
        let text = output["choices"][0]["message"]["content"].as_str();
        Ok(Self::clean(text.ok_or("the media reading is missing")?))
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
    /// Scores how well each of `passages` answers `query`, from 0 to 1, in the same order.
    pub async fn rerank(&self, query: &str, passages: &[String]) -> Result<Vec<f32>> {
        let mut contexts = Vec::new();
        for passage in passages {
            contexts.push(json!({ "text": passage }));
        }
        let input = json!({ "query": query, "contexts": contexts });
        let options = Self::gateway(json!({ "step": "rerank" }), MODEL_TIMEOUT_MS);
        let output = self.call(RERANK_MODEL, &input, Some(&options)).await?;
        let ranked: Vec<Ranked> = serde_json::from_value(output["response"].clone())?;
        let mut scores = vec![0.0; passages.len()];
        for item in ranked {
            scores[item.id] = item.score;
        }
        Ok(scores)
    }

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
    pub fn push_results(context: &mut Vec<Value>, results: Vec<ToolResult>) {
        let mut ids = Vec::new();
        if let Some(last) = context.last() {
            if let Some(tool_calls) = last["tool_calls"].as_array() {
                for call in tool_calls {
                    ids.push(call["id"].clone());
                }
            }
        }
        for (id, result) in ids.into_iter().zip(results) {
            context
                .push(json!({ "role": "tool", "tool_call_id": id, "content": result.into_text() }));
        }
    }

    /// Removes what Gemma sometimes leaks into its text: a thinking block before `<channel|>`,
    /// special tokens like `<|file_separator|>`, HTML tags, and zero-width characters.
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
        // iMessage shows no HTML. Gemma once wrote a tapback as a `<button>` tag. A tag starts
        // with a letter or "/", so "<3" stays.
        let mut from = 0;
        while let Some(found) = text[from..].find('<') {
            let start = from + found;
            let next = text[start + 1..].chars().next();
            let is_tag = matches!(next, Some(c) if c.is_ascii_alphabetic() || c == '/');
            match (is_tag, text[start..].find('>')) {
                (true, Some(len)) => text.replace_range(start..=start + len, ""),
                _ => from = start + 1,
            }
        }
        text.trim().to_string()
    }
}
