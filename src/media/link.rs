use linkify::{LinkFinder, LinkKind};
use serde_json::{json, Value};
use worker::Url;

use super::Media;

/// Links in one message, split by how sure we are.
#[derive(Default)]
pub struct Found {
    /// Typed with a scheme, like `https://zillow.com/x`.
    pub sure: Vec<Url>,
    /// No scheme, like `pd494.ai`. Could be a file name instead.
    pub bare: Vec<Bare>,
}

/// A schemeless match: the text as typed, and the URL with `https://` added.
pub struct Bare {
    pub text: String,
    pub url: Url,
}

/// Finds every link in `text`, with or without a scheme.
pub fn find_links(text: &str) -> Found {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]).url_must_have_scheme(false);
    let mut found = Found::default();
    for link in finder.links(text) {
        let raw = link
            .as_str()
            .trim_matches(|c: char| !c.is_ascii() && !c.is_alphanumeric());
        match Url::parse(raw) {
            Ok(url) if url.has_host() => found.sure.push(url),
            _ => {
                if let Ok(url) = Url::parse(&format!("https://{raw}")) {
                    found.bare.push(Bare {
                        text: raw.to_string(),
                        url,
                    });
                }
            }
        }
    }
    found
}

#[derive(Debug)]
pub struct Link {
    pub url: Url,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<Url>,
    pub site_name: Option<String>,
    pub image_alt: Option<String>,
    pub author: Option<String>,
}

impl Media for Link {
    fn get_type(&self) -> &'static str {
        "link"
    }

    fn content(&self) -> String {
        self.url.to_string()
    }

    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn data(&self) -> Value {
        json!({
            "image_url": self.image_url.as_ref().map(Url::as_str),
            "site_name": self.site_name,
            "image_alt": self.image_alt,
            "author": self.author,
        })
    }
}

impl Link {
    pub(crate) fn bare(url: Url) -> Link {
        Link {
            url,
            title: None,
            description: None,
            image_url: None,
            site_name: None,
            image_alt: None,
            author: None,
        }
    }
}
