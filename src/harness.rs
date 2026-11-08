use crate::agent::{Agent, Intent};
use crate::db::{Db, Pending};
use crate::media::{Link, Note};
use crate::photon::{OutboundMessage, PhotonClient};
use worker::{console_error, Env, Result, SqlStorage};

/// Failed model calls before a message gets `sorry` instead.
const MAX_ATTEMPTS: i64 = 3;

/// Lowest Clef save confidence that stores a link-free message as a note.
const MIN_NOTE_CONFIDENCE: f64 = 0.5;

/// The tapback a saved message gets instead of a text reply.
const SAVED_TAPBACK: &str = "👍";

/// How Satchel answers one message.
enum Reply {
    Text(String),
    /// A tapback on the user's message `on` (its Photon id).
    Tapback {
        on: String,
    },
}

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
    pub fn receive(&self, event_id: &str, photon_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, photon_id, text)?;
        Ok(())
    }

    /// Replies to every unanswered message, oldest first. A message whose model call failed
    /// `MAX_ATTEMPTS` times gets `sorry` instead, so it can't block the messages after it.
    pub async fn answer_pending(&self, space_id: &str) -> Result<()> {
        while let Some(message) = self.db.next_unanswered()? {
            let read_key = format!("{}:read", message.event_id);
            if let Err(error) = self
                .photon
                .mark_read(space_id, &message.photon_id, &read_key)
                .await
            {
                console_error!("mark read failed: {error}");
            }
            let context = self.db.load_history(message.id, 6)?;
            let reply = if context.len() == 1 {
                Reply::Text(GREETING.to_string())
            } else if message.attempts >= MAX_ATTEMPTS {
                Reply::Text(sorry(&message.content))
            } else {
                let result: Result<Reply> = async {
                    let saved_it = match self.agent.classify_intent(&message.content).await? {
                        Intent::Save { confidence } => self.save(&message, confidence).await?,
                        Intent::Search | Intent::Remind | Intent::Chat => false,
                    };
                    if saved_it {
                        return Ok(Reply::Tapback {
                            on: message.photon_id.clone(),
                        });
                    }
                    let saved = self.db.load_saved(20)?;
                    Ok(Reply::Text(self.agent.response(&context, &saved).await?))
                }
                .await;
                if result.is_err() {
                    self.db.record_failure(&message.event_id)?;
                }
                result?
            };
            let answer = match reply {
                Reply::Text(text) => self.send_text(space_id, text, &message.event_id).await?,
                Reply::Tapback { on } => {
                    let react = self
                        .photon
                        .react(space_id, &on, SAVED_TAPBACK, &message.event_id);
                    match react.await {
                        Ok(()) => SAVED_TAPBACK.to_string(),
                        // Shared Photon lines can't send tapbacks. The message is already
                        // marked read, which is the quietest acknowledgement they allow.
                        Err(error) => {
                            console_error!("tapback failed, leaving the read receipt: {error}");
                            "Saved".to_string()
                        }
                    }
                }
            };
            self.db.save_agent_message(&message.event_id, &answer)?;
        }
        Ok(())
    }

    /// Sends `text` into `space_id` and returns it, for the history.
    async fn send_text(
        &self,
        space_id: &str,
        text: String,
        idempotency_key: &str,
    ) -> Result<String> {
        let outbound = OutboundMessage {
            text,
            space_id: space_id.to_string(),
        };
        self.photon.send(&outbound, idempotency_key).await?;
        Ok(outbound.text)
    }

    /// Saves every link in `message`, or the whole text as a note when it has none.
    /// Returns `false`, saving nothing, for a link-free message that Clef was unsure about:
    /// that is usually a question ("send me the chili recipe"), so it gets an answer instead.
    async fn save(&self, message: &Pending, confidence: f64) -> Result<bool> {
        let found = Link::find_all(&message.content);
        let links = self.agent.check_links(&message.content, found).await;
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

fn sorry(text: &str) -> String {
    format!("Sorry, I lost \"{text}\" and couldn't answer it. Try sending it again.")
}
