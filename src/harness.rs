use crate::agent::Agent;
use crate::db::{Db, Pending};
use crate::photon::{InboundMessage, OutboundMessage, PhotonClient};
use worker::Result;

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";
const FALLBACK: &str =
    "Sorry, I couldn't answer that one. Your message is saved, so try asking again.";
const MAX_ATTEMPTS: i64 = 3;

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

    /// Sends every pending reply, oldest first, writing each one before it is sent.
    ///
    /// Every step reads its state from `Db`, so an alarm retry resumes where the last run stopped:
    /// a saved reply is resent as is (Photon dedupes on `event_id`), and a message whose model call
    /// keeps failing gets `FALLBACK` after `MAX_ATTEMPTS` so later messages are not stuck behind it.
    pub async fn answer_pending(&self) -> Result<()> {
        while let Some(message) = self.db.next_pending()? {
            let text = match message.reply {
                Some(text) => text,
                None => {
                    let text = self.compose(&message).await?;
                    self.db.save_agent_message(&message.event_id, &text)?;
                    text
                }
            };
            let reply = OutboundMessage {
                space_id: message.space_id,
                text,
            };
            self.photon.send(&reply, &message.event_id).await?;
            self.db.mark_sent(&message.event_id)?;
        }
        Ok(())
    }

    async fn compose(&self, message: &Pending) -> Result<String> {
        let context = self.db.load_history(message.id, 6)?;
        if context.len() == 1 {
            return Ok(GREETING.to_string());
        }
        if self.db.record_attempt(message.id)? > MAX_ATTEMPTS {
            return Ok(FALLBACK.to_string());
        }
        self.agent.reply(&context).await
    }
}
