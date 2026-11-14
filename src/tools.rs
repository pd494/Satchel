use schemars::{schema_for, JsonSchema, Schema};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[schemars(inline)]
pub enum Tapback {
    #[serde(rename = "👍")]
    Like,
    #[serde(rename = "❤️")]
    Love,
    #[serde(rename = "👎")]
    Dislike,
    #[serde(rename = "😂")]
    Laugh,
    #[serde(rename = "‼️")]
    Emphasize,
    #[serde(rename = "❓")]
    Question,
}

impl Tapback {
    pub fn emoji(self) -> &'static str {
        match self {
            Self::Like => "👍",
            Self::Love => "❤️",
            Self::Dislike => "👎",
            Self::Laugh => "😂",
            Self::Emphasize => "‼️",
            Self::Question => "❓",
        }
    }
}

/// Tapback the user's message.
#[derive(Deserialize, JsonSchema)]
struct React {
    emoji: Tapback,
}

/// Save labeled links and files from the user's message, and/or a note to remember.
#[derive(Deserialize, JsonSchema)]
struct Save {
    /// Labels like "L1" or "F1".
    #[serde(default)]
    items: Vec<String>,
    /// Text to remember, in the user's words.
    note: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum ToolCall {
    React {
        emoji: Tapback,
    },
    Save {
        items: Vec<String>,
        note: Option<String>,
    },
}

/// `Err` goes back to the model as an error message.
pub type ToolResult = Result<String, String>;

pub fn definitions() -> Value {
    let tool = |name: &str, mut parameters: Schema| {
        parameters.remove("$schema");
        parameters.remove("title");
        let description = parameters.remove("description");
        json!({ "type": "function", "function": {
            "name": name, "description": description, "parameters": parameters,
        }})
    };
    json!([
        tool("react", schema_for!(React)),
        tool("save", schema_for!(Save)),
    ])
}

/// `arguments` is a JSON string in the OpenAI format; some models send an object instead.
pub fn parse(name: &str, arguments: &Value) -> Result<ToolCall, String> {
    let bad = |error: serde_json::Error| format!("bad {name} arguments: {error}");
    let arguments = match arguments {
        Value::String(text) => serde_json::from_str(text).map_err(bad)?,
        other => other.clone(),
    };
    match name {
        "react" => serde_json::from_value(arguments)
            .map(|React { emoji }| ToolCall::React { emoji })
            .map_err(bad),
        "save" => serde_json::from_value(arguments)
            .map(|Save { items, note }| ToolCall::Save { items, note })
            .map_err(bad),
        _ => Err(format!("unknown tool {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_object_arguments() {
        assert_eq!(
            parse("react", &json!(r#"{"emoji":"👍"}"#)),
            Ok(ToolCall::React {
                emoji: Tapback::Like
            })
        );
        assert_eq!(
            parse(
                "save",
                &json!({ "items": ["L1"], "note": "wifi pw is on the fridge" })
            ),
            Ok(ToolCall::Save {
                items: vec!["L1".into()],
                note: Some("wifi pw is on the fridge".into())
            })
        );
        assert_eq!(
            parse("save", &json!({ "note": "x" })),
            Ok(ToolCall::Save {
                items: vec![],
                note: Some("x".into())
            })
        );
    }

    #[test]
    fn rejects_bad_calls() {
        assert!(parse("react", &json!({ "emoji": "🦀" })).is_err());
        assert!(parse("react", &json!("not json")).is_err());
        assert!(parse("delete", &json!({})).is_err());
    }

    #[test]
    fn definitions_inline_the_schemas() {
        let tools = definitions();
        assert_eq!(tools[0]["function"]["name"], "react");
        assert_eq!(
            tools[0]["function"]["parameters"]["properties"]["emoji"]["enum"][0],
            "👍"
        );
        assert_eq!(tools[1]["function"]["parameters"]["required"], Value::Null);
    }
}
