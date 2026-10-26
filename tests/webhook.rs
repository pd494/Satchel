//! Drives the real Worker and User DO (`wrangler dev`) with signed webhooks, against a fake Photon.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering::SeqCst};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const KEY: &[u8] = b"test-signing-key";
const BOT: &str = "pho_usr_bot";
const NEW: &str = "Hello new user, how can I help you today?";
const EXISTING: &str = "Hello existing user, how can I help you today?";
const RESET: &str = "Reset done. Text again to start as a new user.";

/// Kills `wrangler dev` (and the workerd it spawns) and deletes its state when the test ends.
struct Worker(Child, std::path::PathBuf);

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", self.0.id())])
            .status();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn event(kind: &str, sender: &str, text: &str) -> String {
    serde_json::json!({ "type": kind, "data": { "object": {
        "content": { "type": "text", "text": text },
        "sentBy": { "id": sender },
        "space": { "id": "pho_spc_chat" },
    }}})
    .to_string()
}

#[test]
fn webhook() {
    // Fake Photon: hands each sent body to the test, answers with `status`.
    let photon = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let photon_port = photon.server_addr().to_ip().unwrap().port();
    let status = Arc::new(AtomicU16::new(200));
    let (tx, sends) = mpsc::channel::<serde_json::Value>();
    let reply_status = status.clone();
    std::thread::spawn(move || {
        for mut req in photon.incoming_requests() {
            tx.send(serde_json::from_reader(req.as_reader()).unwrap())
                .unwrap();
            let response = tiny_http::Response::empty(reply_status.load(SeqCst));
            req.respond(response).unwrap();
        }
    });

    // Throwaway secrets and fresh DO storage, so the real `.env` and state stay out.
    let dir = std::env::temp_dir().join(format!("satchel-webhook-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let env_file = dir.join("test.env");
    let vars = format!(
        "PROJECT_ID=p\nPROJECT_SECRET=s\nSPECTRUM_WEBHOOK_SECRET=whsec_{}\nPHOTON_BOT_ID={BOT}\n",
        STANDARD.encode(KEY)
    );
    std::fs::write(&env_file, vars).unwrap();
    let port = free_port();
    let _worker = Worker(
        Command::new("npx")
            .args([
                "--yes",
                "wrangler",
                "dev",
                "--ip",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .arg("--persist-to")
            .arg(dir.join("state"))
            .arg("--env-file")
            .arg(&env_file)
            .args([
                "--var",
                &format!("PHOTON_API:http://127.0.0.1:{photon_port}"),
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap(),
        dir.clone(),
    );

    let http: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(600);
    while http
        .get(&url)
        .call()
        .map_or(true, |res| res.status() != 200)
    {
        assert!(Instant::now() < deadline, "wrangler dev never came up");
        std::thread::sleep(Duration::from_millis(200));
    }

    // POSTs a webhook signed the way Photon signs it; returns the HTTP status.
    let post = |id: &str, body: &str, signature: Option<&str>| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(format!("{id}.{ts}.{body}").as_bytes());
        let signed = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
        http.post(format!("{url}/webhook"))
            .header("webhook-id", id)
            .header("webhook-timestamp", &ts)
            .header("webhook-signature", signature.unwrap_or(&signed))
            .send(body)
            .unwrap()
            .status()
            .as_u16()
    };
    // The one body sent to Photon since the last check, or None.
    let sent = || {
        let body = sends.try_recv().ok()?;
        assert!(sends.try_recv().is_err(), "more than one send");
        Some(body)
    };
    let text = |body: Option<serde_json::Value>| body.expect("no send")["content"]["text"].clone();
    let hi = event("message.received", "pho_usr_alice", "hi");
    let reset = event("message.received", "pho_usr_alice", "reset");

    // Only Photon can trigger the bot, and the bot ignores its own sends.
    assert_eq!(post("evt_forged", &hi, Some("v1,forged")), 401);
    assert_eq!(
        post("evt_echo", &event("message.sent", BOT, "hi"), None),
        200
    );
    assert!(sent().is_none());

    // Replies go out as the bot, into the sender's space, keyed by the event.
    assert_eq!(post("evt_1", &hi, None), 200);
    let first = sent().expect("no send");
    assert_eq!(first["content"]["text"], NEW);
    assert_eq!(first["senderId"], BOT);
    assert_eq!(first["spaceId"], "pho_spc_chat");

    // A retried event replays its reply instead of running again.
    assert_eq!(post("evt_1", &hi, None), 200);
    assert_eq!(text(sent()), NEW);
    assert_eq!(post("evt_2", &hi, None), 200);
    assert_eq!(text(sent()), EXISTING);

    // A failed send fails the webhook so Photon retries; the retry then delivers, and reset took effect.
    status.store(500, SeqCst);
    assert_eq!(post("evt_3", &reset, None), 500);
    assert_eq!(text(sent()), RESET);
    status.store(200, SeqCst);
    assert_eq!(post("evt_3", &reset, None), 200);
    assert_eq!(text(sent()), RESET);
    assert_eq!(post("evt_4", &hi, None), 200);
    assert_eq!(text(sent()), NEW);

    // Each sender has their own User DO.
    assert_eq!(
        post(
            "evt_5",
            &event("message.received", "pho_usr_bob", "hi"),
            None
        ),
        200
    );
    assert_eq!(text(sent()), NEW);
}
