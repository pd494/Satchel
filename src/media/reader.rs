use super::File;
use crate::agent::Agent;
use html_escape::decode_html_entities;
use lol_html::html_content::TextChunk;
use lol_html::{element, rewrite_str, text, HandlerResult, Settings};
use mime_guess::mime::{Mime, APPLICATION_OCTET_STREAM};
use serde_json::Value;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wildmatch::WildMatch;
use worker::js_sys::{decode_uri_component, Promise, Reflect, Uint8Array, JSON};
use worker::wasm_bindgen_futures::JsFuture;
use worker::web_sys::{AbortSignal, Headers, ReadableStream, RequestInit, Response};
use worker::{console_error, Env, Result, Url};

#[wasm_bindgen]
extern "C" {
    /// The global `fetch`. A `worker::Response` hides the URL after redirects.
    #[wasm_bindgen(js_name = fetch, catch)]
    fn global_fetch(url: &str, init: &RequestInit) -> std::result::Result<Promise, JsValue>;

    type Images;
    #[wasm_bindgen(method, catch)]
    fn input(this: &Images, image: &ReadableStream) -> std::result::Result<Transform, JsValue>;
    type Transform;
    #[wasm_bindgen(method)]
    fn transform(this: &Transform, options: &JsValue) -> Transform;
    #[wasm_bindgen(method, catch)]
    fn output(this: &Transform, options: &JsValue) -> std::result::Result<Promise, JsValue>;
    type Output;
    #[wasm_bindgen(method)]
    fn response(this: &Output) -> Response;
}

const USER_AGENT: &str = "Mozilla/5.0 (compatible; SatchelBot/0.1; +https://textsatchel.com)";
const OEMBED_REGISTRY: &str = "https://oembed.com/providers.json";
const JSON_LD: &str = "script[type='application/ld+json']";
/// The most text that goes into one embedding. The model reads about 8,000 tokens.
/// A Durable Object has 128 MB, and bytes are copied a few times on the way to the model.
const MAX_BYTES: usize = 10_000_000;
const TIMEOUT_MS: u32 = 10_000;

/// Extracted text and optional `(mime, bytes)` image or video to embed with it.
pub type Read = (String, Option<(String, Vec<u8>)>);

pub struct Reader {
    images: Images,
    providers: OnceCell<Vec<(String, Vec<WildMatch>)>>,
}

impl Reader {
    pub fn from_env(env: &Env) -> Result<Self> {
        let images = Reflect::get(env, &"IMAGES".into())?;
        if images.is_undefined() {
            return Err("the IMAGES binding is missing".into());
        }
        Ok(Self {
            images: images.unchecked_into(),
            providers: OnceCell::new(),
        })
    }

    /// Reads `url` and joins the user's words, extracted text, and URL or filename words.
    pub async fn read(&self, url: &Url, context: Option<String>, agent: &Agent) -> Read {
        let is_file = File::parse(url.as_str()).is_some();
        let read = self.try_read(url, is_file, agent).await;
        let (text, media) = read.unwrap_or_else(|error| {
            console_error!("read {url} failed: {error}");
            Read::default()
        });
        let name = url.as_str().rsplit('/').next().unwrap_or_default();
        let raw = if is_file { name } else { url.as_str() };
        let source = decode_uri_component(raw)
            .map(String::from)
            .unwrap_or_else(|_| raw.into());
        let mut words: Vec<&str> = context.as_deref().into_iter().collect();
        words.push(text.trim());
        words.extend(source.split(|c: char| !c.is_alphanumeric()));
        words.retain(|word| !word.is_empty());
        (words.join(" "), media)
    }

    async fn try_read(&self, url: &Url, is_file: bool, agent: &Agent) -> Result<Read> {
        let (final_url, mime, bytes) = Self::get(url.as_str()).await?;
        let final_url = Url::parse(&final_url).unwrap_or_else(|_| url.clone());
        let name = final_url.path().rsplit('/').next().unwrap_or_default();
        // A Linq file's name tells its type.
        let mime = match mime.parse::<Mime>() {
            Ok(mime) if !is_file && mime != APPLICATION_OCTET_STREAM => mime,
            _ => mime_guess::from_path(name).first_or_octet_stream(),
        };
        let essence = mime.essence_str();
        Ok(match (mime.type_().as_str(), mime.subtype().as_str()) {
            ("text", "html") => return self.page(url, &final_url, &bytes).await,
            ("text", sub) if sub.contains("vcard") => (Self::vcard(&bytes), None),
            ("text", _) => (String::from_utf8_lossy(&bytes).into(), None),
            ("image", _) => (String::new(), Some(self.jpeg(bytes).await?)),
            ("video", "mp4" | "quicktime") => (String::new(), Some((essence.into(), bytes))),
            ("audio", _) => (agent.transcribe(&bytes).await?, None),
            _ => (agent.read_document(name, essence, &bytes).await?, None),
        })
    }

    /// Reads a page's meta tags, JSON-LD, oEmbed, and preview image.
    async fn page(&self, url: &Url, base: &Url, html: &[u8]) -> Result<Read> {
        let meta = RefCell::new(HashMap::new());
        let (title, json_ld) = (RefCell::default(), RefCell::default());
        let settings = Settings::new()
            .append_element_content_handler(element!("meta[content]", |el| {
                let key = el.get_attribute("property").or(el.get_attribute("name"));
                let value = el.get_attribute("content").filter(|v| !v.trim().is_empty());
                if let (Some(k), Some(v)) = (key, value) {
                    meta.borrow_mut().entry(k).or_insert(v);
                }
                Ok(())
            }))
            .append_element_content_handler(text!("title", |c| Self::collect(&title, c)))
            .append_element_content_handler(text!(JSON_LD, |c| Self::collect(&json_ld, c)));
        // A page that fails to parse partway still keeps what was read before.
        if let Err(error) = rewrite_str(&String::from_utf8_lossy(html), settings) {
            console_error!("parse {url} failed: {error}");
        }

        let meta = meta.into_inner();
        let pick = |k: &str| meta.get(k).map(String::as_str);
        let oembed = self.oembed(base).await.unwrap_or_default();
        let field = |k: &str| oembed[k].as_str();
        let title = title.into_inner();
        let fallback = title.split('\0').next().unwrap_or_default();
        let mut text = vec![pick("og:title").or(field("title")).unwrap_or(fallback)];
        text.extend(pick("og:description").or(pick("description")));
        text.extend(field("author_name"));
        let html = field("html").unwrap_or_default();
        text.extend(html.split(['<', '>']).step_by(2));
        let mut text = text.join(" ");
        for block in json_ld.into_inner().split('\0') {
            Self::strings(&serde_json::from_str(block).unwrap_or_default(), &mut text);
        }
        let src = pick("og:image").or(field("thumbnail_url"));
        let image = async {
            let src = base.join(&Self::decode(src.ok_or("no image")?))?;
            self.jpeg(Self::get(src.as_str()).await?.2).await
        };
        Ok((Self::decode(&text), image.await.ok()))
    }

    async fn oembed(&self, url: &Url) -> Result<Value> {
        if self.providers.get().is_none() {
            let raw: Vec<Value> = serde_json::from_slice(&Self::get(OEMBED_REGISTRY).await?.2)?;
            let endpoints = raw
                .iter()
                .flat_map(|p| p["endpoints"].as_array().into_iter().flatten())
                .filter_map(|e| {
                    let api = e["url"].as_str()?.replace("{format}", "json");
                    let schemes = e["schemes"].as_array()?.iter().filter_map(Value::as_str);
                    Some((api, schemes.map(WildMatch::new).collect()))
                })
                .collect();
            let _ = self.providers.set(endpoints);
        }
        let mut endpoints = self.providers.get().into_iter().flatten();
        let (api, _) = endpoints
            .find(|(_, schemes)| schemes.iter().any(|s| s.matches(url.as_str())))
            .ok_or("no oEmbed provider")?;
        let api = Url::parse_with_params(api, [("format", "json"), ("url", url.as_str())])?;
        Ok(serde_json::from_slice(&Self::get(api.as_str()).await?.2)?)
    }

    /// Converts an image to a JPEG at most 1024px wide, which the embedding model reads.
    async fn jpeg(&self, mut image: Vec<u8>) -> Result<(String, Vec<u8>)> {
        let image = Response::new_with_opt_u8_array(Some(&mut image))?.body();
        let width = JSON::parse(r#"{"width":1024}"#)?;
        let format = JSON::parse(r#"{"format":"image/jpeg"}"#)?;
        let input = self.images.input(&image.ok_or("the image is empty")?)?;
        let output = input.transform(&width).output(&format)?;
        let output: Output = JsFuture::from(output).await?.unchecked_into();
        let jpeg = JsFuture::from(output.response().array_buffer()?).await?;
        Ok(("image/jpeg".into(), Uint8Array::new(&jpeg).to_vec()))
    }

    /// Fetches `url` as SatchelBot. Returns the final URL, content type, and body.
    async fn get(url: &str) -> Result<(String, String, Vec<u8>)> {
        let headers = Headers::new()?;
        headers.set("User-Agent", USER_AGENT)?;
        let init = RequestInit::new();
        init.set_headers(&headers);
        init.set_signal(Some(&AbortSignal::timeout_with_u32(TIMEOUT_MS)));
        let response: Response = JsFuture::from(global_fetch(url, &init)?)
            .await?
            .unchecked_into();
        let headers = response.headers();
        let length = headers.get("content-length")?;
        let length = length.and_then(|n| n.parse::<usize>().ok());
        if !response.ok() || length > Some(MAX_BYTES) {
            return Err(format!("{url} returned {} ({length:?} bytes)", response.status()).into());
        }
        let mime = headers.get("content-type")?.unwrap_or_default();
        let bytes = Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?);
        if bytes.length() as usize > MAX_BYTES {
            return Err(format!("{url} is over {MAX_BYTES} bytes").into());
        }
        Ok((response.url(), mime, bytes.to_vec()))
    }

    /// Appends a chunk to `buffer`, ending each text node with `\0`.
    fn collect(buffer: &RefCell<String>, chunk: &TextChunk) -> HandlerResult {
        let end = if chunk.last_in_text_node() { "\0" } else { "" };
        buffer.borrow_mut().extend([chunk.as_str(), end]);
        Ok(())
    }

    /// Appends every non-URL string in a JSON-LD block to `text`.
    fn strings(json: &Value, text: &mut String) {
        match json {
            Value::String(value) if !value.starts_with("http") && !value.starts_with('/') => {
                text.extend([" ", value]);
            }
            Value::Array(items) => items.iter().for_each(|item| Self::strings(item, text)),
            Value::Object(fields) => fields.values().for_each(|value| Self::strings(value, text)),
            _ => {}
        }
    }

    /// Unfolds a vCard's continuation lines and drops its `PHOTO`.
    fn vcard(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes)
            .replace("\r\n", "\n")
            .replace("\n ", "");
        let is_photo = |l: &&str| l.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("PHOTO"));
        text.lines()
            .filter(|l| !is_photo(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Decodes HTML entities twice, since some sites encode them twice.
    fn decode(text: &str) -> String {
        let once = decode_html_entities(text);
        let decoded = decode_html_entities(&once);
        decoded.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}
