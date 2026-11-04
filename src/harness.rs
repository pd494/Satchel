use crate::agent::Agent;
use crate::db::Db;
use crate::photon::{InboundMessage, OutboundMessage, PhotonClient};
use worker::Result;

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

/// Runs conversation turns: owns the user's state (`Db`), the model (`Agent`), and the outbound
/// channel (`PhotonClient`), and is the only place that knows the order of a turn.
pub struct Harness {
    db: Db,
    agent: Agent,
    photon: PhotonClient,
}

impl Harness {
    pub fn new(db: Db, agent: Agent, photon: PhotonClient) -> Self {
        Self { db, agent, photon }
    }

    /// Saves an inbound text so a later `answer_pending` replies to it.
    pub fn receive(&self, message: &InboundMessage) -> Result<()> {
        self.db
            .save_user_message(&message.event_id, &message.space_id, &message.text)
    }

    /// Replies to every unanswered message, oldest first.
    ///
    /// A brand-new user gets `GREETING` instead of a model call. The reply is saved only after it
    /// is sent, so a failure leaves the message unanswered and the alarm's retry tries again.
    pub async fn answer_pending(&self) -> Result<()> {
        while let Some(message) = self.db.next_unanswered()? {
            let context = self.db.load_history(message.id, 6)?;
            let reply = OutboundMessage {
                text: if context.len() == 1 {
                    GREETING.to_string()
                } else {
                    self.agent.reply(&context).await?
                },
                space_id: message.space_id,
            };
            self.photon.send(&reply, &message.event_id).await?;
            self.db.save_agent_message(&message.event_id, &reply.text)?;
        }
        Ok(())
    }
}
