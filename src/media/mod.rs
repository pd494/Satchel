mod link;
mod note;

pub use link::Link;
pub use note::Note;
use serde_json::{json, Value};

pub trait Media {
    fn get_type(&self) -> &'static str;

    fn content(&self) -> String;

    fn title(&self) -> Option<&str> {
        None
    }

    fn description(&self) -> Option<&str> {
        None
    }

    fn data(&self) -> Value {
        json!({})
    }
}
