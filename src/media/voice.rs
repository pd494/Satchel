use mime_guess::mime;

use super::Media;

pub struct Voice {
    pub url: String,
    pub id: String,
}

impl Media for Voice {
    fn get_type(&self) -> &'static str {
        "voice"
    }

    fn content(&self) -> String {
        self.url.clone()
    }
}

impl Voice {
    pub fn parse(url: &str) -> Option<Self> {
        let (id, file_type) = super::linq_file(url)?;
        (file_type.type_() == mime::AUDIO).then(|| Self {
            url: url.to_owned(),
            id,
        })
    }
}
