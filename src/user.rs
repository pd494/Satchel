use worker::{
    durable_object, DurableObject, Env, Error, Headers, Method, Request, RequestInit, Response,
    Result, State,
};

#[durable_object]
pub struct User {
    state: State,
}

pub async fn handle_message(
    env: &Env,
    sender_id: &str,
    text: String,
    event_id: &str,
) -> Result<String> {
    let stub = env.durable_object("USER")?.get_by_name(sender_id)?;

    let headers = Headers::new();
    headers.set("x-event-id", event_id)?;
    let request = Request::new_with_init(
        "https://do/",
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(text.into()))
            .with_headers(headers),
    )?;
    let mut cf_response = stub.fetch_with_request(request).await?;
    match cf_response.status_code() {
        200..=299 => cf_response.text().await,
        status => {
            let detail = cf_response.text().await.unwrap_or_default();
            Err(Error::RustError(format!(
                "User DO returned {status} {detail}"
            )))
        }
    }
}

impl DurableObject for User {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    /// Takes the user's text as the request body and returns the reply text.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let storage = self.state.storage();
        let text = req.text().await?;
        let idempotency_key = format!(
            "event:{}",
            req.headers()
                .get("x-event-id")?
                .ok_or_else(|| Error::RustError("missing x-event-id".into()))?
        );

        if let Some(reply) = storage.get::<String>(&idempotency_key).await? {
            return Response::ok(reply);
        }
        let reply = if text.trim().eq_ignore_ascii_case("reset") {
            storage.delete_all().await?;
            "Reset done. Text again to start as a new user."
        } else if storage.get::<String>("id").await?.is_some() {
            "Hello existing user, how can I help you today?"
        } else {
            storage.put("id", self.state.id().to_string()).await?;
            "Hello new user, how can I help you today?"
        };

        storage.put(&idempotency_key, reply).await?;
        Response::ok(reply)
    }
}
