use crate::agent::{Agent, Intent};
use crate::db::{Db, Pending};
use crate::linq::{LinqClient, OutboundMessage, Part};
use crate::media::{Link, Note};
use worker::{console_error, Env, Result, SqlStorage};

/// Failed model calls or sends before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const MIN_NOTE_CONFIDENCE: f64 = 0.5;

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

    /// Saves an inbound text so `answer_pending` replies to it later.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, text)?;
        Ok(())
    }

    /// Replies to every unanswered message, oldest first. After `MAX_ATTEMPTS` failures, a
    /// message gets `SORRY`, and it is marked answered even if that send fails.
    pub async fn answer_pending(&self, space_id: &str) -> Result<()> {
        while let Some(message) = self.db.next_unanswered()? {
            if let Err(error) = self.linq.mark_read(space_id).await {
                console_error!("mark read failed: {error}");
            }
            let context = self.db.load_history(message.id, 6)?;
            let reply = if context.len() == 1 {
                Reply::Text(GREETING.to_string())
            } else if message.attempts >= MAX_ATTEMPTS {
                Reply::Text(SORRY.to_string())
            } else {
                let result: Result<Reply> = async {
                    let saved_it = match self.agent.classify_intent(&message.content).await? {
                        (Intent::Save, confidence) => self.save(&message, confidence).await?,
                        (Intent::Search | Intent::Remind | Intent::Chat, _) => false,
                    };
                    if saved_it {
                        return Ok(Reply::Tapback);
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
                    let (rest, links) = Link::split(&text);
                    let mut parts = Vec::new();
                    if !rest.is_empty() {
                        parts.push(Part::Text(rest));
                    }
                    parts.extend(links.iter().map(|link| Part::Link(link.url.to_string())));
                    let outbound = OutboundMessage {
                        space_id: space_id.to_string(),
                        parts,
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

    async fn save(&self, message: &Pending, confidence: f64) -> Result<bool> {
        let found = Link::find_all(&message.content);
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
        if links.is_empty() {
            if confidence < MIN_NOTE_CONFIDENCE {
                return Ok(false);
            }
            let note = Note {
                text: message.content.clone(),
            };
            self.db.insert_saved(message.id, &note)?;
        }
        for link in &links {
            self.db.insert_saved(message.id, link)?;
        }
        Ok(true)
    }
}
