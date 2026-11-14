use crate::ai;
use crate::db::Row;
use crate::tools::{self, ToolCall, ToolResult};
use serde_json::{json, Value};
use std::future::Future;
use worker::{Ai, Result};

const MODEL: &str = "@cf/google/gemma-4-26b-a4b-it";
const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");
const GATEWAY: &str = "satchel";
const MAX_ROUNDS: usize = 4;

pub trait ToolExecutor {
    async fn execute(&mut self, call: ToolCall) -> ToolResult;
}

pub struct Agent {
    ai: Ai,
    user_id: String,
}

impl Agent {
    pub fn new(ai: Ai, user_id: String) -> Self {
        Self { ai, user_id }
    }

    /// Returns the model's reply text, empty if it only used tools. `labels` is appended to the
    /// newest message so the model can save its links and files by label.
    pub async fn response(
        &self,
        history: &[Row],
        labels: &str,
        executor: &mut impl ToolExecutor,
    ) -> Result<String> {
        let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
        messages.extend(
            history
                .iter()
                .map(|row| json!({ "role": row.role, "content": row.content })),
        );
        if let Some(Value::String(content)) = messages.last_mut().map(|last| &mut last["content"]) {
            content.push_str(labels);
        }
        run(messages, executor, |messages| self.chat(messages)).await
    }

    async fn chat(&self, messages: Vec<Value>) -> Result<Value> {
        let input = json!({
            "messages": messages,
            "tools": tools::definitions(),
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let options =
            json!({ "gateway": { "id": GATEWAY, "metadata": { "user_id": self.user_id } } });
        ai::run(&self.ai, MODEL, &input, &options).await
    }
}

/// Runs tool rounds until the model stops calling tools or `MAX_ROUNDS` is reached.
async fn run<F: Future<Output = Result<Value>>>(
    mut messages: Vec<Value>,
    executor: &mut impl ToolExecutor,
    mut chat: impl FnMut(Vec<Value>) -> F,
) -> Result<String> {
    let mut text = String::new();
    for _ in 0..MAX_ROUNDS {
        let message = chat(messages.clone()).await?["choices"][0]["message"].take();
        text = message["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let calls = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if calls.is_empty() {
            break;
        }
        messages.push(message);
        for call in calls {
            let function = &call["function"];
            let result = match tools::parse(
                function["name"].as_str().unwrap_or_default(),
                &function["arguments"],
            ) {
                Ok(call) => executor.execute(call).await,
                Err(error) => Err(error),
            };
            let content = result.unwrap_or_else(|error| format!("error: {error}"));
            messages
                .push(json!({ "role": "tool", "tool_call_id": call["id"], "content": content }));
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tapback;
    use futures::executor::block_on;

    #[derive(Default)]
    struct Fake {
        calls: Vec<ToolCall>,
    }

    impl ToolExecutor for Fake {
        async fn execute(&mut self, call: ToolCall) -> ToolResult {
            let result = match &call {
                ToolCall::Save { items, .. } if items.iter().any(|item| item == "L9") => {
                    Err("unknown label L9".into())
                }
                _ => Ok("ok".into()),
            };
            self.calls.push(call);
            result
        }
    }

    fn call(name: &str, arguments: Value) -> Value {
        json!({ "id": name, "function": { "name": name, "arguments": arguments.to_string() } })
    }

    fn reply(content: &str, tool_calls: Vec<Value>) -> Value {
        json!({ "choices": [{ "message": { "role": "assistant", "content": content, "tool_calls": tool_calls } }] })
    }

    /// Plays `script` as the model's replies; returns the reply text, executed calls, and the
    /// messages the model saw last.
    fn play(script: Vec<Value>) -> (String, Vec<ToolCall>, Vec<Value>) {
        let mut script = script.into_iter();
        let mut seen = Vec::new();
        let mut fake = Fake::default();
        let user = vec![json!({ "role": "user", "content": "hi" })];
        let text = block_on(run(user, &mut fake, |messages| {
            seen = messages;
            let next = script.next().unwrap_or_else(|| reply("", vec![]));
            async move { Ok(next) }
        }))
        .unwrap();
        (text, fake.calls, seen)
    }

    fn save(items: &[&str], note: Option<&str>) -> ToolCall {
        ToolCall::Save {
            items: items.iter().map(|item| item.to_string()).collect(),
            note: note.map(str::to_string),
        }
    }

    const LIKE: ToolCall = ToolCall::React {
        emoji: Tapback::Like,
    };

    #[test]
    fn bare_link_is_saved_and_liked() {
        let (text, calls, _) = play(vec![
            reply(
                "",
                vec![
                    call("save", json!({ "items": ["L1"] })),
                    call("react", json!({ "emoji": "👍" })),
                ],
            ),
            reply("", vec![]),
        ]);
        assert_eq!(text, "");
        assert_eq!(calls, vec![save(&["L1"], None), LIKE]);
    }

    #[test]
    fn question_about_link_is_answered_not_saved() {
        let (text, calls, _) = play(vec![reply("Looks legit to me.", vec![])]);
        assert_eq!(text, "Looks legit to me.");
        assert!(calls.is_empty());
    }

    #[test]
    fn link_with_question_is_saved_and_answered() {
        let (text, calls, _) = play(vec![
            reply("", vec![call("save", json!({ "items": ["L1"] }))]),
            reply("Saved! Looks roomy.", vec![]),
        ]);
        assert_eq!(text, "Saved! Looks roomy.");
        assert_eq!(calls, vec![save(&["L1"], None)]);
    }

    #[test]
    fn photo_is_saved_and_liked() {
        let (_, calls, _) = play(vec![reply(
            "",
            vec![
                call("save", json!({ "items": ["F1"] })),
                call("react", json!({ "emoji": "👍" })),
            ],
        )]);
        assert_eq!(calls, vec![save(&["F1"], None), LIKE]);
    }

    #[test]
    fn note_is_saved_and_liked() {
        let note = "the wifi pw is on the fridge";
        let (_, calls, _) = play(vec![reply(
            "",
            vec![
                call("save", json!({ "note": note })),
                call("react", json!({ "emoji": "👍" })),
            ],
        )]);
        assert_eq!(calls, vec![save(&[], Some(note)), LIKE]);
    }

    #[test]
    fn thanks_gets_a_tapback_without_saving() {
        let (_, calls, _) = play(vec![reply(
            "",
            vec![call("react", json!({ "emoji": "❤️" }))],
        )]);
        assert_eq!(
            calls,
            vec![ToolCall::React {
                emoji: Tapback::Love
            }]
        );
    }

    #[test]
    fn errors_go_back_to_the_model() {
        let (_, calls, seen) = play(vec![
            reply(
                "",
                vec![
                    call("save", json!({ "items": ["L9"] })),
                    call("delete", json!({})),
                ],
            ),
            reply("Sorry, which one?", vec![]),
        ]);
        assert_eq!(calls, vec![save(&["L9"], None)]);
        assert_eq!(seen[2]["content"], "error: unknown label L9");
        assert_eq!(seen[3]["content"], "error: unknown tool delete");
        assert_eq!(seen[3]["tool_call_id"], "delete");
    }

    #[test]
    fn stops_after_max_rounds_with_last_text() {
        let looping = || reply("still going", vec![call("react", json!({ "emoji": "👍" }))]);
        let (text, calls, _) = play((0..10).map(|_| looping()).collect());
        assert_eq!(text, "still going");
        assert_eq!(calls.len(), MAX_ROUNDS);
    }
}
