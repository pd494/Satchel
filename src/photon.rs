use std::cell::RefCell;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use prost::encoding::{bool as pbool, bytes, int32, string};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use worker::js_sys::Uint8Array;
use worker::{Date, Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

const CLOUD_API: &str = "https://spectrum.photon.codes";
const IMESSAGE_API: &str = "https://imessage.spectrum.photon.codes/photon.imessage.v1";
type HmacSha256 = Hmac<Sha256>;

pub enum PhotonRequestOutcome {
    Text(InboundMessage),
    Unsupported,
    BadSignature,
    TooLarge,
}

pub struct InboundMessage {
    pub sender_id: String,
    /// The iMessage chat, like `any;-;+15551234567`.
    pub space_id: String,
    pub text: String,
    /// Photon's message id: the dedupe key across retries, and what a tapback points at.
    pub event_id: String,
}

/// Talks to a Photon project's iMessage line. Sends go over gRPC-web, which Workers' `fetch` can carry.
pub struct PhotonClient {
    project_id: String,
    project_secret: String,
    signing_secret: String,
    /// The line token, and the Unix milliseconds at which to renew it.
    token: RefCell<Option<(String, u64)>>,
}

impl PhotonClient {
    pub fn from_env(env: &Env) -> Result<Self> {
        Ok(Self {
            project_id: env.secret("PHOTON_PROJECT_ID")?.to_string(),
            project_secret: env.secret("PHOTON_PROJECT_SECRET")?.to_string(),
            signing_secret: env.secret("PHOTON_SIGNING_SECRET")?.to_string(),
            token: RefCell::new(None),
        })
    }

    /// Verifies and parses a webhook.
    pub async fn receive(&self, mut req: Request) -> Result<PhotonRequestOutcome> {
        let header = |name| req.headers().get(name).map(Option::unwrap_or_default);
        let timestamp = header("x-spectrum-timestamp")?;
        let signature = header("x-spectrum-signature")?;
        // Refuse oversized bodies before buffering them.
        let length = header("content-length")?.parse::<u64>().unwrap_or(0);
        if length > 1_000_000 {
            return Ok(PhotonRequestOutcome::TooLarge);
        }
        let body = req.text().await?;
        let now = Date::now().as_millis().cast_signed() / 1000;
        if !self.verify_webhook(&body, &timestamp, &signature, now) {
            return Ok(PhotonRequestOutcome::BadSignature);
        }
        Ok(match parse(&body) {
            Some(message) => PhotonRequestOutcome::Text(message),
            None => PhotonRequestOutcome::Unsupported,
        })
    }

    /// Checks that only Photon could have sent the webhook. `now` is Unix seconds.
    fn verify_webhook(&self, body: &str, timestamp: &str, signature: &str, now: i64) -> bool {
        // Anything over 5 minutes off is a replay.
        let Ok(ts) = timestamp.parse::<i64>() else {
            return false;
        };
        if now.abs_diff(ts) > 300 {
            return false;
        }
        let Ok(mut mac) = HmacSha256::new_from_slice(self.signing_secret.as_bytes()) else {
            return false;
        };
        mac.update(format!("v0:{timestamp}:{body}").as_bytes());
        let expected = format!("v0={:x}", mac.finalize().into_bytes());
        // Constant time, so response timing can't leak how many bytes matched.
        bool::from(signature.as_bytes().ct_eq(expected.as_bytes()))
    }

    /// Sends `text` into `space_id`. iMessage renders a preview card for a link in it.
    pub async fn send(&self, space_id: &str, text: &str, idempotency_key: &str) -> Result<()> {
        // SendTextMessageRequest { chat_guid = 1, text = 2, enable_link_preview = 7 }
        let mut request = Vec::new();
        string::encode(1, &space_id.to_owned(), &mut request);
        string::encode(2, &text.to_owned(), &mut request);
        pbool::encode(7, &true, &mut request);
        self.call("MessageService/SendTextMessage", request, idempotency_key)
            .await
    }

    /// Puts a 👍 tapback on the message `message_id`.
    pub async fn like(
        &self,
        space_id: &str,
        message_id: &str,
        idempotency_key: &str,
    ) -> Result<()> {
        // SetReactionRequest { target = 1 { chat_guid = 1, message_guid = 2 }, reaction = 2 { kind = 1 (LIKE) }, is_set = 3 }
        let mut target = Vec::new();
        string::encode(1, &space_id.to_owned(), &mut target);
        string::encode(2, &message_id.to_owned(), &mut target);
        let mut reaction = Vec::new();
        int32::encode(1, &1, &mut reaction);
        let mut request = Vec::new();
        bytes::encode(1, &target, &mut request);
        bytes::encode(2, &reaction, &mut request);
        pbool::encode(3, &true, &mut request);
        self.call("MessageService/SetReaction", request, idempotency_key)
            .await
    }

    /// Marks everything in `space_id` as read.
    pub async fn mark_read(&self, space_id: &str, idempotency_key: &str) -> Result<()> {
        // MarkChatReadRequest { chat_guid = 1 }
        let mut request = Vec::new();
        string::encode(1, &space_id.to_owned(), &mut request);
        self.call("ChatService/MarkChatRead", request, idempotency_key)
            .await
    }

    /// Makes one unary gRPC-web call with the protobuf bytes `message`. Only the reply's status matters.
    async fn call(&self, method: &str, message: Vec<u8>, idempotency_key: &str) -> Result<()> {
        // A gRPC-web frame is a flag byte, a 4-byte big-endian length, then the protobuf bytes.
        let length = u32::try_from(message.len()).map_err(|e| Error::RustError(e.to_string()))?;
        let mut frame = vec![0];
        frame.extend(length.to_be_bytes());
        frame.extend(message);

        let headers = Headers::new();
        headers.set("Content-Type", "application/grpc-web+proto")?;
        headers.set("X-Grpc-Web", "1")?;
        headers.set("X-Idempotency-Key", idempotency_key)?;
        headers.set("Authorization", &format!("Bearer {}", self.token().await?))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(Uint8Array::from(frame.as_slice()).into()));
        let url = format!("{IMESSAGE_API}.{method}");
        let mut response = Fetch::Request(Request::new_with_init(&url, &init)?)
            .send()
            .await?;

        // A failed call puts its status in the headers; a successful one in a trailer frame.
        let headers = response.headers();
        let trailers = match headers.get("grpc-status")? {
            Some(status) => format!(
                "grpc-status:{status}\r\ngrpc-message:{}",
                headers.get("grpc-message")?.unwrap_or_default()
            ),
            None => trailer_frame(&response.bytes().await?).unwrap_or_default(),
        };
        let field = |name: &str| {
            trailers
                .lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
                .map(str::trim)
                .unwrap_or_default()
        };
        match (response.status_code(), field("grpc-status")) {
            (200, "0") => Ok(()),
            (http, grpc) => Err(Error::RustError(format!(
                "Photon {method} failed: http {http}, grpc-status {grpc:?} {}",
                field("grpc-message")
            ))),
        }
    }

    /// A line token, renewed shortly before its 15 minutes run out.
    async fn token(&self) -> Result<String> {
        let now = Date::now().as_millis();
        if let Some((token, _)) = self
            .token
            .borrow()
            .as_ref()
            .filter(|(_, renew_at)| *renew_at > now)
        {
            return Ok(token.clone());
        }
        let credentials = BASE64.encode(format!("{}:{}", self.project_id, self.project_secret));
        let headers = Headers::new();
        headers.set("Authorization", &format!("Basic {credentials}"))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post).with_headers(headers);
        let url = format!("{CLOUD_API}/projects/{}/imessage/tokens", self.project_id);
        let mut response = Fetch::Request(Request::new_with_init(&url, &init)?)
            .send()
            .await?;
        if response.status_code() != 200 {
            return Err(Error::RustError(format!(
                "Photon token request failed: {}",
                response.status_code()
            )));
        }
        let json: serde_json::Value = response.json().await?;
        let token = json["data"]["token"]
            .as_str()
            .ok_or_else(|| Error::RustError("Photon token missing".into()))?
            .to_owned();
        let lifetime = json["data"]["expiresIn"].as_u64().unwrap_or(0);
        let renew_at = now + lifetime.saturating_sub(60) * 1000;
        *self.token.borrow_mut() = Some((token.clone(), renew_at));
        Ok(token)
    }
}

/// Pulls a direct text message out of a webhook body. Returns `None` for anything else,
/// including group chats, reactions, and attachments.
fn parse(body: &str) -> Option<InboundMessage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = &payload["message"];
    if payload["event"] != "messages"
        || message["space"]["type"] != "dm"
        || message["content"]["type"] != "text"
    {
        return None;
    }
    let field = |path: &str| Some(message.pointer(path)?.as_str()?.to_owned());
    Some(InboundMessage {
        sender_id: field("/sender/id")?,
        space_id: field("/space/id")?,
        text: field("/content/text")?,
        event_id: field("/id")?,
    })
}

/// Returns the text of the trailer frame (flag `0x80`) in a gRPC-web body.
fn trailer_frame(body: &[u8]) -> Option<String> {
    let mut rest = body;
    while let [flag, a, b, c, d, tail @ ..] = rest {
        let length = u32::from_be_bytes([*a, *b, *c, *d]) as usize;
        let frame = tail.get(..length)?;
        if flag & 0x80 != 0 {
            return Some(String::from_utf8_lossy(frame).into_owned());
        }
        rest = &tail[length..];
    }
    None
}
