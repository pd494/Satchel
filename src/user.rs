use crate::agent::Agent;
use crate::db::Db;
use crate::harness::Harness;
use crate::photon::{InboundMessage, PhotonClient};
use std::time::Duration;
use worker::{
    durable_object, DurableObject, Env, Error, Method, Request, RequestInit, Response, Result,
    State,
};

#[durable_object]
pub struct User {
    state: State,
    harness: Harness,
}

impl User {
    /// Hands one inbound text to the sender's Durable Object, which saves it and replies later.
    pub async fn send(env: &Env, message: &InboundMessage) -> Result<()> {
        let stub = env
            .durable_object("USER")?
            .get_by_name(&message.sender_id)?;

        let request = Request::new_with_init(
            "https://do/",
            RequestInit::new()
                .with_method(Method::Post)
                .with_body(Some(serde_json::to_string(message)?.into())),
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
        let db = Db::open(state.storage().sql()).expect("messages schema");
        let ai = env.ai("AI").expect("AI binding");
        let photon = PhotonClient::from_env(&env).expect("Photon secrets");
        Self {
            state,
            harness: Harness::new(db, Agent::new(ai), photon),
        }
    }

    /// Saves the user's text and schedules the alarm that replies to it.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let message: InboundMessage = req.json().await?;

        if message.text.trim().eq_ignore_ascii_case("reset") {
            self.state.storage().delete_all().await?;
            // delete_all drops the table too; recreate it so the next text works.
            Db::open(self.state.storage().sql())?;
        }

        self.harness.receive(&message)?;
        self.state.storage().set_alarm(Duration::ZERO).await?;
        Response::ok("ok")
    }

    /// Replies to saved messages one at a time; the runtime retries this if it fails.
    async fn alarm(&self) -> Result<Response> {
        self.harness.answer_pending().await?;
        Response::ok("ok")
    }
}
