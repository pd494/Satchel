use crate::agent::Agent;
use crate::db::{Db, Pending};
use crate::linq::{InboundMessage, LinqClient, OutboundMessage};
use crate::media::{Attachment, Link};
use worker::{console_error, Env, Result, SqlStorage};

/// Failed model calls or sends before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const KEEP_LINK: f64 = 0.1;

const SAVED_TAPBACK: &str = "👍";

enum Reply {
    Text(String),
    Tapback,
}

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

    /// Saves an inbound message and its media so `answer_pending` replies to it later.
    /// A webhook retry with the same `event_id` saves nothing.
    pub fn receive(&self, message: &InboundMessage) -> Result<()> {
        if let Some(id) = self
            .db
            .save_user_message(&message.event_id, &message.text)?
        {
            for attachment in &message.media {
                self.db.insert_saved(id, attachment)?;
            }
        }
        Ok(())
    }

    /// Replies to every unanswered message, oldest first. After `MAX_ATTEMPTS` failures, a
    /// message gets `SORRY`, and it is marked answered even if that send fails.
    pub async fn answer_pending(&self, space_id: &str) -> Result<()> {
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }
        while let Some(message) = self.db.next_unanswered()? {
            let reply = if message.attempts >= MAX_ATTEMPTS {
                Reply::Text(SORRY.to_string())
            } else {
                let result: Result<Reply> = async {
                    if self.save(&message).await? || message.saved > 0 {
                        return Ok(Reply::Tapback);
                    }
                    let context = self.db.load_history(message.id, 6)?;
                    if context.len() == 1 {
                        return Ok(Reply::Text(GREETING.to_string()));
                    }
                    Ok(Reply::Text(self.agent.response(&context).await?))
                }
                .await;
                if result.is_err() {
                    self.db.record_failure(&message.event_id)?;
                }
                result?
            };
            let (answer, sent) = match reply {
                Reply::Text(text) => {
                    let (rest, ids) = Attachment::split_tags(&text);
                    let (rest, links) = Link::split(&rest);
                    let mut attachments = Vec::new();
                    for id in ids {
                        attachments.extend(self.db.attachment_id(id)?);
                    }
                    let outbound = OutboundMessage {
                        space_id: space_id.to_string(),
                        text: rest,
                        links,
                        attachments,
                        idempotency_key: message.event_id.clone(),
                    };
                    let sent = self.linq.send(&outbound).await;
                    (text, sent)
                }
                Reply::Tapback => {
                    let sent = self.linq.like(&message.event_id).await;
                    (SAVED_TAPBACK.to_string(), sent)
                }
            };
            if sent.is_err() && message.attempts < MAX_ATTEMPTS {
                self.db.record_failure(&message.event_id)?;
            } else {
                self.db.save_agent_message(&message.event_id, &answer)?;
            }
            sent?;
        }
        Ok(())
    }

    async fn save(&self, message: &Pending) -> Result<bool> {
        let found = Link::find_all(&message.content);
        if found.is_empty() {
            return Ok(false);
        }
        let odds = match self.agent.website_odds(&message.content, &found).await {
            Ok(odds) => odds,
            Err(error) => {
                console_error!("website_odds failed, keeping all links: {error}");
                vec![None; found.len()]
            }
        };
        let links: Vec<Link> = found
            .into_iter()
            .zip(odds)
            .filter(|(_, odds)| odds.is_none_or(|odds| odds >= KEEP_LINK))
            .map(|(link, _)| link)
            .collect();
        for link in &links {
            self.db.insert_saved(message.id, link)?;
        }
        Ok(!links.is_empty())
    }
}
