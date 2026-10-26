pub mod photon;
pub mod user;

use photon::{OutboundMessage, PhotonClient, PhotonRequestOutcome};
use worker::{console_error, event, Context, Env, Method, Request, Response, Result};

/// Worker entry point and HTTP routing.
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match (req.method(), req.path().as_str()) {
        (Method::Get, "/") => Response::ok("hello world"),
        (Method::Post, "/webhook") => recv(req, env).await,
        _ => Response::error("not found", 404),
    }
}

async fn recv(req: Request, env: Env) -> Result<Response> {
    let client = PhotonClient::from_env(&env)?;
    let message = match client.receive(req).await? {
        PhotonRequestOutcome::Text(message) => message,
        PhotonRequestOutcome::Unsupported => return Response::ok("ok"),
        PhotonRequestOutcome::BadSignature => return Response::error("bad signature", 401),
        PhotonRequestOutcome::TooLarge => return Response::error("payload too large", 413),
    };

    let reply = OutboundMessage {
        text: user::handle_message(&env, &message.sender_id, message.text, &message.event_id)
            .await?,
        space_id: message.space_id,
    };

    client
        .send(&reply, &message.event_id)
        .await
        .inspect_err(|err| console_error!("reply send failed: {err}"))?;
    Response::ok("ok")
}
