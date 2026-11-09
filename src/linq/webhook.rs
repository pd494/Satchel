use crate::media::{Attachment, MediaKind};
use base64::{engine::general_purpose::STANDARD_PAD_INDIFFERENT as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use worker::{Date, Env, Error, Request, Result};

const MAX_BODY_BYTES: u64 = 1_000_000;
const TIMESTAMP_TOLERANCE_SECS: u64 = 300;

pub enum LinqRequestOutcome {
    Message(InboundMessage),
    Unsupported,
    BadSignature,
    TooLarge,
}

#[derive(Serialize, Deserialize)]
pub struct InboundMessage {
    pub sender_id: String,
    pub space_id: String,
    pub text: String,
    pub media: Vec<Attachment>,
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
            Some(message) => LinqRequestOutcome::Message(message),
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
    let parts = data["parts"].as_array()?;
    let text = parts
        .iter()
        .filter(|part| part["type"] == "text" || part["type"] == "link")
        .filter_map(|part| part["value"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let media: Vec<Attachment> = parts
        .iter()
        .filter(|part| part["type"] == "media")
        .filter_map(attachment)
        .collect();
    if text.is_empty() && media.is_empty() {
        return None;
    }
    let field = |path: &str| Some(data.pointer(path)?.as_str()?.to_owned());
    Some(InboundMessage {
        sender_id: field("/sender_handle/handle")?,
        space_id: field("/chat/id")?,
        text,
        media,
        event_id: field("/id")?,
    })
}

fn attachment(part: &Value) -> Option<Attachment> {
    let field = |name: &str| Some(part[name].as_str()?.to_owned());
    Some(Attachment {
        kind: MediaKind::from_mime(part["mime_type"].as_str()?)?,
        id: field("id")?,
        filename: field("filename"),
        url: field("url")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(parts: Value) -> String {
        json!({
            "event_type": "message.received",
            "data": {
                "id": "evt_1",
                "chat": { "id": "chat_1", "is_group": false },
                "sender_handle": { "handle": "+15550001111" },
                "parts": parts,
            },
        })
        .to_string()
    }

    fn media(id: &str, mime: &str) -> Value {
        json!({
            "type": "media",
            "id": id,
            "filename": "photo.jpg",
            "mime_type": mime,
            "size_bytes": 245678,
            "url": "https://cdn.linqapp.com/x",
        })
    }

    #[test]
    fn parses_media_only_message() {
        let message = parse(&body(json!([media("att_1", "image/jpeg")]))).unwrap();
        assert_eq!(message.text, "");
        assert_eq!(message.media.len(), 1);
        let photo = &message.media[0];
        assert!(matches!(photo.kind, MediaKind::Image));
        assert_eq!(photo.id, "att_1");
        assert_eq!(photo.filename.as_deref(), Some("photo.jpg"));
        assert_eq!(photo.url, "https://cdn.linqapp.com/x");
    }

    #[test]
    fn parses_text_with_media() {
        let parts = json!([
            { "type": "text", "value": "our trip" },
            media("att_1", "image/png"),
            media("att_2", "video/mp4"),
        ]);
        let message = parse(&body(parts)).unwrap();
        assert_eq!(message.text, "our trip");
        let kinds: Vec<_> = message.media.iter().map(|m| m.kind).collect();
        assert!(matches!(kinds[..], [MediaKind::Image, MediaKind::Video]));
    }

    #[test]
    fn drops_unsupported_media() {
        assert!(parse(&body(json!([media("att_1", "application/pdf")]))).is_none());

        let parts = json!([{ "type": "text", "value": "notes" }, media("att_1", "audio/m4a")]);
        let message = parse(&body(parts)).unwrap();
        assert_eq!(message.text, "notes");
        assert!(message.media.is_empty());
    }
}
