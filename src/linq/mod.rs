use serde_json::{json, Value};
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

use crate::media::Link;

mod webhook;
pub use webhook::{LinqRequestOutcome, LinqWebhook};

const API: &str = "https://api.linqapp.com/api/partner/v3";

pub struct LinqClient {
    api_key: String,
}

impl LinqClient {
    pub fn from_env(env: &Env) -> Result<Self> {
        let api_key = env.secret("LINQ_API_KEY")?.to_string();
        Ok(Self { api_key })
    }

    /// Sends `text` into the chat `space_id`, then each link in it as its own message.
    /// Linq requires a link part to be the only part. Linq drops a repeated `idempotency_key`.
    pub async fn send(&self, space_id: &str, text: &str, idempotency_key: &str) -> Result<()> {
        let links = Link::find_all(text);
        let rest = links
            .iter()
            .fold(text.to_owned(), |rest, link| rest.replace(&link.typed, ""));
        if !rest.trim().is_empty() {
            let part = json!({ "type": "text", "value": rest.trim() });
            self.send_part(space_id, part, idempotency_key).await?;
        }
        for (i, link) in links.iter().enumerate() {
            let part = json!({ "type": "link", "value": link.url.as_str() });
            let key = format!("{idempotency_key}:link.{i}");
            self.send_part(space_id, part, &key).await?;
        }
        Ok(())
    }

    async fn send_part(&self, space_id: &str, part: Value, idempotency_key: &str) -> Result<()> {
        let body = json!({ "message": { "parts": [part], "idempotency_key": idempotency_key } });
        self.post(&format!("chats/{space_id}/messages"), Some(body))
            .await
    }

    pub async fn like(&self, message_id: &str) -> Result<()> {
        let body = json!({ "operation": "add", "type": "like" });
        self.post(&format!("messages/{message_id}/reactions"), Some(body))
            .await
    }

    pub async fn mark_read(&self, space_id: &str) -> Result<()> {
        self.post(&format!("chats/{space_id}/read"), None).await
    }

    async fn post(&self, path: &str, body: Option<Value>) -> Result<()> {
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", self.api_key))?;
        headers.set("Content-Type", "application/json")?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(body.map(|body| body.to_string().into()));
        let url = format!("{API}/{path}");
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
