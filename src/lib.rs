mod agent;
pub mod db;
mod harness;
pub mod linq;
mod media;
pub mod user;

use linq::{LinqRequestOutcome, LinqWebhook};
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

    let message = match LinqWebhook::from_env(&env)?.receive(req).await? {
        LinqRequestOutcome::Message(message) => message,
        LinqRequestOutcome::Unsupported => return Response::ok("ok"),
        LinqRequestOutcome::BadSignature => return Response::error("bad signature", 401),
        LinqRequestOutcome::TooLarge => return Response::error("payload too large", 413),
    };

    User::send(&env, &message).await?;
    Response::ok("ok")
}
