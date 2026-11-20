use crate::agent::{Agent, Calls};
use crate::db::{Db, UnansweredMessage};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Note};
use crate::tools::ToolCall;
use worker::{console_error, Env, Result, SqlStorage};

/// Past messages the model sees with each new message.
const HISTORY_LIMIT: usize = 40;

/// Runs before a message gets `SORRY` instead.
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
            agent: Agent::new(env.ai("AI")?)?,
            linq: LinqClient::from_env(env)?,
        })
    }

    /// Saves an inbound text so `inbox` replies to it later.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, text)?;
        Ok(())
    }

    /// Replies to every unanswered message, oldest first. A reply is stored before it is sent,
    /// so a failed send only resends it. After `MAX_ATTEMPTS` runs, a message gets `SORRY`,
    /// and it is marked sent even if that send fails.
    pub async fn inbox(&self, space_id: &str) -> Result<()> {
        // Show the user that Satchel read the chat. A failure here does not stop the replies.
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }
        while let Some(message) = self.db.next_unanswered()? {
            // Count this attempt before the run. A crash mid-run still counts toward
            // `MAX_ATTEMPTS`.
            let last_try = message.attempts >= MAX_ATTEMPTS;
            self.db.record_attempt(&message.event_id)?;

            // Get the reply: the stored one, `SORRY`, or a new one from the model.
            let text = self.response_for(&message, last_try).await?;

            // Send it. Mark it sent if the send worked, or if this was the last try.
            // An unsent reply is sent again on the next `inbox`.
            let sent = self.send_text(space_id, &message.event_id, &text).await;
            if sent.is_ok() || last_try {
                self.db.mark_sent(&message.event_id)?;
            }
            sent?;
        }
        Ok(())
    }

    /// Returns the stored unsent reply, or makes and stores a new one. The reply is empty if
    /// the model only reacted.
    async fn response_for(&self, message: &UnansweredMessage, last_try: bool) -> Result<String> {
        if let Some(text) = &message.response {
            return Ok(text.clone());
        }
        let text = if last_try {
            SORRY.to_string()
        } else {
            self.run_agent(message).await?
        };
        self.db.save_agent_message(&message.event_id, &text)?;
        Ok(text)
    }

    /// Runs the model on `message` until it stops calling tools or reacts. Text that comes
    /// with tool calls is ignored, since the model writes its reply after it sees the results.
    /// A tapback is the whole reply, so the text is empty.
    async fn run_agent(&self, message: &UnansweredMessage) -> Result<String> {
        let history = self.db.load_history(message.id, HISTORY_LIMIT)?;
        let links = Link::find_all(&message.content);
        let mut context = Agent::context(&history);
        let text = loop {
            let (text, calls) = self.agent.run_model(&mut context).await?;
            if calls.is_empty() {
                break text;
            }
            let results = self.run_tools(&calls, message, &links).await?;
            let failed = results.iter().any(|result| result.starts_with("error"));
            if Self::reacted(&calls) && !failed {
                break String::new();
            }
            Agent::push_results(&mut context, results);
        };
        if history.len() == 1 {
            return Ok(GREETING.to_string());
        }
        Ok(text)
    }

    /// Runs each tool call and returns one result per call: "ok", or an error the model can
    /// fix. A Linq or D1 failure is an `Err`.
    async fn run_tools(
        &self,
        calls: &Calls,
        message: &UnansweredMessage,
        links: &[Link],
    ) -> Result<Vec<String>> {
        let mut results = Vec::new();
        for call in calls {
            let result = match call {
                Err(error) => format!("error: {error}"),
                Ok(ToolCall::React { emoji }) => {
                    self.linq.react(&message.event_id, *emoji).await?;
                    "ok".to_string()
                }
                Ok(ToolCall::Save { urls, note }) => 'save: {
                    for url in urls {
                        let found = Link::find_all(url)
                            .pop()
                            .and_then(|wanted| links.iter().find(|link| link.url == wanted.url));
                        let Some(link) = found else {
                            break 'save format!("error: {url} is not in the newest message");
                        };
                        match File::parse(link.url.as_str()) {
                            Some(file) => self.db.insert_saved(message.id, &file)?,
                            None => self.db.insert_saved(message.id, link)?,
                        }
                    }
                    if let Some(note) = note {
                        self.db.insert_saved(message.id, &Note(note.clone()))?;
                    }
                    "ok".to_string()
                }
            };
            results.push(result);
        }
        Ok(results)
    }

    /// Sends each part of `text` as its own message. Linq requires a link or media part to be
    /// the only part.
    async fn send_text(&self, space_id: &str, event_id: &str, text: &str) -> Result<()> {
        let (rest, urls) = Link::split(text);
        let mut parts = Vec::new();
        if !rest.is_empty() {
            parts.push(Part::Text(rest));
        }
        for url in urls {
            let part = match File::parse(&url) {
                Some(File {
                    kind: Kind::Voice,
                    id,
                    ..
                }) => Part::Voice(id),
                Some(file) => Part::Media(file.id),
                None => Part::Link(url),
            };
            parts.push(part);
        }
        for (i, part) in parts.into_iter().enumerate() {
            self.linq
                .send(space_id, &format!("{event_id}:{i}"), part)
                .await?;
        }
        Ok(())
    }

    /// A `react` call ends the model's reply, since a tapback means it has nothing to say.
    fn reacted(calls: &Calls) -> bool {
        for call in calls {
            if let Ok(ToolCall::React { .. }) = call {
                return true;
            }
        }
        false
    }
}
