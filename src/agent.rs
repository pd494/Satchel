use crate::db::Row;
use crate::harness::Turn;
use crate::tools::{self, Definition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use worker::js_sys::Promise;
use worker::wasm_bindgen_futures::JsFuture;
use worker::{Ai, Error, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");
const GATEWAY: &str = "satchel";
const MAX_ROUNDS: usize = 4;

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
    ai: Ai,
    user_id: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: &'a [ChatMessage],
    tools: &'a [Definition],
    chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Serialize)]
struct ChatTemplateKwargs {
    enable_thinking: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: Role,
    content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ModelToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ModelToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: ToolKind,
    function: FunctionCall,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ToolKind {
    Function,
}

#[derive(Clone, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    /// A JSON string in the OpenAI format; some models send an object instead.
    arguments: Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Serialize)]
struct RunOptions<'a> {
    gateway: Gateway<'a>,
}

#[derive(Serialize)]
struct Gateway<'a> {
    id: &'a str,
    metadata: Metadata<'a>,
}

#[derive(Serialize)]
struct Metadata<'a> {
    user_id: &'a str,
}

impl ChatMessage {
    fn new(role: Role, content: String, tool_call_id: Option<String>) -> Self {
        Self {
            role,
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id,
        }
    }
}

impl Agent {
    pub fn new(ai: Ai, user_id: String) -> Self {
        Self { ai, user_id }
    }

    /// Runs tool rounds until the model stops calling tools or `MAX_ROUNDS` is reached, and
    /// returns its last reply text, empty if it only used tools. `labels` is appended to the
    /// newest message so the model can save its links and files by label.
    pub async fn response(
        &self,
        history: &[Row],
        labels: &str,
        turn: &mut Turn<'_>,
    ) -> Result<String> {
        let mut messages = vec![ChatMessage::new(Role::System, SYSTEM_PROMPT.into(), None)];
        messages.extend(
            history
                .iter()
                .map(|row| ChatMessage::new(row.role, row.content.clone(), None)),
        );
        if let Some(content) = messages.last_mut().and_then(|last| last.content.as_mut()) {
            content.push_str(labels);
        }
        let tools = tools::definitions();
        let mut text = String::new();
        for _ in 0..MAX_ROUNDS {
            let request = ChatRequest {
                messages: &messages,
                tools: &tools,
                chat_template_kwargs: ChatTemplateKwargs {
                    enable_thinking: false,
                },
            };
            let message = self
                .chat(&request)
                .await?
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| Error::RustError("model returned no choices".into()))?
                .message;
            text = message.content.as_deref().unwrap_or_default().trim().into();
            if message.tool_calls.is_empty() {
                break;
            }
            let calls = message.tool_calls.clone();
            messages.push(message);
            for call in calls {
                let result = match tools::parse(&call.function.name, &call.function.arguments) {
                    Ok(parsed) => turn.execute(parsed).await,
                    Err(error) => Err(error),
                };
                let content = result.unwrap_or_else(|error| format!("error: {error}"));
                messages.push(ChatMessage::new(Role::Tool, content, Some(call.id)));
            }
        }
        Ok(text)
    }

    async fn chat(&self, request: &ChatRequest<'_>) -> Result<ChatResponse> {
        let options = RunOptions {
            gateway: Gateway {
                id: GATEWAY,
                metadata: Metadata {
                    user_id: &self.user_id,
                },
            },
        };
        let json = serde_wasm_bindgen::Serializer::json_compatible();
        let binding: &AiBinding = self.ai.as_ref().unchecked_ref();
        let promise = binding.run(MODEL, request.serialize(&json)?, options.serialize(&json)?)?;
        Ok(serde_wasm_bindgen::from_value(
            JsFuture::from(promise).await?,
        )?)
    }
}
