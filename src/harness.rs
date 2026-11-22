use crate::agent::{Agent, Calls};
use crate::db::{Db, UnansweredMessage};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Note};
use crate::tools::{Remind, Tapback, ToolCall};
use jiff::{tz::TimeZone, Timestamp};
use std::time::Duration;
use worker::{console_error, Env, Result, SqlStorage};

/// Past messages the model sees with each new message.
const HISTORY_LIMIT: usize = 40;

/// Runs before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const REMINDER_EVENT_PREFIX: &str = "reminder:";

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
            agent: Agent::from_env(env)?,
            linq: LinqClient::from_env(env)?,
        })
    }

    /// Saves an inbound text so `inbox` replies to it later.
    pub fn receive(&self, event_id: &str, text: &str) -> Result<()> {
        self.db.save_user_message(event_id, text)?;
        Ok(())
    }

    fn queue_due_reminders(&self) -> Result<()> {
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let now = Timestamp::now().as_second();
        let due = self
            .db
            .reminders()?
            .into_iter()
            .take_while(|reminder| reminder.schedule.due_at <= now);

        for reminder in due {
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
    /// and it is marked sent even if that send fails. Returns how long until the next
    /// reminder, or `None` if none is pending.
    pub async fn inbox(&self, space_id: &str) -> Result<Option<Duration>> {
        // Show the user that Satchel read the chat. A failure here does not stop the replies.
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }

        self.queue_due_reminders()?;

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

        let now = Timestamp::now().as_second();
        Ok(self.db.reminders()?.first().map(|reminder| {
            Duration::from_secs(reminder.schedule.due_at.saturating_sub(now).max(0) as u64)
        }))
    }

    /// Returns the stored unsent reply, or makes and stores a new one. The reply is empty if
    /// the model only reacted.
    async fn response_for(&self, message: &UnansweredMessage, last_try: bool) -> Result<String> {
        if let Some(text) = &message.response {
            return Ok(text.clone());
        }

        let text = if !last_try {
            self.run_agent(message).await?
        } else if message.event_id.starts_with(REMINDER_EVENT_PREFIX) {
            message.content.clone()
        } else {
            SORRY.to_string()
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
        let state = self.prompt_context()?;
        let mut context = Agent::context(&history, &state);
        let mut saved = false;

        let text = loop {
            let (text, calls) = self.agent.run_model(&mut context).await?;
            if calls.is_empty() {
                break text;
            }

            let results = self.run_tools(&calls, message, &links, &mut saved).await?;
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
        saved: &mut bool,
    ) -> Result<Vec<String>> {
        // Saves run first. A 👍 then goes out only after a save works.
        let mut results = vec![String::new(); calls.len()];
        for (result, call) in results.iter_mut().zip(calls) {
            if let Ok(ToolCall::Save { urls, note }) = call {
                *result = self.save(message.id, urls, note.as_deref(), links)?;
                if *result == "ok" && !urls.is_empty() {
                    *saved = true;
                }
            }
        }
        for (result, call) in results.iter_mut().zip(calls) {
            *result = match call {
                Ok(ToolCall::Save { .. }) => continue,
                Err(error) => format!("error: {error}"),
                Ok(ToolCall::React {
                    emoji: Tapback::Like,
                }) if !links.is_empty() && !*saved => {
                    "error: save the links or files from the newest message before reacting with 👍"
                        .to_string()
                }
                Ok(ToolCall::React { emoji }) => {
                    self.linq.react(&message.event_id, *emoji).await?;
                    "ok".to_string()
                }
                Ok(ToolCall::Remind(remind)) => self.remind(remind, message.id)?,
            };
        }
        Ok(results)
    }

    fn save(
        &self,
        message_id: i64,
        urls: &[String],
        note: Option<&str>,
        links: &[Link],
    ) -> Result<String> {
        for url in urls {
            let found = Link::find_all(url)
                .pop()
                .and_then(|wanted| links.iter().find(|link| link.url == wanted.url));
            let Some(link) = found else {
                return Ok(format!("error: {url} is not in the newest message"));
            };

            match File::parse(link.url.as_str()) {
                Some(file) => self.db.insert_saved(message_id, &file)?,
                None => self.db.insert_saved(message_id, link)?,
            };
        }

        if let Some(note) = note {
            self.db.insert_saved(message_id, &Note(note.to_string()))?;
        }

        Ok("ok".to_string())
    }

    fn remind(&self, remind: &Remind, message_id: i64) -> Result<String> {
        if remind.at && self.db.time_zone()?.is_none() {
            let zone = remind
                .time_zone
                .as_ref()
                .ok_or_else(|| "time_zone is required with at".to_string())?;
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

        let earlier = self
            .db
            .reminders()?
            .into_iter()
            .filter(|reminder| reminder.content.eq_ignore_ascii_case(&remind.note))
            .max_by_key(|reminder| reminder.id);

        let (id, reply) = match earlier {
            Some(old) => {
                let zone = match &remind.time_zone {
                    Some(zone) => zone.clone(),
                    None => self.time_zone()?.unwrap_or(TimeZone::UTC),
                };
                (
                    old.id,
                    format!(
                        "ok, moved the earlier \"{}\" reminder from {} to {}. Tell the user you changed it.",
                        remind.note,
                        Self::local_time(old.schedule.due_at, &zone)?,
                        Self::local_time(remind.schedule.due_at, &zone)?
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
        if remind.at {
            if let Some(name) = remind.time_zone.as_ref().and_then(TimeZone::iana_name) {
                self.db.set_time_zone(name)?;
            }
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

    fn local_time(at: i64, zone: &TimeZone) -> Result<String> {
        let at = Timestamp::from_second(at).map_err(|error| error.to_string())?;
        Ok(at
            .to_zoned_datetime(zone.clone())
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
                let due = Self::local_time(reminder.schedule.due_at, &zone)?;
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
