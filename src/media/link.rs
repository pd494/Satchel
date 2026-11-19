use std::ops::Range;

use linkify::{LinkFinder, LinkKind};
use worker::Url;

use super::Media;

const FILE_EXTENSIONS: [&str; 7] = ["md", "py", "rs", "sh", "pl", "zip", "mov"];

pub struct Link {
    pub url: Url,
    pub suspicious: bool,
    span: Range<usize>,
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
    pub fn split(text: &str) -> (String, Vec<String>) {
        let links: Vec<Self> = Self::find_all(text)
            .into_iter()
            .filter(|link| !link.suspicious)
            .collect();
        let mut rest = String::new();
        let mut cursor = 0;
        for link in &links {
            rest.push_str(&text[cursor..link.span.start]);
            cursor = link.span.end;
        }
        rest.push_str(&text[cursor..]);
        let urls = links.iter().map(|link| link.url.to_string()).collect();
        (rest.trim().to_owned(), urls)
    }

    pub fn find_all(text: &str) -> Vec<Self> {
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
                let trusted_suffix = url
                    .host_str()
                    .and_then(|host| psl::suffix(host.as_bytes()))
                    .is_some_and(|suffix| {
                        suffix.is_known() && !FILE_EXTENSIONS.iter().any(|ext| suffix == *ext)
                    });
                Some(Self {
                    url,
                    suspicious: bare && !trusted_suffix,
                    span: start..start + typed.len(),
                })
            })
            .collect()
    }
}

fn is_trimmed(c: char) -> bool {
    !c.is_ascii() && !c.is_alphanumeric()
}
