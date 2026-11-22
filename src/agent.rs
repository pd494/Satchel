use crate::db::Row;
use crate::tools::ToolCall;
use serde::Serialize;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;
use worker::js_sys::Promise;
use worker::wasm_bindgen_futures::JsFuture;
use worker::{Ai, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");
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
}

pub struct Agent {
    ai: AiBinding,
    tools: Value,
    options: JsValue,
}

impl Agent {
    pub fn new(ai: Ai) -> Result<Self> {
        Ok(Self {
            ai: ai.unchecked_into(),
            tools: ToolCall::definitions(),
            options: json!({ "gateway": { "id": "satchel" } }).serialize(&JSON)?,
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

    /// Calls the model once through AI Gateway and adds its message to `context`. Returns its
    /// trimmed text and its tool calls.
    pub async fn run_model(&self, context: &mut Vec<Value>) -> Result<(String, Calls)> {
        let input = json!({
            "messages": context,
            "tools": self.tools,
            "chat_template_kwargs": { "enable_thinking": false },
        })
        .serialize(&JSON)?;
        let output = JsFuture::from(self.ai.run(MODEL, input, self.options.clone())?).await?;
        let response =
            serde_wasm_bindgen::from_value::<Value>(output)?["choices"][0]["message"].take();
        let text = Self::clean(response["content"].as_str().unwrap_or_default());
        let calls = response["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|call| ToolCall::parse(&call["function"]).map_err(|error| error.to_string()))
            .collect();
        context.push(response);
        Ok((text, calls))
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
        let mut text = text.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "");
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
