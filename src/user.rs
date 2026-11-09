use crate::db::Db;
use crate::harness::Harness;
use crate::linq::InboundMessage;
use std::time::Duration;
use worker::{
    durable_object, DurableObject, Env, Error, Method, Request, RequestInit, Response, Result,
    State,
};

/// Storage key for the space the alarm replies into.
const SPACE_ID_KEY: &str = "space_id";

#[durable_object]
pub struct User {
    state: State,
    harness: Harness,
}

impl User {
    /// Hands one inbound message to the sender's Durable Object, which replies from its alarm.
    pub async fn send(env: &Env, message: &InboundMessage) -> Result<()> {
        let stub = env
            .durable_object("USER")?
            .get_by_name(&message.sender_id)?;
        let body = serde_json::to_string(message)?;
        let request = Request::new_with_init(
            "https://do/",
            RequestInit::new()
                .with_method(Method::Post)
                .with_body(Some(body.into())),
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
        let harness = Harness::new(state.storage().sql(), &env).expect("harness setup");
        Self { state, harness }
    }

    /// Saves the user's message and media, and sets the alarm that replies to it.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let message: InboundMessage = req.json().await?;

        if message.text.trim().eq_ignore_ascii_case("reset") {
            self.state.storage().delete_all().await?;
            Db::open(self.state.storage().sql())?;
        }

        self.harness.receive(&message)?;
        self.state
            .storage()
            .put(SPACE_ID_KEY, &message.space_id)
            .await?;
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
