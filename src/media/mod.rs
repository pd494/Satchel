//! What a user can save. One type per kind; each implements `Media`.

mod link;
mod note;

pub use link::Link;
pub use note::Note;
use serde_json::{json, Value};

/// What the Harness and `Db` need from any saved thing.
pub trait Media {
    /// The value for `saved.media`.
    fn get_type(&self) -> &'static str;

    /// The value for `saved.content`: what the user sent.
    fn content(&self) -> String;

    fn title(&self) -> Option<&str> {
        None
    }

    fn description(&self) -> Option<&str> {
        None
    }

    /// Fields only this type has, stored as JSON in `saved.data`.
    fn data(&self) -> Value {
        json!({})
    }
}
