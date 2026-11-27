use crate::agent::Agent;
use crate::db::{Db, Role, Row, Saved, Setting, SettingChange, Summary, Turn, UnansweredMessage};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Note, Reader};
use crate::tools::{Remind, Search, Tapback, ToolCall, ToolResult};
use jiff::{tz::TimeZone, Timestamp};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;
use worker::{console_error, Env, Result, SqlStorage};

const SYSTEM_PROMPT: &str = include_str!("prompts/system_prompt.md");
const COMPACT_PROMPT: &str = include_str!("prompts/compact.md");
const COMPACT_AT_TOKENS: usize = 4_000;
const KEEP_RECENT: usize = 10;

/// Runs before a message gets `SORRY` instead.
const MAX_ATTEMPTS: i64 = 3;

const SORRY: &str = "I couldn't save that. Send it again if you want it saved.";

const REMINDER_EVENT_PREFIX: &str = "reminder:";

/// Seconds Satchel waits after a text for more texts. Texts sent together then get one reply.
const DEBOUNCE: i64 = 5;
/// The most model calls in one reply. It stops a model that keeps calling tools.
const MAX_STEPS: usize = 6;

const SEARCH_RESULTS: usize = 10;
const RRF_K: f32 = 60.0;
const SNIPPET: usize = 300;

/// Sits next to the newest messages. The same rule in the system prompt alone does not make the
/// model search.
const SEARCH_FIRST: &str = "Before you say you don't have or don't know something, call `search`.";

const NOT_PENDING: &str = "that reminder already fired or was canceled.";

const GREETING: &str =
    "Hey, I'm Satchel! Text me links, photos, or notes and I'll save them. Ask me anytime to find something.";

/// The model's JSON from the compact prompt.
#[derive(Deserialize)]
struct Compaction {
    summary: String,
    facts: Vec<Fact>,
}

#[derive(Deserialize)]
struct Fact {
    key: String,
    value: String,
}

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

    /// Replies to every unanswered message.
    /// Returns how long until the next reminder or waiting burst, or `None` if none is pending.
    pub async fn inbox(&self, space_id: &str) -> Result<Option<Duration>> {
        // Show the user that Satchel read the chat. A failure here does not stop the replies.
        if let Err(error) = self.linq.mark_read(space_id).await {
            console_error!("mark read failed: {error}");
        }

        self.queue_due_reminders()?;

        let mut burst_due_at = self.reply_all(space_id).await?;
        if burst_due_at.is_none() {
            if let Err(error) = self.compact().await {
                console_error!("compaction failed: {error}");
            }
            burst_due_at = self.reply_all(space_id).await?;
        }

        // Wake for the next reminder or the waiting burst, whichever comes first.
        let wake_at = match (self.db.next_due()?, burst_due_at) {
            (Some(reminder), Some(burst)) => reminder.min(burst),
            (Some(reminder), None) => reminder,
            (None, Some(burst)) => burst,
            (None, None) => return Ok(None),
        };
        Ok(Some(Self::until(wake_at)))
    }

    /// Replies to every burst it can. Stops at a burst whose newest text is under `DEBOUNCE`
    /// seconds old, and returns the time that burst can be answered. Returns `None` when every
    /// text has its reply.
    async fn reply_all(&self, space_id: &str) -> Result<Option<i64>> {
        let mut burst_due_at = None;
        loop {
            let burst = Self::next_turn(self.db.unanswered()?);
            let Some(message) = burst.last() else {
                break;
            };
            // A text that arrived while this alarm ran waits out its own debounce.
            let debounced = message.created_at + DEBOUNCE;
            if !message.alone() && debounced > Timestamp::now().as_second() {
                burst_due_at = Some(debounced);
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
        Ok(burst_due_at)
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
            self.run_agent(burst, message).await?
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
    /// with tool calls is ignored. The model writes its reply after it sees the results. A
    /// tapback is the whole reply, and its text is empty.
    async fn run_agent(
        &self,
        burst: &[UnansweredMessage],
        message: &UnansweredMessage,
    ) -> Result<String> {
        let summary = self.db.summary()?;
        let through = summary.as_ref().map(|summary| summary.through);
        let chat = self.db.chat(through, Some(message.turn()))?;
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let mut context = Self::context(
            &self.db.settings()?,
            summary.as_ref(),
            &chat,
            &self.prompt_context()?,
            &zone,
        )?;
        let saved = self.handle_burst(burst, message, &mut context).await?;

        let mut step = 0;
        let text = loop {
            let (text, calls) = self
                .agent
                .run_model(
                    &mut context,
                    json!({ "event_id": message.event_id, "step": step }),
                    None,
                )
                .await?;
            step += 1;
            if calls.is_empty() || step == MAX_STEPS {
                break text;
            }

            let mut results = Vec::new();
            for call in &calls {
                results.push(self.run_tool(call, burst, message, saved).await?);
            }
            // A `react` call ends the model's reply. A tapback means it has nothing to say.
            let failed = results
                .iter()
                .any(|result| matches!(result, ToolResult::Error(_)));
            let reacted = calls
                .iter()
                .any(|call| matches!(call, Ok(ToolCall::React { .. })));
            if reacted && !failed {
                break String::new();
            }
            Agent::push_results(&mut context, results);
        };

        let first_message = summary.is_none() && chat.len() == burst.len();
        self.finish(text, burst, message, first_message, saved)
            .await
    }

    async fn handle_burst(
        &self,
        burst: &[UnansweredMessage],
        message: &UnansweredMessage,
        context: &mut Vec<Value>,
    ) -> Result<bool> {
        let texts: Vec<&str> = burst.iter().map(|text| text.content.as_str()).collect();
        let links = Self::burst_links(&texts, message.is_reminder());
        let (links_to_save, links_to_read) = self.agent.save_or_read(&texts, links).await?;
        let saved = !links_to_save.is_empty();
        if !saved && links_to_read.is_empty() {
            return Ok(false);
        }

        let urls: Vec<String> = links_to_save
            .iter()
            .map(|link| link.url.to_string())
            .collect();
        let mut result = "Nothing saved.".to_string();
        if saved {
            let note = Self::burst_words(&texts, &links_to_read);
            result = self
                .save(message.id, &urls, note.as_deref())
                .await?
                .into_text();
            self.react(burst, Tapback::Like).await?;
        }
        let (read_text, images) = self.read_links(&links_to_read).await;
        result += &read_text;
        result += "\n\nAnswer every other question or request in the newest messages in text.";

        Self::push_save_call(context, &urls, result, &images);
        Ok(saved)
    }

    /// The links Clef may keep. A reminder's links were saved before. A URL sent twice counts
    /// once.
    fn burst_links(texts: &[&str], is_reminder: bool) -> Vec<Link> {
        if is_reminder {
            return Vec::new();
        }
        let mut seen = HashSet::new();
        let mut links = Vec::new();
        for text in texts {
            for link in Link::find_all(text) {
                if !link.suspicious && seen.insert(link.url.clone()) {
                    links.push(link);
                }
            }
        }
        links
    }

    /// The user's words in the burst, without the URLs. They become each saved link's context.
    /// A text with a link to read is skipped. Its words are a question about that link.
    fn burst_words(texts: &[&str], links_to_read: &[Link]) -> Option<String> {
        let mut words = Vec::new();
        for text in texts {
            let (rest, urls) = Link::split(text);
            let asks_about_a_link = links_to_read
                .iter()
                .any(|link| urls.contains(&link.url.to_string()));
            if !rest.is_empty() && !asks_about_a_link {
                words.push(rest);
            }
        }
        if words.is_empty() {
            None
        } else {
            Some(words.join("\n"))
        }
    }

    /// Reads each link the user only asked about. Returns a line for each one, and the photos
    /// among them.
    async fn read_links(&self, links: &[Link]) -> (String, Vec<(String, Vec<u8>)>) {
        let mut text = String::new();
        let mut images = Vec::new();
        for link in links {
            let (page, media) = self.reader.read(&link.url, None, &self.agent).await;
            if let Some((mime, bytes)) = media {
                if mime.starts_with("image/") {
                    images.push((mime, bytes));
                }
            }
            text += &format!(
                "\nNot saved, since the user only asked about it: {}: {page}",
                link.url
            );
        }
        (text, images)
    }

    /// Adds a `save` call and its result to the context, as if the model had made the call.
    /// The model then knows what was saved. The photos the user asked about follow.
    fn push_save_call(
        context: &mut Vec<Value>,
        urls: &[String],
        result: String,
        images: &[(String, Vec<u8>)],
    ) {
        let call = json!({ "name": "save", "arguments": json!({ "urls": urls }).to_string() });
        context.push(json!({ "role": "assistant", "tool_calls": [{ "id": "keep", "type": "function", "function": call }] }));
        Agent::push_results(context, vec![ToolResult::Done(result)]);
        if !images.is_empty() {
            let content: Vec<Value> = images
                .iter()
                .map(|(mime, bytes)| json!({ "type": "image_url", "image_url": { "url": Agent::data_url(mime, bytes) } }))
                .collect();
            context.push(json!({ "role": "user", "content": content }));
        }
    }

    async fn run_tool(
        &self,
        call: &std::result::Result<ToolCall, String>,
        burst: &[UnansweredMessage],
        message: &UnansweredMessage,
        saved: bool,
    ) -> Result<ToolResult> {
        let call = match call {
            Ok(call) => call,
            Err(error) => return Ok(ToolResult::Error(error.clone())),
        };
        match call {
            ToolCall::Save { urls, note } => self.save(message.id, urls, note.as_deref()).await,
            ToolCall::React { emoji } => {
                if !saved && !message.is_reminder() {
                    self.react(burst, *emoji).await?;
                }
                Ok(ToolResult::Done("ok".to_string()))
            }
            ToolCall::Remind(remind) => self.remind(remind, message.id),
            ToolCall::Search(search) => self.search(search, burst[0].turn()).await,
            ToolCall::Cancel { id } => self.cancel(*id),
            ToolCall::Settings { key, value } => self.settings(key, value),
        }
    }

    async fn finish(
        &self,
        text: String,
        burst: &[UnansweredMessage],
        message: &UnansweredMessage,
        first_message: bool,
        saved: bool,
    ) -> Result<String> {
        if first_message {
            return Ok(GREETING.to_string());
        }
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
    async fn save(
        &self,
        message_id: i64,
        urls: &[String],
        note: Option<&str>,
    ) -> Result<ToolResult> {
        let mut items = Vec::new();
        for url in urls {
            let Some(link) = Link::find_all(url).pop() else {
                return Ok(ToolResult::Error(format!("{url} is not a URL")));
            };
            match File::parse(link.url.as_str()) {
                Some(file) => self.db.insert_saved(message_id, &file)?,
                None => self.db.insert_saved(message_id, &link)?,
            };
            let Some(row) = self.db.add_context(link.url.as_str(), note)? else {
                return Ok(ToolResult::Error(format!("{url} is not saved")));
            };
            let (text, mut media) = self.reader.read(&link.url, row.context, &self.agent).await;
            let vector = match self
                .agent
                .embed(Agent::SAVED_ITEM, &text, media.as_ref())
                .await
            {
                Err(error) if media.take().is_some() => {
                    console_error!("embed {url} with its media failed: {error}");
                    self.agent.embed(Agent::SAVED_ITEM, &text, None).await?
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
            let vector = self.agent.embed(Agent::SAVED_ITEM, note, None).await?;
            self.db.set_read(id, note, vector)?;
            items.push(format!("- note: {note}"));
        }

        if items.is_empty() {
            return Ok(ToolResult::Error("nothing to save".to_string()));
        }
        Ok(ToolResult::Done(format!(
            "ok. Search text:\n{}",
            items.join("\n")
        )))
    }

    fn remind(&self, remind: &Remind, message_id: i64) -> Result<ToolResult> {
        let saved_zone = self.time_zone()?;
        let now = Timestamp::now().as_second();
        if let (Some(zone), None) = (&remind.at_zone, &saved_zone) {
            self.db
                .change_setting(&SettingChange::TimeZone(zone.clone()), now)?;
            return Ok(ToolResult::Error(format!(
                "the time zone is saved, but `at` was worked out from UTC. {} Work out `at` again from this time and call remind again.",
                Self::now_line(Some(zone))
            )));
        }

        if remind.schedule.due_at <= now {
            return Ok(ToolResult::Error(
                "that time has already passed".to_string(),
            ));
        }

        let id = match remind.id {
            Some(id) => id,
            None => self
                .db
                .insert_saved(message_id, &Note(remind.note.clone()))?,
        };
        let reply = match self.db.reminder(id)? {
            Some(old) => {
                let zone = remind
                    .at_zone
                    .clone()
                    .or(saved_zone)
                    .unwrap_or(TimeZone::UTC);
                format!(
                    "ok, moved the \"{}\" reminder from {} to {}. Tell the user you changed it.",
                    old.content,
                    Self::local_time(old.schedule.due_at, &zone)?,
                    Self::local_time(remind.schedule.due_at, &zone)?
                )
            }
            None if remind.id.is_some() => {
                return Ok(ToolResult::Error(NOT_PENDING.to_string()));
            }
            None => "ok".to_string(),
        };

        self.db.set_reminder(id, Some(&remind.schedule))?;
        if let Some(zone) = &remind.at_zone {
            self.db
                .change_setting(&SettingChange::TimeZone(zone.clone()), now)?;
        }
        Ok(ToolResult::Done(reply))
    }

    fn time_zone(&self) -> Result<Option<TimeZone>> {
        let Some(name) = self.db.time_zone()? else {
            return Ok(None);
        };
        Ok(Some(
            TimeZone::get(&name).map_err(|error| error.to_string())?,
        ))
    }

    fn cancel(&self, id: i64) -> Result<ToolResult> {
        let Some(reminder) = self.db.reminder(id)? else {
            return Ok(ToolResult::Error(NOT_PENDING.to_string()));
        };
        self.db.set_reminder(id, None)?;
        Ok(ToolResult::Done(format!(
            "ok, canceled the \"{}\" reminder. Tell the user.",
            reminder.content
        )))
    }

    fn settings(&self, key: &str, value: &str) -> Result<ToolResult> {
        let change = match SettingChange::parse(key, value) {
            Ok(change) => change,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        self.db
            .change_setting(&change, Timestamp::now().as_second())?;
        let reply = match &change {
            SettingChange::TimeZone(zone) => format!("ok. {}", Self::now_line(Some(zone))),
            SettingChange::Set { .. } => "ok".to_string(),
            SettingChange::Remove { key } => format!("ok, removed {key}"),
        };
        Ok(ToolResult::Done(reply))
    }

    fn context(
        settings: &[Setting],
        summary: Option<&Summary>,
        chat: &[Row],
        now: &str,
        zone: &TimeZone,
    ) -> Result<Vec<Value>> {
        let mut system_prompt = format!(
            "{SYSTEM_PROMPT}\n\n# User settings\n\n{}",
            Self::settings_text(settings, zone)?
        );
        if let Some(summary) = summary {
            system_prompt.push_str(&format!("\n\n# Earlier in this chat\n\n{}", summary.text));
        }
        let mut context = vec![json!({ "role": "system", "content": system_prompt })];
        for row in chat {
            context.push(json!({ "role": row.role, "content": row.content }));
        }
        context.push(json!({ "role": "system", "content": now }));
        Ok(context)
    }

    fn settings_text(settings: &[Setting], zone: &TimeZone) -> Result<String> {
        let mut lines = Vec::new();
        for setting in settings {
            let said = Timestamp::from_second(setting.said_at)
                .map_err(|error| error.to_string())?
                .to_zoned_datetime(zone.clone())
                .strftime("%Y-%m-%d");
            lines.push(format!(
                "- {}: {} (said {said})",
                setting.key, setting.value
            ));
        }
        if lines.is_empty() {
            return Ok("None yet.".to_string());
        }
        Ok(lines.join("\n"))
    }

    fn transcript_line(row: &Row, zone: &TimeZone, max_chars: Option<usize>) -> Result<String> {
        let sent = Timestamp::from_second(row.created_at)
            .map_err(|error| error.to_string())?
            .to_zoned_datetime(zone.clone())
            .strftime("%a %b %-d %Y %-I:%M %p")
            .to_string();
        let who = match row.role {
            Role::User => "user",
            Role::Assistant => "Satchel",
        };
        let one_line = row.content.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            return Ok(format!("- {sent}, {who}: (tapback)"));
        }
        let text = match max_chars {
            Some(max_chars) => one_line.chars().take(max_chars).collect(),
            None => one_line,
        };
        Ok(format!("- {sent}, {who}: {text}"))
    }

    /// When the summary and the chat after it pass `COMPACT_AT_TOKENS`, moves the oldest turns
    /// into the summary. The newest `KEEP_RECENT` messages stay. Otherwise it does nothing.
    async fn compact(&self) -> Result<()> {
        let summary = self.db.summary()?;
        let chat_after_summary = self
            .db
            .chat(summary.as_ref().map(|summary| summary.through), None)?;

        // Size: about 4 characters a token.
        let mut chars = summary
            .as_ref()
            .map_or(0, |summary| summary.text.chars().count());
        for row in &chat_after_summary {
            chars += row.content.chars().count();
        }
        if chars / 4 < COMPACT_AT_TOKENS || chat_after_summary.len() <= KEEP_RECENT {
            return Ok(());
        }

        // The oldest rows, up to the turn of the first kept row. A reply stays with its text.
        let first_kept = chat_after_summary[chat_after_summary.len() - KEEP_RECENT].turn;
        let mut messages_before_compaction = Vec::new();
        for row in chat_after_summary {
            if row.turn >= first_kept {
                break;
            }
            messages_before_compaction.push(row);
        }
        if messages_before_compaction.is_empty() {
            return Ok(());
        }

        // One model call with the compact prompt returns the new summary and the facts.
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let mut transcript = Vec::new();
        for row in &messages_before_compaction {
            transcript.push(Self::transcript_line(row, &zone, None)?);
        }
        let input = format!(
            "Summary so far:\n{}\n\nUser settings:\n{}\n\nNew messages:\n{}",
            summary
                .as_ref()
                .map_or("(none)", |summary| summary.text.as_str()),
            Self::settings_text(&self.db.settings()?, &zone)?,
            transcript.join("\n")
        );
        let mut context = vec![
            json!({ "role": "system", "content": COMPACT_PROMPT }),
            json!({ "role": "user", "content": input }),
        ];
        // summary: the new summary of all chat up to the last compacted message.
        // facts: the setting changes in the compacted messages, each a key and a value.
        let response_schema = json!({
            "type": "object",
            "required": ["summary", "facts"],
            "properties": {
                "summary": { "type": "string" },
                "facts": { "type": "array", "items": {
                    "type": "object",
                    "required": ["key", "value"],
                    "properties": { "key": { "type": "string" }, "value": { "type": "string" } },
                }},
            },
        });
        let (text, _) = self
            .agent
            .run_model(
                &mut context,
                json!({ "step": "compact" }),
                Some(&response_schema),
            )
            .await?;
        let compaction: Compaction = serde_json::from_str(&text)?;

        // A fact that fails to parse is logged and skipped. An empty summary writes nothing.
        let new_summary = compaction.summary.trim();
        if new_summary.is_empty() {
            return Err("the compaction returned an empty summary".into());
        }
        let mut facts = Vec::new();
        for fact in compaction.facts {
            match SettingChange::parse(&fact.key, &fact.value) {
                Ok(change) => facts.push(change),
                Err(error) => console_error!("compaction fact {}: {error}", fact.key),
            }
        }
        if !self
            .db
            .save_compaction(&messages_before_compaction, new_summary, &facts)?
        {
            console_error!("a reset deleted the chat during compaction");
        }
        Ok(())
    }

    async fn search(&self, search: &Search, before_turn: Turn) -> Result<ToolResult> {
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let (after, before) = match search.bounds(&zone) {
            Ok(bounds) => bounds,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        let query = search.query.as_deref().map(str::trim);
        let query = query.filter(|query| !query.is_empty());

        let mut found = Vec::new();
        let matching_saved = self.search_saved(query, after, before, &zone).await?;
        if !matching_saved.is_empty() {
            found.push(format!("Saved items:\n{}", matching_saved.join("\n")));
        }
        let mut matching_chat = Vec::new();
        for excerpt in self.db.search_chat(query, after, before, before_turn)? {
            let mut lines = Vec::new();
            for row in &excerpt {
                lines.push(Self::transcript_line(row, &zone, Some(SNIPPET))?);
            }
            matching_chat.push(lines.join("\n"));
        }
        if !matching_chat.is_empty() {
            found.push(format!("Past chat:\n{}", matching_chat.join("\n\n")));
        }
        if found.is_empty() {
            return Ok(ToolResult::Done("Nothing matches.".to_string()));
        }
        Ok(ToolResult::Done(found.join("\n\n")))
    }

    async fn search_saved(
        &self,
        query: Option<&str>,
        after: Option<i64>,
        before: Option<i64>,
        zone: &TimeZone,
    ) -> Result<Vec<String>> {
        let mut rows = self.db.searchable(after, before)?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // With no query, the newest saves come first.
        if let Some(query) = query {
            let vector = self.agent.embed(Agent::SEARCH, query, None).await?;
            rows = Self::rank(query, &vector, rows);
        }
        rows.truncate(SEARCH_RESULTS);

        // One line per result. A link or file starts with its URL. A note shows its text.
        let mut lines = Vec::new();
        for row in &rows {
            let saved = Timestamp::from_second(row.created_at)
                .map_err(|error| error.to_string())?
                .to_zoned_datetime(zone.clone())
                .strftime("%a %b %-d %Y")
                .to_string();
            let text = row.description.as_deref().unwrap_or_default();
            let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let snippet: String = one_line.chars().take(SNIPPET).collect();
            if row.media == "note" {
                lines.push(format!("- note, saved {saved}: {snippet}"));
            } else {
                lines.push(format!(
                    "- {} ({}, saved {saved}): {snippet}",
                    row.content, row.media
                ));
            }
        }
        Ok(lines)
    }

    fn rank(query: &str, query_vector: &[u8], rows: Vec<Saved>) -> Vec<Saved> {
        // A text's words: lowercase, and longer than 2 characters.
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| !c.is_alphanumeric())
                .filter(|word| word.chars().count() > 2)
                .map(str::to_lowercase)
                .collect()
        };
        // A vector is stored as little-endian `f32` bytes.
        let floats = |bytes: &[u8]| -> Vec<f32> {
            let (chunks, _) = bytes.as_chunks::<4>();
            chunks
                .iter()
                .map(|&chunk| f32::from_le_bytes(chunk))
                .collect()
        };
        let length = |vector: &[f32]| vector.iter().map(|x| x * x).sum::<f32>().sqrt();

        let query_words = words(query);
        let query_vector = floats(query_vector);
        let mut word_hits = Vec::new();
        let mut closeness = Vec::new();
        for row in &rows {
            // Word hits: the query words that start a word in the row's search text.
            let row_words = words(row.description.as_deref().unwrap_or_default());
            let mut hits = 0;
            for word in &query_words {
                if row_words
                    .iter()
                    .any(|row_word| row_word.starts_with(word.as_str()))
                {
                    hits += 1;
                }
            }
            word_hits.push(hits);

            // Closeness: the cosine of the query vector and the row's vector.
            let vector = floats(row.vector.as_deref().unwrap_or_default());
            let dot: f32 = query_vector.iter().zip(&vector).map(|(a, b)| a * b).sum();
            let lengths = (length(&query_vector) * length(&vector)).max(f32::EPSILON);
            closeness.push(dot / lengths);
        }

        // Two ranks of row indexes, best first. A row with no word hit is not in the word rank.
        let mut by_words: Vec<usize> = (0..rows.len()).filter(|&i| word_hits[i] > 0).collect();
        by_words.sort_by(|&a, &b| word_hits[b].cmp(&word_hits[a]));
        let mut by_meaning: Vec<usize> = (0..rows.len()).collect();
        by_meaning.sort_by(|&a, &b| closeness[b].total_cmp(&closeness[a]));

        // Reciprocal rank fusion: each rank gives a row 1 / (RRF_K + its place).
        let mut scores = vec![0.0; rows.len()];
        for ranked in [by_words, by_meaning] {
            for (place, i) in (1..).zip(ranked) {
                scores[i] += 1.0 / (RRF_K + place as f32);
            }
        }

        // Best score first. Ties keep the newer row first.
        let mut scored: Vec<(f32, Saved)> = scores.into_iter().zip(rows).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, row)| row).collect()
    }

    fn local_time(at: i64, zone: &TimeZone) -> Result<String> {
        Ok(Timestamp::from_second(at)
            .map_err(|error| error.to_string())?
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
                let schedule = &reminder.schedule;
                let mut when = Self::local_time(schedule.due_at, &zone)?;
                if let Some(repeat) = schedule.repeat {
                    when.push_str(&format!(", repeat {repeat}"));
                }
                if let Some(until_at) = schedule.until_at {
                    when.push_str(&format!(", until {}", Self::local_time(until_at, &zone)?));
                }
                text.push_str(&format!(
                    "\n- id {}: \"{}\" ({when})",
                    reminder.id, reminder.content
                ));
            }
        }
        text.push('\n');
        text.push_str(SEARCH_FIRST);
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
        let (rest, mut urls) = Link::split(text);
        let mut seen = HashSet::new();
        urls.retain(|url| seen.insert(url.clone()));
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
