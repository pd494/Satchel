use super::Media;

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
