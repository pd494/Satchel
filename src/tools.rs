use schemars::{schema_for, JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Deserialize, JsonSchema)]
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
pub struct React {
    pub emoji: Tapback,
}

/// Save labeled links and files from the user's message, and/or a note to remember.
#[derive(Deserialize, JsonSchema)]
pub struct Save {
    /// Labels like "L1" or "F1".
    #[serde(default)]
    pub items: Vec<String>,
    /// Text to remember, in the user's words.
    pub note: Option<String>,
}

pub enum ToolCall {
    React(React),
    Save(Save),
}

/// `Err` goes back to the model as an error message.
pub type ToolResult = Result<String, String>;

#[derive(Serialize)]
pub struct Definition {
    #[serde(rename = "type")]
    kind: &'static str,
    function: Function,
}

#[derive(Serialize)]
struct Function {
    name: &'static str,
    description: Option<Value>,
    parameters: Schema,
}

impl Definition {
    fn new(name: &'static str, mut parameters: Schema) -> Self {
        parameters.remove("$schema");
        parameters.remove("title");
        let description = parameters.remove("description");
        Self {
            kind: "function",
            function: Function {
                name,
                description,
                parameters,
            },
        }
    }
}

pub fn definitions() -> [Definition; 2] {
    [
        Definition::new("react", schema_for!(React)),
        Definition::new("save", schema_for!(Save)),
    ]
}

/// `arguments` is a JSON string in the OpenAI format; some models send an object instead.
pub fn parse(name: &str, arguments: &Value) -> Result<ToolCall, String> {
    let bad = |error: serde_json::Error| format!("bad {name} arguments: {error}");
    let arguments = match arguments {
        Value::String(text) => serde_json::from_str(text).map_err(bad)?,
        other => other.clone(),
    };
    match name {
        "react" => serde_json::from_value(arguments).map(ToolCall::React),
        "save" => serde_json::from_value(arguments).map(ToolCall::Save),
        _ => return Err(format!("unknown tool {name}")),
    }
    .map_err(bad)
}
