use worker::{event, Context, Env, Method, Request, Response, Result};

/// Worker entry point and HTTP routing.
#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    match (req.method(), req.path().as_str()) {
        (Method::Get, "/") => Response::ok("hello world"),
        _ => Response::error("not found", 404),
    }
}
