use crate::agent::Agent;
use crate::db::Db;
use crate::harness::Harness;
use worker::{
    durable_object, DurableObject, Env, Error, Headers, Method, Request, RequestInit, Response,
    Result, State,
};

/// Header that carries Photon's `webhook-id` from the Worker into the DO.
const EVENT_ID_HEADER: &str = "x-event-id";

#[durable_object]
pub struct User {
    state: State,
    harness: Harness,
}

impl User {
    /// Forwards one inbound text to the sender's Durable Object and returns its reply.
    pub async fn send(env: &Env, sender_id: &str, text: &str, event_id: &str) -> Result<String> {
        let stub = env.durable_object("USER")?.get_by_name(sender_id)?;

        let headers = Headers::new();
        headers.set(EVENT_ID_HEADER, event_id)?;
        let request = Request::new_with_init(
            "https://do/",
            RequestInit::new()
                .with_method(Method::Post)
                .with_body(Some(text.into()))
                .with_headers(headers),
        )?;
        let mut response = stub.fetch_with_request(request).await?;
        match response.status_code() {
            200..=299 => response.text().await,
            status => {
                let detail = response.text().await.unwrap_or_default();
                Err(Error::RustError(format!(
                    "User DO returned {status} {detail}"
                )))
            }
        }
    }
}

impl DurableObject for User {
    fn new(state: State, env: Env) -> Self {
        let db = Db::open(state.storage().sql()).expect("messages schema");
        let ai = env.ai("AI").expect("AI binding");
        Self {
            state,
            harness: Harness::new(db, Agent::new(ai)),
        }
    }

    /// Takes the user's text as the request body and returns the reply text.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let text = req.text().await?;
        let event_id = req
            .headers()
            .get(EVENT_ID_HEADER)?
            .ok_or_else(|| Error::RustError(format!("missing {EVENT_ID_HEADER}")))?;

        if text.trim().eq_ignore_ascii_case("reset") {
            self.state.storage().delete_all().await?;
            // delete_all drops the table too; recreate it so the next text works.
            Db::open(self.state.storage().sql())?;
            return Response::ok("Reset done. Text again to start as a new user.");
        }

        let reply = self.harness.run_turn(&text, &event_id).await?;
        Response::ok(reply)
    }
}
