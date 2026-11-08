use std::cell::RefCell;
use std::collections::HashMap;

use futures_util::StreamExt;
use html_escape::decode_html_entities;
use linkify::{LinkFinder, LinkKind};
use lol_html::{element, rewrite_str, text, RewriteStrSettings};
use serde_json::{json, Value};
use worker::{web_sys, Fetch, Headers, Method, Request, RequestInit, Response, Result, Url};

use super::Media;

const MAX_HTML_BYTES: usize = 500_000;

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
                    found.bare.push(Bare { text: raw.to_string(), url });
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
    /// Fetches `url`, reads its og: tags, and adds oEmbed data when the page advertises an endpoint.
    pub async fn build_link(url: Url) -> Result<Link> {
        let sys = web_sys::Response::from(get(url.as_str()).await?);
        let final_url = Url::parse(&sys.url())?;
        let mut response = Response::from(sys);
        if !(200..=299).contains(&response.status_code()) {
            return Ok(Link::bare(final_url));
        }
        let (page, oembed_url) = Link::from_html(&read_head(&mut response).await?, &final_url)?;
        let Some(oembed_url) = oembed_url else {
            return Ok(page);
        };
        // A failed oEmbed call must not lose what the page already gave us.
        Ok(
            match Link::from_oembed(oembed_url, &final_url)
                .await
                .ok()
                .flatten()
            {
                Some(oembed) => oembed.merge(page),
                None => page,
            },
        )
    }

    /// Keeps each field of `self`, and fills the fields it lacks from `other`.
    fn merge(self, other: Link) -> Link {
        Link {
            url: self.url,
            title: self.title.or(other.title),
            description: self.description.or(other.description),
            image_url: self.image_url.or(other.image_url),
            site_name: self.site_name.or(other.site_name),
            image_alt: self.image_alt.or(other.image_alt),
            author: self.author.or(other.author),
        }
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

    /// `None` when the provider refuses (deleted post, private video).
    async fn from_oembed(mut endpoint: Url, page: &Url) -> Result<Option<Link>> {
        endpoint.query_pairs_mut().append_pair("format", "json");
        let mut response = get(endpoint.as_str()).await?;
        if !(200..=299).contains(&response.status_code()) {
            return Ok(None);
        }
        let json: Value = response.json().await?;
        let field = |key: &str| json[key].as_str().map(str::to_owned);
        // Some providers (X) send no `title`; their text is inside `html`.
        let html_text = || field("html").map(|html| strip_tags(&html));
        Ok(Some(Link {
            url: page.clone(),
            title: field("title").or_else(html_text),
            description: None,
            image_url: field("thumbnail_url").and_then(|s| Url::parse(&s).ok()),
            site_name: field("provider_name"),
            image_alt: None,
            author: field("author_name"),
        }))
    }

    /// Reads og: then twitter: then <title>, only up to `</head>`.
    /// Also returns the oEmbed endpoint the page advertises, if any.
    fn from_html(html: &str, base: &Url) -> Result<(Link, Option<Url>)> {
        let head = html.split("</head>").next().unwrap_or(html);
        let meta: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
        let page_title = RefCell::new(String::new());
        let oembed_href: RefCell<Option<String>> = RefCell::new(None);
        let settings = RewriteStrSettings::new()
            .append_element_content_handler(element!(
                r#"meta[property^="og:"], meta[name^="twitter:"], meta[name="description"], meta[name="author"]"#,
                |el| {
                    let key = el
                        .get_attribute("property")
                        .or_else(|| el.get_attribute("name"));
                    if let (Some(key), Some(value)) = (key, el.get_attribute("content")) {
                        meta.borrow_mut().entry(key).or_insert(value);
                    }
                    Ok(())
                }
            ))
            .append_element_content_handler(element!(
                r#"link[type="application/json+oembed"]"#,
                |el| {
                    if let Some(href) = el.get_attribute("href") {
                        oembed_href.borrow_mut().get_or_insert(href);
                    }
                    Ok(())
                }
            ))
            .append_element_content_handler(text!("title", |chunk| {
                page_title.borrow_mut().push_str(chunk.as_str());
                Ok(())
            }));
        rewrite_str(head, settings).map_err(|e| worker::Error::RustError(e.to_string()))?;

        let meta = meta.into_inner();
        let pick = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| meta.get(*key))
                .map(|value| decode_html_entities(value).into_owned())
        };
        let page_title = decode_html_entities(page_title.into_inner().trim()).into_owned();
        let link = Link {
            url: base.clone(),
            title: pick(&["og:title", "twitter:title"])
                .or((!page_title.is_empty()).then_some(page_title)),
            description: pick(&["og:description", "twitter:description", "description"]),
            image_url: pick(&["og:image", "twitter:image"]).and_then(|s| base.join(&s).ok()),
            site_name: pick(&["og:site_name"]),
            image_alt: pick(&["og:image:alt", "twitter:image:alt"]),
            author: pick(&["author"]),
        };
        let oembed = oembed_href
            .into_inner()
            .and_then(|href| base.join(&decode_html_entities(&href)).ok());
        Ok((link, oembed))
    }
}

async fn get(url: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("User-Agent", "Satchel/0.1")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    Fetch::Request(Request::new_with_init(url, &init)?)
        .send()
        .await
}

/// Reads the body only until `</head>` or `MAX_HTML_BYTES`, so a huge page is never fully buffered.
async fn read_head(response: &mut Response) -> Result<String> {
    let mut stream = response.stream()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend(chunk?);
        if bytes.len() >= MAX_HTML_BYTES || bytes.windows(7).any(|w| w == b"</head>") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    decode_html_entities(&text).into_owned()
}
