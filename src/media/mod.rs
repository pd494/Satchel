mod file;
mod link;

pub use file::{File, Kind};
pub use link::Link;
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

pub struct Note(pub String);

impl Media for Note {
    fn get_type(&self) -> &'static str {
        "note"
    }

    fn content(&self) -> String {
        self.0.clone()
    }
}
