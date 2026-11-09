mod attachment;
mod link;

pub use attachment::{Attachment, MediaKind};
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
