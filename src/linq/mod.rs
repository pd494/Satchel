use crate::tools::Tapback;
use serde_json::{json, Value};
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

mod webhook;
pub use webhook::{LinqRequestOutcome, LinqWebhook};

const API: &str = "https://api.linqapp.com/api/partner/v3";

pub struct LinqClient {
    api: String,
    api_key: String,
}

pub enum Part {
    Text(String),
    Link(String),
    Media(String),
    Voice(String),
}

impl LinqClient {
    /// `LINQ_API` overrides the Linq URL. The end-to-end tests point it at a fake Linq.
    pub fn from_env(env: &Env) -> Result<Self> {
        let api = match env.var("LINQ_API") {
            Ok(api) => api.to_string(),
            Err(_) => API.to_string(),
        };
        let api_key = env.secret("LINQ_API_KEY")?.to_string();
        Ok(Self { api, api_key })
    }

    /// Linq drops a repeated `idempotency_key`. A voice memo goes to its own endpoint.
    pub async fn send(&self, space_id: &str, idempotency_key: &str, part: Part) -> Result<()> {
        let part = match part {
            Part::Text(text) => json!({ "type": "text", "value": text }),
            Part::Link(url) => json!({ "type": "link", "value": url }),
            Part::Media(id) => json!({ "type": "media", "attachment_id": id }),
            Part::Voice(id) => {
                let body = json!({ "attachment_id": id });
                return self
                    .post(&format!("chats/{space_id}/voicememo"), Some(body))
                    .await;
            }
        };
        let body = json!({ "message": { "parts": [part], "idempotency_key": idempotency_key } });
        self.post(&format!("chats/{space_id}/messages"), Some(body))
            .await
    }

    pub async fn react(&self, message_id: &str, tapback: Tapback) -> Result<()> {
        let kind = match tapback {
            Tapback::Like => "like",
            Tapback::Love => "love",
            Tapback::Dislike => "dislike",
            Tapback::Laugh => "laugh",
            Tapback::Emphasize => "emphasize",
            Tapback::Question => "question",
        };
        let body = json!({ "operation": "add", "type": kind });
        self.post(&format!("messages/{message_id}/reactions"), Some(body))
            .await
    }

    pub async fn mark_read(&self, space_id: &str) -> Result<()> {
        self.post(&format!("chats/{space_id}/read"), None).await
    }

    /// Shows or hides the typing indicator. Sending a message also hides it.
    pub async fn typing(&self, space_id: &str, on: bool) -> Result<()> {
        let method = if on { Method::Post } else { Method::Delete };
        self.request(method, &format!("chats/{space_id}/typing"), None)
            .await
    }

    async fn post(&self, path: &str, body: Option<Value>) -> Result<()> {
        self.request(Method::Post, path, body).await
    }

    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<()> {
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", self.api_key))?;
        headers.set("Content-Type", "application/json")?;
        let mut init = RequestInit::new();
        init.with_method(method)
            .with_headers(headers)
            .with_body(body.map(|body| body.to_string().into()));
        let url = format!("{}/{path}", self.api);
        let mut response = Fetch::Request(Request::new_with_init(&url, &init)?)
            .send()
            .await?;
        let status = response.status_code();
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(Error::RustError(format!(
            "Linq {path} failed: http {status} {}",
            response.text().await.unwrap_or_default()
        )))
    }
}
