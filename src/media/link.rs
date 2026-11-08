use std::cell::RefCell;

use html_escape::decode_html_entities;
use linkify::{LinkFinder, LinkKind};
use lol_html::{element, rewrite_str, text, RewriteStrSettings};
use worker::{console_error, Fetch, Headers, Request, RequestInit, Result, Url};

use super::Media;

pub struct Link {
    pub url: Url,
    /// The link as the user typed it, which Clef needs to judge it.
    pub typed: String,
    pub title: Option<String>,
    pub description: Option<String>,
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
}

impl Link {
    /// Finds every link in `text`, with or without a scheme. A match with no scheme gets `https://`.
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
                    title: None,
                    description: None,
                })
            })
            .collect()
    }

    /// Fills `title` and `description` from the page, so a later question can match what it is
    /// about. A page that can't be read leaves them empty; the link is still saved.
    pub async fn describe(&mut self) {
        if let Err(error) = self.read_page().await {
            console_error!("couldn't read {}: {error}", self.url);
        }
    }

    async fn read_page(&mut self) -> Result<()> {
        let headers = Headers::new();
        headers.set("User-Agent", "Satchel/0.1")?;
        let mut init = RequestInit::new();
        init.with_headers(headers);
        let mut response = Fetch::Request(Request::new_with_init(self.url.as_str(), &init)?)
            .send()
            .await?;
        if response.status_code() != 200 {
            return Ok(());
        }
        let html = response.text().await?;
        let head = html.split("</head>").next().unwrap_or(&html);
        let title = RefCell::new(None);
        let description = RefCell::new(None);
        let page_title = RefCell::new(String::new());
        let settings = RewriteStrSettings::new()
            .append_element_content_handler(element!("meta[content]", |el| {
                let key = el.get_attribute("property").or(el.get_attribute("name"));
                let slot = match key.as_deref() {
                    Some("og:title") => &title,
                    Some("og:description" | "description") => &description,
                    _ => return Ok(()),
                };
                slot.borrow_mut()
                    .get_or_insert(el.get_attribute("content").unwrap_or_default());
                Ok(())
            }))
            .append_element_content_handler(text!("title", |chunk| {
                page_title.borrow_mut().push_str(chunk.as_str());
                Ok(())
            }));
        rewrite_str(head, settings).map_err(|e| worker::Error::RustError(e.to_string()))?;
        let page_title = Some(page_title.into_inner()).filter(|t| !t.trim().is_empty());
        let clean = |value: Option<String>| {
            value
                .map(|v| decode_html_entities(v.trim()).into_owned())
                .filter(|v| !v.is_empty())
        };
        self.title = clean(title.into_inner().or(page_title));
        self.description = clean(description.into_inner());
        Ok(())
    }
}
