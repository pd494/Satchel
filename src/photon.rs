use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use worker::wasm_bindgen::JsValue;
use worker::{Date, Env, Fetch, Headers, Method, Request, RequestInit};

const PHOTON_API: &str = "https://api.photon.codes";
type HmacSha256 = Hmac<Sha256>;

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

pub struct OutboundMessage {
    pub space_id: String,
    pub text: String,
}

pub enum PhotonRequestOutcome {
    Text(InboundMessage),
    Unsupported,
    BadSignature,
    TooLarge,
}

pub struct InboundMessage {
    pub sender_id: String,
    pub space_id: String,
    pub text: String,
    /// The `webhook-id` header; Photon reuses it on retries.
    pub event_id: String,
}

impl InboundMessage {
    /// Pulls the sender, space, and text out of a `message.received` webhook body.
    ///
    /// Returns `None` for other events (including the bot's own `message.sent`),
    /// non-text messages, and malformed JSON.
    pub fn parse(body: &str, event_id: String) -> Option<Self> {
        let payload: serde_json::Value = serde_json::from_str(body).ok()?;
        if payload["type"] != "message.received" {
            return None;
        }
        let message = &payload["data"]["object"];
        let field = |path: &str| Some(message.pointer(path)?.as_str()?.to_owned());
        Some(Self {
            sender_id: field("/sentBy/id")?,
            space_id: field("/space/id")?,
            text: field("/content/text")?,
            event_id,
        })
    }
}

pub struct PhotonClient {
    project_id: String,
    project_secret: String,
    webhook_secret: String,
    /// The bot's Photon user id (`pho_usr_...`), not its phone number or email.
    bot_id: String,
}

impl PhotonClient {
    pub fn new(
        project_id: String,
        project_secret: String,
        webhook_secret: String,
        bot_id: String,
    ) -> Self {
        Self {
            project_id,
            project_secret,
            webhook_secret,
            bot_id,
        }
    }

    /// Builds a client from the Worker's Photon secrets.
    pub fn from_env(env: &Env) -> worker::Result<Self> {
        Ok(Self::new(
            env.secret("PROJECT_ID")?.to_string(),
            env.secret("PROJECT_SECRET")?.to_string(),
            env.secret("SPECTRUM_WEBHOOK_SECRET")?.to_string(),
            env.secret("PHOTON_BOT_ID")?.to_string(),
        ))
    }

    /// Sends `message` into its space, as the bot.
    pub async fn send(
        &self,
        message: &OutboundMessage,
        idempotency_key: &str,
    ) -> worker::Result<()> {
        let request_body = serde_json::json!({
            "content": {
                "type": "text",
                "format": "plain",
                "text": message.text,
            },
            "senderId": self.bot_id,
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

    /// Verifies and parses a webhook.
    pub async fn receive(&self, mut req: Request) -> worker::Result<PhotonRequestOutcome> {
        let header = |name| req.headers().get(name).map(Option::unwrap_or_default);
        let id = header("webhook-id")?;
        let timestamp = header("webhook-timestamp")?;
        let signature = header("webhook-signature")?;
        if signature.is_empty() {
            return Ok(PhotonRequestOutcome::BadSignature);
        }
        // Refuse oversized bodies before buffering them.
        let length = header("content-length")?.parse::<u64>().unwrap_or(0);
        if length > 1_000_000 {
            return Ok(PhotonRequestOutcome::TooLarge);
        }
        let body = req.text().await?;
        let now = Date::now().as_millis().cast_signed() / 1000;
        if !self.verify_webhook(&body, &id, &timestamp, &signature, now) {
            return Ok(PhotonRequestOutcome::BadSignature);
        }
        Ok(match InboundMessage::parse(&body, id) {
            Some(message) => PhotonRequestOutcome::Text(message),
            None => PhotonRequestOutcome::Unsupported,
        })
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
        if now.abs_diff(ts) > 300 {
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
