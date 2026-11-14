use crate::agent::{Agent, ToolExecutor};
use crate::db::{Db, Pending};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Media, Note};
use crate::tools::{React, Save, Tapback, ToolCall, ToolResult};
use worker::{console_error, Env, Error, Result, SqlStorage};

/// Failed model calls or sends before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

enum Reply {
    Text(String),
    /// The emoji the `react` tool already sent.
    Tapback(&'static str),
}

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

pub struct Harness {
    db: Db,
    agent: Agent,
    linq: LinqClient,
}

impl Harness {
    pub fn new(sql: SqlStorage, env: &Env, user_id: String) -> Result<Self> {
        Ok(Self {
            db: Db::open(sql)?,
            agent: Agent::new(env.ai("AI")?, user_id),
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
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }
        while let Some(message) = self.db.next_unanswered()? {
            let reply = if message.attempts >= MAX_ATTEMPTS {
                Reply::Text(SORRY.to_string())
            } else {
                let result: Result<Reply> = async {
                    let context = self.db.load_history(message.id, 6)?;
                    let labels = label(&message.content);
                    let listed: String = labels
                        .iter()
                        .map(|(label, part)| format!("\n[{label}] {}", part.content()))
                        .collect();
                    let mut turn = Turn {
                        db: &self.db,
                        linq: &self.linq,
                        message: &message,
                        labels,
                        tapback: None,
                    };
                    let text = self.agent.response(&context, &listed, &mut turn).await?;
                    match (text.is_empty(), turn.tapback) {
                        _ if context.len() == 1 => Ok(Reply::Text(GREETING.to_string())),
                        (false, _) => Ok(Reply::Text(text)),
                        (true, Some(tapback)) => Ok(Reply::Tapback(tapback.emoji())),
                        (true, None) => Err(Error::RustError("model sent no reply".into())),
                    }
                }
                .await;
                if result.is_err() {
                    self.db.record_failure(&message.event_id)?;
                }
                result?
            };
            let (answer, sent) = match reply {
                Reply::Text(text) => {
                    let sent = self.send_text(space_id, &message.event_id, &text).await;
                    (text, sent)
                }
                Reply::Tapback(emoji) => (emoji.to_string(), Ok(())),
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

    /// Sends each part of `text` as its own message. Linq requires a link or media part to be
    /// the only part. A saved photo, video, contact, location, or file URL goes back as the file itself, and a saved
    /// voice memo goes back as a voice-memo bubble.
    async fn send_text(&self, space_id: &str, event_id: &str, text: &str) -> Result<()> {
        let (rest, urls) = Link::split(text);
        let text = (!rest.is_empty()).then_some(Part::Text(rest));
        let files = urls.into_iter().map(|url| match File::parse(&url) {
            Some(File {
                kind: Kind::Voice,
                id,
                ..
            }) => Part::Voice(id),
            Some(file) => Part::Media(file.id),
            None => Part::Link(url),
        });
        for (i, part) in text.into_iter().chain(files).enumerate() {
            self.linq
                .send(space_id, &format!("{event_id}:{i}"), part)
                .await?;
        }
        Ok(())
    }
}

/// Labels the links and files in `text` `L1`, `F1`, ... so the model can save them by label.
fn label(text: &str) -> Vec<(String, Box<dyn Media>)> {
    let (mut links, mut files) = (0, 0);
    Link::find_all(text)
        .into_iter()
        .map(|link| match File::parse(link.url.as_str()) {
            Some(file) => {
                files += 1;
                (format!("F{files}"), Box::new(file) as Box<dyn Media>)
            }
            None => {
                links += 1;
                (format!("L{links}"), Box::new(link) as Box<dyn Media>)
            }
        })
        .collect()
}

/// Runs the model's tool calls for one message.
struct Turn<'a> {
    db: &'a Db,
    linq: &'a LinqClient,
    message: &'a Pending,
    labels: Vec<(String, Box<dyn Media>)>,
    tapback: Option<Tapback>,
}

impl ToolExecutor for Turn<'_> {
    async fn execute(&mut self, call: ToolCall) -> ToolResult {
        match call {
            ToolCall::React(React { emoji }) => self.react(emoji).await,
            ToolCall::Save(Save { items, note }) => self.save(&items, note),
        }
    }
}

impl Turn<'_> {
    async fn react(&mut self, tapback: Tapback) -> ToolResult {
        self.linq
            .react(&self.message.event_id, tapback)
            .await
            .map_err(|error| error.to_string())?;
        self.tapback = Some(tapback);
        Ok("reacted".into())
    }

    fn save(&self, items: &[String], note: Option<String>) -> ToolResult {
        let parts = items
            .iter()
            .map(|item| {
                self.labels
                    .iter()
                    .find(|(label, _)| label == item)
                    .map(|(_, part)| part.as_ref())
                    .ok_or_else(|| format!("unknown label {item}"))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let insert = |part: &dyn Media| {
            self.db
                .insert_saved(self.message.id, part)
                .map_err(|error| error.to_string())
        };
        parts.into_iter().try_for_each(insert)?;
        note.map(|note| insert(&Note(note))).transpose()?;
        Ok("saved".into())
    }
}
