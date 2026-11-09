use std::ops::Range;

use linkify::{LinkFinder, LinkKind};
use worker::Url;

use super::Media;

pub struct Link {
    pub url: Url,
    pub typed: String,
    pub suspicious: bool,
}

impl Media for Link {
    fn get_type(&self) -> &'static str {
        "link"
    }

    fn content(&self) -> String {
        self.url.to_string()
    }
}

impl Link {
    pub fn find_all(text: &str) -> Vec<Self> {
        Self::find_spans(text)
            .into_iter()
            .map(|(_, link)| link)
            .collect()
    }

    /// Returns `text` with its links removed, and the links.
    pub fn split(text: &str) -> (String, Vec<Self>) {
        let spans = Self::find_spans(text);
        let mut rest = String::new();
        let mut cursor = 0;
        for (span, _) in &spans {
            rest.push_str(&text[cursor..span.start]);
            cursor = span.end;
        }
        rest.push_str(&text[cursor..]);
        let links = spans.into_iter().map(|(_, link)| link).collect();
        (rest.trim().to_owned(), links)
    }

    fn find_spans(text: &str) -> Vec<(Range<usize>, Self)> {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url]).url_must_have_scheme(false);
        finder
            .links(text)
            .filter_map(|found| {
                let raw = found.as_str();
                let typed = raw.trim_matches(is_trimmed);
                let start = found.start() + raw.len() - raw.trim_start_matches(is_trimmed).len();
                let typed_url = Url::parse(typed).ok().filter(Url::has_host);
                let bare = typed_url.is_none();
                let url = typed_url.or_else(|| Url::parse(&format!("https://{typed}")).ok())?;
                let known_suffix = url
                    .host_str()
                    .and_then(|host| psl::suffix(host.as_bytes()))
                    .is_some_and(|suffix| suffix.is_known());
                let link = Self {
                    url,
                    typed: typed.to_owned(),
                    suspicious: bare && !known_suffix,
                };
                Some((start..start + typed.len(), link))
            })
            .collect()
    }
}

fn is_trimmed(c: char) -> bool {
    !c.is_ascii() && !c.is_alphanumeric()
}
