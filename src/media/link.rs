use linkify::{LinkFinder, LinkKind};
use serde_json::{json, Value};
use worker::Url;

use super::Media;

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
    /// Finds every link in `text`, with or without a scheme, each paired with the text as typed.
    /// A match with no scheme gets `https://`.
    pub fn find_all(text: &str) -> Vec<(&str, Link)> {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url]).url_must_have_scheme(false);
        finder
            .links(text)
            .filter_map(|found| {
                let typed = found
                    .as_str()
                    .trim_matches(|c: char| !c.is_ascii() && !c.is_alphanumeric());
                let url = Url::parse(typed)
                    .ok()
                    .filter(Url::has_host)
                    .or_else(|| Url::parse(&format!("https://{typed}")).ok())?;
                Some((typed, Link::bare(url)))
            })
            .collect()
    }

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
