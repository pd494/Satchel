use serde_json::{json, Value};
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

mod webhook;
pub use webhook::{LinqRequestOutcome, LinqWebhook};

const API: &str = "https://api.linqapp.com/api/partner/v3";

pub struct LinqClient {
    api_key: String,
}

pub struct OutboundMessage {
    pub space_id: String,
    pub text: String,
    pub links: Vec<String>,
    pub idempotency_key: String,
}

impl LinqClient {
    pub fn from_env(env: &Env) -> Result<Self> {
        let api_key = env.secret("LINQ_API_KEY")?.to_string();
        Ok(Self { api_key })
    }

    /// Linq requires a link part to be the only part. Linq drops a repeated `idempotency_key`.
    pub async fn send(&self, message: &OutboundMessage) -> Result<()> {
        let path = format!("chats/{}/messages", message.space_id);
        let text =
            (!message.text.is_empty()).then(|| json!({ "type": "text", "value": message.text }));
        let links = message
            .links
            .iter()
            .map(|link| json!({ "type": "link", "value": link }));
        for (i, part) in text.into_iter().chain(links).enumerate() {
            let key = format!("{}:{i}", message.idempotency_key);
            let body = json!({ "message": { "parts": [part], "idempotency_key": key } });
            self.post(&path, Some(body)).await?;
        }
        Ok(())
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
