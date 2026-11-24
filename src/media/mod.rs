mod file;
mod link;
mod note;
mod reader;

pub use file::{File, Kind};
pub use link::Link;
pub use note::Note;
pub use reader::Reader;
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
