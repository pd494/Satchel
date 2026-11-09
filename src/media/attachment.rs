use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Media;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    pub fn from_mime(mime: &str) -> Option<Self> {
        let (top, _) = mime.split_once('/')?;
        if top.eq_ignore_ascii_case("image") {
            Some(Self::Image)
        } else if top.eq_ignore_ascii_case("video") {
            Some(Self::Video)
        } else {
            None
        }
    }
}

/// A photo or video the user texted, kept by Linq under `id`.
#[derive(Serialize, Deserialize)]
pub struct Attachment {
    pub kind: MediaKind,
    pub id: String,
    pub filename: Option<String>,
    pub url: String,
}

impl Media for Attachment {
    fn get_type(&self) -> &'static str {
        match self.kind {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
        }
    }

    fn content(&self) -> String {
        self.id.clone()
    }

    fn title(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    fn data(&self) -> Value {
        json!({ "url": self.url })
    }
}

impl Attachment {
    /// Removes each `[#id ...]` tag from `text`. Returns the rest and the saved ids, in order.
    pub fn split_tags(text: &str) -> (String, Vec<i64>) {
        let mut rest = String::new();
        let mut ids = Vec::new();
        let mut tail = text;
        while let Some(start) = tail.find("[#") {
            let after = &tail[start + 2..];
            let digits = after
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after.len());
            let close = after[digits..]
                .find(']')
                .filter(|&gap| gap == 0 || after[digits..].starts_with(' '));
            match (after[..digits].parse(), close) {
                (Ok(id), Some(gap)) => {
                    rest.push_str(&tail[..start]);
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                    tail = &after[digits + gap + 1..];
                }
                _ => {
                    rest.push_str(&tail[..start + 2]);
                    tail = after;
                }
            }
        }
        rest.push_str(tail);
        (rest.trim().to_owned(), ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_mime_types() {
        assert!(matches!(
            MediaKind::from_mime("image/jpeg"),
            Some(MediaKind::Image)
        ));
        assert!(matches!(
            MediaKind::from_mime("image/heic"),
            Some(MediaKind::Image)
        ));
        assert!(matches!(
            MediaKind::from_mime("Video/QuickTime"),
            Some(MediaKind::Video)
        ));
        assert!(matches!(
            MediaKind::from_mime("video/mp4"),
            Some(MediaKind::Video)
        ));
        assert!(MediaKind::from_mime("application/pdf").is_none());
        assert!(MediaKind::from_mime("audio/m4a").is_none());
        assert!(MediaKind::from_mime("image").is_none());
    }

    #[test]
    fn splits_tags_from_reply() {
        let (rest, ids) = Attachment::split_tags("Here it is [#12 image photo.jpg]");
        assert_eq!(rest, "Here it is");
        assert_eq!(ids, [12]);

        let (rest, ids) = Attachment::split_tags("[#3] and [#4 video a.mov] and [#3]");
        assert_eq!(rest, "and  and");
        assert_eq!(ids, [3, 4]);
    }

    #[test]
    fn keeps_text_that_is_not_a_tag() {
        let text = "item [#] or [#x] or [#5x] or [#7";
        assert_eq!(Attachment::split_tags(text), (text.to_owned(), vec![]));
    }
}
