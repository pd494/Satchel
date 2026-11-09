use linkify::{LinkFinder, LinkKind};
use worker::Url;

use super::Media;

pub struct Link {
    pub url: Url,
    pub typed: String,
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
                Some(Self {
                    url,
                    typed: typed.to_owned(),
                })
            })
            .collect()
    }
}
