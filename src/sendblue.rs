use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use worker::{console_error, Fetch, Headers, Method, Request, RequestInit, Response};

const SEND_MESSAGE_URL: &str = "https://api.sendblue.com/api/send-message";

/// Sendblue API credentials and the number Satchel texts from.
pub struct SendblueClient {
    pub api_key_id: String,
    pub api_secret_key: String,
    pub from_number: String,
}

#[derive(Deserialize)]
struct Inbound {
    from_number: String,
    content: String,
    #[serde(default)]
    is_outbound: bool,
}

#[derive(Serialize)]
struct SendMessage<'a> {
    number: &'a str,
    from_number: &'a str,
    content: &'a str,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("could not encode the message: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("could not reach Sendblue: {0}")]
    Network(#[from] worker::Error),
    #[error("Sendblue rejected the message with status {0}")]
    Rejected(u16),
}

impl SendblueClient {
    /// Texts `content` to the phone number `to`.
    pub async fn send(&self, to: &str, content: &str) -> Result<(), SendError> {
        let body = serde_json::to_string(&SendMessage {
            number: to,
            from_number: &self.from_number,
            content,
        })?;

        let headers = Headers::new();
        headers.set("sb-api-key-id", &self.api_key_id)?;
        headers.set("sb-api-secret-key", &self.api_secret_key)?;
        headers.set("content-type", "application/json")?;

        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body)));

        let request = Request::new_with_init(SEND_MESSAGE_URL, &init)?;
        let response = Fetch::Request(request).send().await?;
        match response.status_code() {
            200..=299 => Ok(()),
            status => Err(SendError::Rejected(status)),
        }
    }
}

/// Echoes each inbound text back to its sender.
pub async fn handle_webhook(mut req: Request, client: &SendblueClient) -> worker::Result<Response> {
    let Ok(body) = req.text().await else {
        return Response::error("Failed to read body", 400);
    };
    let Ok(msg) = serde_json::from_str::<Inbound>(&body) else {
        return Response::error("invalid payload", 400);
    };
    // Our own replies can come back through the webhook; echoing them would loop.
    if msg.is_outbound {
        return Response::ok("ignored outbound");
    }
    // Still 200 on send failure so Sendblue doesn't retry and double-text.
    if let Err(err) = client.send(&msg.from_number, &msg.content).await {
        console_error!("echo send failed: {err}");
    }
    Response::ok("Webhook received")
}
