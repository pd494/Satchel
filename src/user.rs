use crate::db::Db;
use crate::harness::Harness;
use std::time::Duration;
use worker::{
    durable_object, DurableObject, Env, Error, Headers, Method, Request, RequestInit, Response,
    Result, State,
};

const EVENT_ID_HEADER: &str = "x-event-id";
const SPACE_ID_HEADER: &str = "x-space-id";
/// Storage key for the space the alarm replies into.
const SPACE_ID_KEY: &str = "space_id";

#[durable_object]
pub struct User {
    state: State,
    harness: Harness,
}

impl User {
    /// Hands one inbound text to the sender's Durable Object, which replies from its alarm.
    pub async fn send(
        env: &Env,
        sender_id: &str,
        text: &str,
        event_id: &str,
        space_id: &str,
    ) -> Result<()> {
        let stub = env.durable_object("USER")?.get_by_name(sender_id)?;

        let headers = Headers::new();
        headers.set(EVENT_ID_HEADER, event_id)?;
        headers.set(SPACE_ID_HEADER, space_id)?;
        let request = Request::new_with_init(
            "https://do/",
            RequestInit::new()
                .with_method(Method::Post)
                .with_body(Some(text.into()))
                .with_headers(headers),
        )?;
        let mut response = stub.fetch_with_request(request).await?;
        match response.status_code() {
            200..=299 => Ok(()),
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
        let user_id = state.id().name().expect("user DO is named");
        let harness = Harness::new(state.storage().sql(), &env, user_id).expect("harness setup");
        Self { state, harness }
    }

    /// Saves the user's text and sets the alarm that replies to it.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let text = req.text().await?;
        let event_id = req
            .headers()
            .get(EVENT_ID_HEADER)?
            .ok_or_else(|| Error::RustError(format!("missing {EVENT_ID_HEADER}")))?;
        let space_id = req
            .headers()
            .get(SPACE_ID_HEADER)?
            .ok_or_else(|| Error::RustError(format!("missing {SPACE_ID_HEADER}")))?;

        if text.trim().eq_ignore_ascii_case("reset") {
            self.state.storage().delete_all().await?;
            Db::open(self.state.storage().sql())?;
        }

        self.harness.receive(&event_id, &text)?;
        self.state.storage().put(SPACE_ID_KEY, &space_id).await?;
        self.state
            .storage()
            .set_alarm(Duration::from_secs(0))
            .await?;
        Response::ok("ok")
    }

    async fn alarm(&self) -> Result<Response> {
        let space_id: String = self
            .state
            .storage()
            .get(SPACE_ID_KEY)
            .await?
            .ok_or_else(|| Error::RustError(format!("missing {SPACE_ID_KEY}")))?;
        self.harness.answer_pending(&space_id).await?;
        Response::ok("ok")
    }
}
