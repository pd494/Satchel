use crate::agent::Agent;
use crate::db::Db;
use crate::linq::LinqClient;
use worker::{Env, Result, SqlStorage};

/// Failed model calls or sends before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

pub struct Harness {
    db: Db,
    agent: Agent,
    linq: LinqClient,
}

impl Harness {
    pub fn new(sql: SqlStorage, env: &Env) -> Result<Self> {
        Ok(Self {
            db: Db::open(sql)?,
            agent: Agent::new(env.ai("AI")?),
            linq: LinqClient::from_env(env)?,
        })
    }

    /// Saves an inbound text so `answer_pending` replies to it later.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, text)?;
        Ok(())
    }

    /// Replies to every unanswered message, oldest first. After `MAX_ATTEMPTS` failures, a
    /// message gets `SORRY`, and it is marked answered even if that send fails.
    pub async fn answer_pending(&self, space_id: &str) -> Result<()> {
        while let Some(message) = self.db.next_unanswered()? {
            let context = self.db.load_history(message.id, 6)?;
            let text = if context.len() == 1 {
                GREETING.to_string()
            } else if message.attempts >= MAX_ATTEMPTS {
                SORRY.to_string()
            } else {
                let response = self.agent.response(&context).await;
                if response.is_err() {
                    self.db.record_failure(&message.event_id)?;
                }
                response?
            };
            let sent = self.linq.send(space_id, &text, &message.event_id).await;
            if sent.is_err() && message.attempts < MAX_ATTEMPTS {
                self.db.record_failure(&message.event_id)?;
            } else {
                self.db.save_agent_message(&message.event_id, &text)?;
            }
            sent?;
        }
        Ok(())
    }
}
