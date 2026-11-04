use crate::agent::Agent;
use crate::db::Db;
use worker::Result;

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

/// Runs a conversation turn: owns the user's state (`Db`) and the model (`Agent`), and is the
/// only place that knows the order of a turn.
pub struct Harness {
    db: Db,
    agent: Agent,
}

impl Harness {
    pub fn new(db: Db, agent: Agent) -> Self {
        Self { db, agent }
    }

    /// Runs one conversational turn and returns the reply text.
    ///
    /// A retried webhook gets the reply already saved for it; a brand-new user gets `GREETING`
    /// instead of a model call. A failed or empty model reply is an error, so nothing is saved for
    /// it and Photon's retry tries again.
    pub async fn run_turn(&self, text: &str, event_id: &str) -> Result<String> {
        if let Some(existing_reply) = self.db.load_reply(event_id)? {
            return Ok(existing_reply);
        }

        let is_new_user = self.db.load_history(1)?.is_empty();
        self.db.save_user_message(event_id, text)?;

        let reply = if is_new_user {
            GREETING.to_string()
        } else {
            let context = self.db.load_history(6)?;
            self.agent.response(&context).await?
        };
        self.db.save_agent_message(event_id, &reply)?;
        Ok(reply)
    }
}
