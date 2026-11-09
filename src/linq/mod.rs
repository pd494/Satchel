use serde_json::{json, Value};
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

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

    /// Sends `text` into the chat `space_id`. Linq drops a repeated `idempotency_key`.
    pub async fn send(&self, space_id: &str, text: &str, idempotency_key: &str) -> Result<()> {
        let body = json!({
            "message": {
                "parts": [{ "type": "text", "value": text }],
                "idempotency_key": idempotency_key,
            }
        });
        self.post(&format!("chats/{space_id}/messages"), Some(body))
            .await
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
