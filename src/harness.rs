use crate::agent::Agent;
use crate::db::{Db, Schedule, UnansweredMessage};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Note, Reader};
use crate::tools::{Remind, Tapback, ToolCall};
use jiff::{tz::TimeZone, Timestamp};
use serde_json::json;
use std::time::Duration;
use worker::{console_error, Env, Result, SqlStorage};

/// Past messages the model sees with each new message.
const HISTORY_LIMIT: usize = 40;

/// Runs before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const REMINDER_EVENT_PREFIX: &str = "reminder:";

/// Seconds Satchel waits after a text for more texts. Texts sent together then get one reply.
const DEBOUNCE: i64 = 5;

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

pub struct Harness {
    db: Db,
    agent: Agent,
    linq: LinqClient,
    reader: Reader,
}

impl Harness {
    pub fn new(sql: SqlStorage, env: &Env) -> Result<Self> {
        Ok(Self {
            db: Db::open(sql)?,
            agent: Agent::from_env(env)?,
            linq: LinqClient::from_env(env)?,
            reader: Reader::from_env(env)?,
        })
    }

    /// Saves an inbound text so `inbox` replies to it later. Returns when to run `inbox`: after
    /// `DEBOUNCE`, or sooner when a reminder is due first.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<Duration> {
        self.db.save_user_message(event_id, text)?;
        let debounced = Timestamp::now().as_second() + DEBOUNCE;
        Ok(Self::until(
            self.db.next_due()?.unwrap_or(debounced).min(debounced),
        ))
    }

    fn until(at: i64) -> Duration {
        Duration::from_secs(at.saturating_sub(Timestamp::now().as_second()).max(0) as u64)
    }

    fn queue_due_reminders(&self) -> Result<()> {
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let now = Timestamp::now().as_second();
        for reminder in self.db.due_reminders(now)? {
            let event_id = format!(
                "{REMINDER_EVENT_PREFIX}{}:{}",
                reminder.id, reminder.schedule.due_at
            );
            let mut content = format!("Reminder due: {}", reminder.content);
            if let Some(urls) = &reminder.urls {
                content.push('\n');
                content.push_str(urls);
            }

            self.db.save_user_message(&event_id, &content)?;
            let next = reminder.next_fire(&zone)?;
            self.db.set_reminder(reminder.id, next.as_ref())?;
        }

        Ok(())
    }

    /// Replies to every unanswered message, oldest first. A reply is stored before it is sent,
    /// so a failed send only resends it. After `MAX_ATTEMPTS` runs, a message gets `SORRY`,
    /// and it is marked sent even if that send fails. A burst still inside `DEBOUNCE` waits.
    /// Returns how long until the next reminder or waiting burst, or `None` if none is pending.
    pub async fn inbox(&self, space_id: &str) -> Result<Option<Duration>> {
        // Show the user that Satchel read the chat. A failure here does not stop the replies.
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }

        self.queue_due_reminders()?;

        let mut waiting = None;
        loop {
            let burst = Self::next_turn(self.db.unanswered()?);
            let Some(message) = burst.last() else {
                break;
            };
            // A text that arrived while this alarm ran waits out its own debounce.
            let debounced = message.created_at + DEBOUNCE;
            if !message.alone() && debounced > Timestamp::now().as_second() {
                waiting = Some(debounced);
                break;
            }

            // Count this attempt before the run, on every text of the burst. A crash mid-run
            // still counts toward `MAX_ATTEMPTS`, and a new text does not restart the count.
            let last_try = burst.iter().any(|sent| sent.attempts >= MAX_ATTEMPTS);
            for sent in &burst {
                self.db.record_attempt(&sent.event_id)?;
            }

            // Get the reply: the stored one, `SORRY`, or a new one from the model.
            let typing = message.response.is_none() && !last_try;
            if typing {
                self.show_typing(space_id, true).await;
            }
            let reply = self.response_for(&burst, last_try).await;
            if typing && !matches!(&reply, Ok(text) if !text.is_empty()) {
                self.show_typing(space_id, false).await;
            }
            let text = reply?;

            // Send it. Mark it sent if the send worked, or if this was the last try.
            // An unsent reply is sent again on the next `inbox`.
            let sent = self.send_text(space_id, &message.event_id, &text).await;
            if sent.is_ok() || last_try {
                self.db.mark_sent(&message.event_id)?;
            }
            sent?;
        }

        // Read the next reminder after the loop. A `remind` call in this run may have set one.
        let wake = [self.db.next_due()?, waiting].into_iter().flatten().min();
        Ok(wake.map(Self::until))
    }

    /// Typing dots are a nicety. A failure does not stop the reply.
    async fn show_typing(&self, space_id: &str, on: bool) {
        if let Err(error) = self.linq.typing(space_id, on).await {
            console_error!("typing failed: {error}");
        }
    }

    /// The messages one reply answers, oldest first: the texts sent together. A due reminder,
    /// or a stored reply that failed to send, is a turn of its own.
    fn next_turn(mut unanswered: Vec<UnansweredMessage>) -> Vec<UnansweredMessage> {
        let end = unanswered
            .iter()
            .position(UnansweredMessage::alone)
            .unwrap_or(usize::MAX);
        unanswered.truncate(end.max(1));
        unanswered
    }

    /// Returns the stored unsent reply, or makes and stores a new one. The reply belongs to the
    /// last text of the burst and is empty if the model only reacted. The other texts are marked
    /// answered.
    async fn response_for(&self, burst: &[UnansweredMessage], last_try: bool) -> Result<String> {
        let (message, earlier) = burst.split_last().expect("a burst has a text");
        if let Some(text) = &message.response {
            return Ok(text.clone());
        }

        let text = if !last_try {
            self.run_agent(burst).await?
        } else if message.is_reminder() {
            message.content.clone()
        } else {
            SORRY.to_string()
        };

        for earlier in earlier {
            self.db.mark_sent(&earlier.event_id)?;
        }
        self.db.save_agent_message(&message.event_id, &text)?;
        Ok(text)
    }

    /// Runs the model on the burst until it stops calling tools or reacts. Text that comes
    /// with tool calls is ignored, since the model writes its reply after it sees the results.
    /// A tapback is the whole reply, so the text is empty.
    async fn run_agent(&self, burst: &[UnansweredMessage]) -> Result<String> {
        let message = burst.last().expect("a burst has a text");
        let history = self.db.load_history(message.id, HISTORY_LIMIT)?;
        let mut context = Agent::context(&history, &self.prompt_context()?);

        // Satchel saves the links and files the user sent to keep, and reads the ones they only
        // asked about. The model sees this as a `save` call it made, so it knows what was saved.
        let texts: Vec<&str> = burst.iter().map(|text| text.content.as_str()).collect();
        let links = texts.iter().flat_map(|text| Link::find_all(text)).collect();
        let (links, asked) = self.agent.keep(&texts, links).await?;
        let saved = !links.is_empty();
        if saved || !asked.is_empty() {
            let urls: Vec<String> = links.iter().map(|link| link.url.to_string()).collect();
            let mut result = "Nothing saved.".to_string();
            if saved {
                for link in &links {
                    match File::parse(link.url.as_str()) {
                        Some(file) => self.db.insert_saved(message.id, &file)?,
                        None => self.db.insert_saved(message.id, link)?,
                    };
                }
                result = self.save(message.id, &urls, None).await?;
                self.react(burst, Tapback::Like).await?;
            }
            for link in &asked {
                let (text, _) = self.reader.read(&link.url, None, &self.agent).await;
                result += &format!(
                    "\nNot saved, since the user only asked about it: {}: {text}",
                    link.url
                );
            }
            result += "\n\nAnswer every other question or request in the newest messages in text.";
            let call = json!({ "name": "save", "arguments": json!({ "urls": urls }).to_string() });
            context.push(json!({ "role": "assistant", "tool_calls": [{ "id": "keep", "type": "function", "function": call }] }));
            Agent::push_results(&mut context, vec![result]);
        }

        let mut step = 0;
        let text = loop {
            let (text, calls) = self
                .agent
                .run_model(&mut context, &message.event_id, step)
                .await?;
            step += 1;
            if calls.is_empty() {
                break text;
            }

            let mut results = Vec::new();
            for call in &calls {
                results.push(match call {
                    Err(error) => format!("error: {error}"),
                    Ok(ToolCall::Save { urls, note }) => {
                        self.save(message.id, urls, note.as_deref()).await?
                    }
                    // A saved burst already has its 👍.
                    Ok(ToolCall::React { emoji }) => {
                        if !saved {
                            self.react(burst, *emoji).await?;
                        }
                        "ok".to_string()
                    }
                    Ok(ToolCall::Remind(remind)) => self.remind(remind, message.id)?,
                });
            }
            let failed = results.iter().any(|result| result.starts_with("error"));
            // A `react` call ends the model's reply, since a tapback means it has nothing to say.
            let reacted = calls
                .iter()
                .any(|c| matches!(c, Ok(ToolCall::React { .. })));
            if !failed && reacted {
                break String::new();
            }
            Agent::push_results(&mut context, results);
        };

        if history.len() == burst.len() {
            return Ok(GREETING.to_string());
        }
        // The model sometimes writes a tapback as text, such as "👍". Send it as a tapback. A
        // reminder turn has no Linq message to react to.
        let tapback = serde_json::from_value::<Tapback>(text.clone().into());
        let (Ok(emoji), false) = (tapback, message.is_reminder()) else {
            return Ok(text);
        };
        if !saved {
            self.react(burst, emoji).await?;
        }
        Ok(String::new())
    }

    /// A tapback answers the whole burst. It goes on every text in it.
    async fn react(&self, burst: &[UnansweredMessage], emoji: Tapback) -> Result<()> {
        for text in burst {
            self.linq.react(&text.event_id, emoji).await?;
        }
        Ok(())
    }

    /// Adds `note` to each saved URL, then reads and embeds it. Without URLs, saves `note` on its
    /// own. Returns each item's search text for the model to check.
    async fn save(&self, message_id: i64, urls: &[String], note: Option<&str>) -> Result<String> {
        let mut items = Vec::new();
        for url in urls {
            let Some(link) = Link::find_all(url).pop() else {
                return Ok(format!("error: {url} is not a URL"));
            };
            let Some(row) = self.db.add_context(link.url.as_str(), note)? else {
                return Ok(format!("error: {url} is not in the messages or saved"));
            };
            let (text, mut media) = self.reader.read(&link.url, row.context, &self.agent).await;
            let vector = match self.agent.embed(&text, media.as_ref()).await {
                Err(error) if media.take().is_some() => {
                    console_error!("embed {url} with its media failed: {error}");
                    self.agent.embed(&text, None).await?
                }
                vector => vector?,
            };
            self.db.set_read(row.id, &text, vector)?;
            let media = media.map_or(String::new(), |(mime, _)| {
                format!(
                    " (plus the {} itself)",
                    mime.split('/').next().unwrap_or_default()
                )
            });
            items.push(format!("- {url}: {text}{media}"));
        }

        if let Some(note) = note.filter(|_| urls.is_empty()) {
            let id = self.db.insert_saved(message_id, &Note(note.to_string()))?;
            let vector = self.agent.embed(note, None).await?;
            self.db.set_read(id, note, vector)?;
            items.push(format!("- note: {note}"));
        }

        if items.is_empty() {
            return Ok("error: nothing to save".to_string());
        }
        Ok(format!("ok. Search text:\n{}", items.join("\n")))
    }

    fn remind(&self, remind: &Remind, message_id: i64) -> Result<String> {
        let saved_zone = self.time_zone()?;
        if let (Some(zone), None) = (&remind.at_zone, &saved_zone) {
            if let Some(name) = zone.iana_name() {
                self.db.set_time_zone(name)?;
            }
            return Ok(format!(
                "error: the time zone is saved, but `at` was worked out from UTC. {} Work out `at` again from this time and call remind again.",
                Self::now_line(Some(zone))
            ));
        }

        if remind.schedule.due_at <= Timestamp::now().as_second() {
            return Ok("error: that time has already passed".to_string());
        }

        let (id, reply) = match self.db.reminder_named(&remind.note)? {
            Some(old) => {
                let zone = remind
                    .at_zone
                    .clone()
                    .or(saved_zone)
                    .unwrap_or(TimeZone::UTC);
                (
                    old.id,
                    format!(
                        "ok, moved the earlier \"{}\" reminder from {} to {}. Tell the user you changed it.",
                        remind.note,
                        Self::local_time(&old.schedule, &zone)?,
                        Self::local_time(&remind.schedule, &zone)?
                    ),
                )
            }
            None => (
                self.db
                    .insert_saved(message_id, &Note(remind.note.clone()))?,
                "ok".to_string(),
            ),
        };

        self.db.set_reminder(id, Some(&remind.schedule))?;
        if let Some(name) = remind.at_zone.as_ref().and_then(TimeZone::iana_name) {
            self.db.set_time_zone(name)?;
        }

        Ok(reply)
    }

    fn time_zone(&self) -> Result<Option<TimeZone>> {
        let Some(name) = self.db.time_zone()? else {
            return Ok(None);
        };
        Ok(Some(
            TimeZone::get(&name).map_err(|error| error.to_string())?,
        ))
    }

    fn local_time(schedule: &Schedule, zone: &TimeZone) -> Result<String> {
        Ok(schedule
            .due(zone)?
            .strftime("%a %b %-d %-I:%M %p %Z")
            .to_string())
    }

    fn prompt_context(&self) -> Result<String> {
        let time_zone = self.time_zone()?;
        let mut text = Self::now_line(time_zone.as_ref());
        let reminders = self.db.reminders()?;
        if !reminders.is_empty() {
            let zone = time_zone.unwrap_or(TimeZone::UTC);
            text.push_str("\nPending reminders:");
            for reminder in reminders {
                let due = Self::local_time(&reminder.schedule, &zone)?;
                text.push_str(&format!("\n- \"{}\" ({due})", reminder.content));
            }
        }
        Ok(text)
    }

    fn now_line(time_zone: Option<&TimeZone>) -> String {
        let now = Timestamp::now().to_zoned_datetime(time_zone.cloned().unwrap_or(TimeZone::UTC));
        let mut line = format!("Now: {}.", now.strftime("%A, %Y-%m-%d %H:%M:%S in %Q"));
        if time_zone.is_none() {
            line.push_str(" The user's time zone is unknown.");
        }
        line
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
}

impl UnansweredMessage {
    fn is_reminder(&self) -> bool {
        self.event_id.starts_with(REMINDER_EVENT_PREFIX)
    }

    fn alone(&self) -> bool {
        self.response.is_some() || self.is_reminder()
    }
}
