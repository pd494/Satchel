use super::Media;

/// A saved message with no link in it.
pub struct Note {
    pub text: String,
}

impl Media for Note {
    fn get_type(&self) -> &'static str {
        "note"
    }

    fn content(&self) -> String {
        self.text.clone()
    }
}
