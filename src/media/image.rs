use mime_guess::mime;

use super::Media;

pub struct Image {
    pub url: String,
    pub id: String,
}

impl Media for Image {
    fn get_type(&self) -> &'static str {
        "image"
    }

    fn content(&self) -> String {
        self.url.clone()
    }
}

impl Image {
    pub fn parse(url: &str) -> Option<Self> {
        let (id, file_type) = super::linq_file(url)?;
        (file_type.type_() == mime::IMAGE).then(|| Self {
            url: url.to_owned(),
            id,
        })
    }
}
