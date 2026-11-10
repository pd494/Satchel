use mime_guess::mime;

use super::Media;

pub enum Kind {
    Image,
    Video,
    Voice,
}

pub struct File {
    pub kind: Kind,
    pub url: String,
    pub id: String,
}

impl Media for File {
    fn get_type(&self) -> &'static str {
        match self.kind {
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Voice => "voice",
        }
    }

    fn content(&self) -> String {
        self.url.clone()
    }
}

impl File {
    /// Reads a Linq file URL,
    /// `https://cdn.linqapp.com/attachments/partners/<partner>/<id>/<filename>`.
    pub fn parse(url: &str) -> Option<Self> {
        let path = url.strip_prefix("https://cdn.linqapp.com/attachments/")?;
        let (folder, filename) = path.rsplit_once('/')?;
        let (_, id) = folder.rsplit_once('/')?;
        let file_type = mime_guess::from_path(filename).first()?;
        let kind = match file_type.type_() {
            mime::IMAGE => Kind::Image,
            mime::VIDEO => Kind::Video,
            mime::AUDIO => Kind::Voice,
            _ => return None,
        };
        Some(Self {
            kind,
            url: url.to_owned(),
            id: id.to_owned(),
        })
    }
}
