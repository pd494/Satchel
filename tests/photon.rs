use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use satchel::photon::{InboundMessage, PhotonClient};
use sha2::Sha256;

const KEY: &[u8] = b"test-signing-key";
const NOW: i64 = 1_760_000_000;

fn client(webhook_secret: String) -> PhotonClient {
    PhotonClient::new(String::new(), String::new(), webhook_secret, String::new())
}

fn sign(id: &str, timestamp: &str, body: &str) -> Option<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(KEY).ok()?;
    mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
    Some(format!(
        "v1,{}",
        STANDARD.encode(mac.finalize().into_bytes())
    ))
}

#[test]
fn verifies_photon_signatures() {
    let padded = client(format!("whsec_{}", STANDARD.encode(KEY)));
    let unpadded = client(format!(
        "whsec_{}",
        STANDARD.encode(KEY).trim_end_matches('=')
    ));
    let ts = NOW.to_string();
    let sig = sign("msg_1", &ts, "{}").unwrap();

    assert!(padded.verify_webhook("{}", "msg_1", &ts, &sig, NOW));
    assert!(unpadded.verify_webhook("{}", "msg_1", &ts, &sig, NOW));
    assert!(padded.verify_webhook("{}", "msg_1", &ts, &format!("v1,old {sig}"), NOW));

    assert!(
        !padded.verify_webhook("{\"x\":1}", "msg_1", &ts, &sig, NOW),
        "tampered body"
    );
    assert!(
        !padded.verify_webhook("{}", "msg_2", &ts, &sig, NOW),
        "swapped id"
    );
    assert!(
        !padded.verify_webhook("{}", "msg_1", &ts, &sig, NOW + 301),
        "stale"
    );
    assert!(
        !padded.verify_webhook("{}", "msg_1", &ts, &sig, NOW - 301),
        "future"
    );
    assert!(
        !padded.verify_webhook("{}", "msg_1", "nope", &sig, NOW),
        "bad timestamp"
    );
    let wrong = client(format!("whsec_{}", STANDARD.encode(b"other-key")));
    assert!(
        !wrong.verify_webhook("{}", "msg_1", &ts, &sig, NOW),
        "wrong secret"
    );
}

#[test]
fn parses_only_received_texts() {
    let event = |kind: &str, content: &str| {
        format!(
            r#"{{"type":"{kind}","data":{{"object":{{"content":{content},
            "sentBy":{{"id":"pho_usr_sender","handle":"+15551234567"}},"receivedBy":{{"id":"pho_usr_bot"}},"space":{{"id":"pho_spc_chat"}}}}}}}}"#
        )
    };
    let text = r#"{"type":"text","text":"hi"}"#;

    let message = InboundMessage::parse(&event("message.received", text), String::new()).unwrap();
    assert_eq!(message.sender_id, "pho_usr_sender");
    assert_eq!(message.space_id, "pho_spc_chat");
    assert_eq!(message.text, "hi");

    assert!(InboundMessage::parse(&event("message.sent", text), String::new()).is_none());
    assert!(InboundMessage::parse(
        &event("message.received", r#"{"type":"image"}"#),
        String::new()
    )
    .is_none());
    assert!(InboundMessage::parse("not json", String::new()).is_none());
}
