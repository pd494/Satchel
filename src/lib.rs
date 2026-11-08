mod agent;
pub mod db;
mod harness;
mod media;
pub mod photon;
pub mod user;

use photon::{PhotonClient, PhotonRequestOutcome};
use user::User;
use worker::{event, Context, Env, Method, Request, Response, Result};

/// Worker entry point and HTTP routing.
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match (req.method(), req.path().as_str()) {
        (Method::Get, "/") => return Response::ok("hello world"),
        (Method::Post, "/webhook") => {}
        _ => return Response::error("not found", 404),
    }

    let client = PhotonClient::from_env(&env)?;
    let message = match client.receive(req).await? {
        PhotonRequestOutcome::Text(message) => message,
        PhotonRequestOutcome::Unsupported => return Response::ok("ok"),
        PhotonRequestOutcome::BadSignature => return Response::error("bad signature", 401),
        PhotonRequestOutcome::TooLarge => return Response::error("payload too large", 413),
    };

    User::send(
        &env,
        &message.sender_id,
        &message.text,
        &message.event_id,
        &message.space_id,
    )
    .await?;
    Response::ok("ok")
}
