mod sendblue;

use sendblue::{handle_webhook, SendblueClient};
use worker::{event, Context, Env, Method, Request, Response, Result};

/// Worker entry point and HTTP routing.
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match (req.method(), req.path().as_str()) {
        (Method::Get, "/") => Response::ok("hello world"),
        (Method::Post, "/webhook") => handle_webhook(req, &sendblue_client(&env)?).await,
        _ => Response::error("not found", 404),
    }
}

/// Reads Sendblue credentials from `.env` locally or `wrangler secret put` in production.
fn sendblue_client(env: &Env) -> Result<SendblueClient> {
    Ok(SendblueClient {
        api_key_id: env.secret("SENDBLUE_API_KEY_ID")?.to_string(),
        api_secret_key: env.secret("SENDBLUE_API_SECRET_KEY")?.to_string(),
        from_number: env.secret("SENDBLUE_FROM_NUMBER")?.to_string(),
    })
}
