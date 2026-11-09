use base64::{engine::general_purpose::STANDARD_PAD_INDIFFERENT as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use worker::{Date, Env, Error, Request, Result};

const MAX_BODY_BYTES: u64 = 1_000_000;
const TIMESTAMP_TOLERANCE_SECS: u64 = 300;

pub enum LinqRequestOutcome {
    Text(InboundMessage),
    Unsupported,
    BadSignature,
    TooLarge,
}

pub struct InboundMessage {
    pub sender_id: String,
    pub space_id: String,
    pub text: String,
    pub event_id: String,
}

pub struct LinqWebhook {
    key: Vec<u8>,
}

impl LinqWebhook {
    pub fn from_env(env: &Env) -> Result<Self> {
        let secret = env.secret("LINQ_WEBHOOK_SECRET")?.to_string();
        let key = BASE64
            .decode(secret.trim_start_matches("whsec_"))
            .map_err(|error| Error::RustError(format!("LINQ_WEBHOOK_SECRET: {error}")))?;
        Ok(Self { key })
    }

    pub async fn receive(&self, mut req: Request) -> Result<LinqRequestOutcome> {
        let header = |name| req.headers().get(name).map(Option::unwrap_or_default);
        let id = header("webhook-id")?;
        let timestamp = header("webhook-timestamp")?;
        let signature = header("webhook-signature")?;
        if id.is_empty() || timestamp.is_empty() || signature.is_empty() {
            return Ok(LinqRequestOutcome::BadSignature);
        }
        let length = header("content-length")?.parse::<u64>().unwrap_or(0);
        if length > MAX_BODY_BYTES {
            return Ok(LinqRequestOutcome::TooLarge);
        }
        let body = req.text().await?;
        if self.verify(&id, &timestamp, &body, &signature).is_none() {
            return Ok(LinqRequestOutcome::BadSignature);
        }
        Ok(match parse(&body) {
            Some(message) => LinqRequestOutcome::Text(message),
            None => LinqRequestOutcome::Unsupported,
        })
    }

    /// Checks a Standard Webhooks signature.
    fn verify(&self, id: &str, timestamp: &str, body: &str, signature: &str) -> Option<()> {
        let now = Date::now().as_millis().cast_signed() / 1000;
        if now.abs_diff(timestamp.parse().ok()?) > TIMESTAMP_TOLERANCE_SECS {
            return None;
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).ok()?;
        mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
        let expected = BASE64.encode(mac.finalize().into_bytes());
        signature
            .split(' ')
            .filter_map(|sig| sig.strip_prefix("v1,"))
            .any(|sig| bool::from(sig.as_bytes().ct_eq(expected.as_bytes())))
            .then_some(())
    }
}

fn parse(body: &str) -> Option<InboundMessage> {
    let payload: Value = serde_json::from_str(body).ok()?;
    let data = &payload["data"];
    if payload["event_type"] != "message.received" || data["chat"]["is_group"] != false {
        return None;
    }
    let text = data["parts"]
        .as_array()?
        .iter()
        .filter(|part| part["type"] == "text" || part["type"] == "link")
        .filter_map(|part| part["value"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return None;
    }
    let field = |path: &str| Some(data.pointer(path)?.as_str()?.to_owned());
    Some(InboundMessage {
        sender_id: field("/sender_handle/handle")?,
        space_id: field("/chat/id")?,
        text,
        event_id: field("/id")?,
    })
}
