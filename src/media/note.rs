use super::Media;

pub struct Note(pub String);

impl Media for Note {
    fn get_type(&self) -> &'static str {
        "note"
    }

    fn content(&self) -> String {
        self.0.clone()
    }
}
