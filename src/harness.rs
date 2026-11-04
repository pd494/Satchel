use crate::agent::Agent;
use crate::db::Db;
use crate::photon::{OutboundMessage, PhotonClient};
use worker::{Env, Result, SqlStorage};

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

pub struct Harness {
    db: Db,
    agent: Agent,
    photon: PhotonClient,
}

impl Harness {
    pub fn new(sql: SqlStorage, env: &Env) -> Result<Self> {
        Ok(Self {
            db: Db::open(sql)?,
            agent: Agent::new(env.ai("AI")?),
            photon: PhotonClient::from_env(env)?,
        })
    }

    /// Saves an inbound text so `answer_pending` replies to it later.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, text)?;
        Ok(())
    }

    /// Replies to every unanswered message, oldest first.
    pub async fn answer_pending(&self, space_id: &str) -> Result<()> {
        while let Some(message) = self.db.next_unanswered()? {
            let context = self.db.load_history(message.id, 6)?;
            let text = if context.len() == 1 {
                GREETING.to_string()
            } else {
                self.agent.response(&context).await?
            };
            let reply = OutboundMessage {
                text,
                space_id: space_id.to_string(),
            };
            self.photon.send(&reply, &message.event_id).await?;
            self.db.save_agent_message(&message.event_id, &reply.text)?;
        }
        Ok(())
    }
}
