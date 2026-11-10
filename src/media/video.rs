use mime_guess::mime;

use super::Media;

pub struct Video {
    pub url: String,
    pub id: String,
}

impl Media for Video {
    fn get_type(&self) -> &'static str {
        "video"
    }

    fn content(&self) -> String {
        self.url.clone()
    }
}

impl Video {
    pub fn parse(url: &str) -> Option<Self> {
        let (id, file_type) = super::linq_file(url)?;
        (file_type.type_() == mime::VIDEO).then(|| Self {
            url: url.to_owned(),
            id,
        })
    }
}
