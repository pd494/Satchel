use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use worker::wasm_bindgen::JsValue;
use worker::{console_error, Date, Fetch, Headers, Method, Request, RequestInit, Response};

const PHOTON_API: &str = "https://api.photon.codes";
type HmacSha256 = Hmac<Sha256>;

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A text to send into a space as the agent user.
pub struct OutboundMessage {
    pub sender_id: String,
    pub space_id: String,
    pub text: String,
}

/// Pulls the echo target out of a `message.received` webhook body.
///
/// Returns `None` for other events, non-text messages, and malformed JSON,
/// so the bot never echoes its own sends.
pub fn parse_inbound(body: &str) -> Option<OutboundMessage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    if payload["type"] != "message.received" {
        return None;
    }
    let message = &payload["data"]["object"];
    let field = |path: &str| Some(message.pointer(path)?.as_str()?.to_owned());
    Some(OutboundMessage {
        sender_id: field("/receivedBy/id")?,
        space_id: field("/space/id")?,
        text: field("/content/text")?,
    })
}

pub struct PhotonClient {
    project_id: String,
    project_secret: String,
    webhook_secret: String,
}

impl PhotonClient {
    pub fn new(project_id: String, project_secret: String, webhook_secret: String) -> Self {
        Self {
            project_id,
            project_secret,
            webhook_secret,
        }
    }

    async fn send(&self, message: OutboundMessage, idempotency_key: &str) -> worker::Result<()> {
        let request_body = serde_json::json!({
            "content": {
                "type": "text",
                "format": "plain",
                "text": message.text,
            },
            "senderId": message.sender_id,
            "spaceId": message.space_id,
        });

        self.post("messages", request_body, idempotency_key).await
    }

    async fn post(
        &self,
        path: &str,
        body: serde_json::Value,
        idempotency_key: &str,
    ) -> worker::Result<()> {
        let body = serde_json::to_string(&body)?;
        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;
        headers.set("Idempotency-Key", idempotency_key)?;
        headers.set("Authorization", &format!("Bearer {}", self.project_secret))?;
        let url = format!("{PHOTON_API}/v1/projects/{}/{}", self.project_id, path);
        let mut request = RequestInit::new();
        request
            .with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body)));
        let mut response = Fetch::Request(Request::new_with_init(&url, &request)?)
            .send()
            .await?;
        match response.status_code() {
            200..=299 => Ok(()),
            status => {
                let detail = response.text().await.unwrap_or_default();
                Err(worker::Error::RustError(format!(
                    "Photon rejected send: {status} {detail}"
                )))
            }
        }
    }

    /// Echoes each inbound Photon text back into the same conversation.
    ///
    /// 1. Read the `webhook-id`, `webhook-timestamp`, and `webhook-signature` headers.
    /// 2. Verify the raw body before parsing; reject with 401 on mismatch.
    /// 3. Parse the echo target with [`parse_inbound`]; ignore anything else.
    /// 4. Send the echo, using `webhook-id` as the idempotency key so retries don't duplicate.
    /// 5. Return 200 after a valid signature even if the send fails.
    ///    - A non-2xx makes Photon retry; send failures are logged instead.
    ///
    /// # Errors
    ///
    /// Fails only if the request body or headers can't be read.
    pub async fn handle_webhook(&self, mut req: Request) -> worker::Result<Response> {
        let header = |name| req.headers().get(name).map(Option::unwrap_or_default);
        let id = header("webhook-id")?;
        let timestamp = header("webhook-timestamp")?;
        let signature = header("webhook-signature")?;
        let body = req.text().await?;
        let now = Date::now().as_millis().cast_signed() / 1000;
        if !self.verify_webhook(&body, &id, &timestamp, &signature, now) {
            return Response::error("bad signature", 401);
        }
        let Some(message) = parse_inbound(&body) else {
            return Response::ok("ok");
        };
        if let Err(err) = self.send(message, &id).await {
            console_error!("echo send failed: {err}");
        }
        Response::ok("ok")
    }

    /// Checks Photon's signature so only Photon can trigger the bot (Standard Webhooks).
    /// `now` is the current Unix time in seconds.
    pub fn verify_webhook(
        &self,
        body: &str,
        id: &str,
        timestamp: &str,
        signature: &str,
        now: i64,
    ) -> bool {
        // `timestamp` is Unix seconds; anything over 5 minutes off is a replay.
        let Ok(ts) = timestamp.parse::<i64>() else {
            return false;
        };
        if (now - ts).abs() > 300 {
            return false;
        }
        // The secret is `whsec_` (a "webhook secret" label) + base64 of the key bytes.
        let Ok(key) = LENIENT.decode(self.webhook_secret.trim_start_matches("whsec_")) else {
            return false;
        };
        // HMAC-SHA256 is a keyed hash: same key + input gives the same 32 bytes,
        // and without the key no one can produce a match.
        let Ok(mut mac) = HmacSha256::new_from_slice(&key) else {
            return false;
        };
        // Signing id, timestamp, and the raw body together means none can be swapped.
        mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
        // base64 makes the bytes header-safe text; `v1,` names the scheme version.
        let expected = format!("v1,{}", BASE64.encode(mac.finalize().into_bytes()));
        // Several signatures arrive during secret rotation; compare in constant
        // time so response timing can't leak how many bytes matched.
        signature
            .split(' ')
            .any(|candidate| bool::from(candidate.as_bytes().ct_eq(expected.as_bytes())))
    }
}
