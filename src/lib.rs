pub mod photon;

use photon::PhotonClient;
use worker::{event, Context, Env, Method, Request, Response, Result};

/// Worker entry point and HTTP routing.
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match (req.method(), req.path().as_str()) {
        (Method::Get, "/") => Response::ok("hello world"),
        (Method::Post, "/webhook") => {
            let client = PhotonClient::new(
                env.secret("PROJECT_ID")?.to_string(),
                env.secret("PROJECT_SECRET")?.to_string(),
                env.secret("PHOTON_WEBHOOK_SECRET")?.to_string(),
            );
            client.handle_webhook(req).await
        }
        _ => Response::error("not found", 404),
    }
}
