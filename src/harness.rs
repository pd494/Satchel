use crate::agent::Agent;
use crate::db::{Db, Saved, UnansweredMessage};
use crate::linq::{LinqClient, Part};
use crate::media::{File, Kind, Link, Note, Reader};
use crate::tools::{Remind, Search, Tapback, ToolCall};
use jiff::{tz::TimeZone, Timestamp};
use serde_json::{json, Value};
use std::collections::HashSet;
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
/// The most model calls in one reply. It stops a model that keeps calling tools.
const MAX_STEPS: usize = 6;

const SEARCH_RESULTS: usize = 10;
const RRF_K: f32 = 60.0;
const SNIPPET: usize = 300;

const NOT_PENDING: &str = "error: that reminder already fired or was canceled.";

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
        let history = self.db.load_history(message.id, HISTORY_LIMIT)?;
        let mut context = Agent::context(&history, &self.prompt_context()?);
        let saved = self.handle_burst(burst, message, &mut context).await?;

        let mut step = 0;
        let text = loop {
            let (text, calls) = self
                .agent
                .run_model(&mut context, &message.event_id, step)
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
            let failed = results.iter().any(|result| result.starts_with("error"));
            let reacted = calls
                .iter()
                .any(|call| matches!(call, Ok(ToolCall::React { .. })));
            if reacted && !failed {
                break String::new();
            }
            Agent::push_results(&mut context, results);
        };

        let first_message = history.len() == burst.len();
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
        let (links, asked) = self.agent.keep(&texts, links).await?;
        let saved = !links.is_empty();
        if !saved && asked.is_empty() {
            return Ok(false);
        }

        let urls: Vec<String> = links.iter().map(|link| link.url.to_string()).collect();
        let mut result = "Nothing saved.".to_string();
        if saved {
            let note = Self::burst_words(&texts);
            result = self.save(message.id, &urls, note.as_deref()).await?;
            self.react(burst, Tapback::Like).await?;
        }
        let (asked_text, images) = self.read_asked(&asked).await;
        result += &asked_text;
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

    /// The user's words in the burst, without the URLs. They are saved as the kept links'
    /// context.
    fn burst_words(texts: &[&str]) -> Option<String> {
        let mut words = Vec::new();
        for text in texts {
            let (rest, _) = Link::split(text);
            if !rest.is_empty() {
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
    async fn read_asked(&self, asked: &[Link]) -> (String, Vec<(String, Vec<u8>)>) {
        let mut text = String::new();
        let mut images = Vec::new();
        for link in asked {
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
        Agent::push_results(context, vec![result]);
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
    ) -> Result<String> {
        let call = match call {
            Ok(call) => call,
            Err(error) => return Ok(format!("error: {error}")),
        };
        match call {
            ToolCall::Save { urls, note } => self.save(message.id, urls, note.as_deref()).await,
            ToolCall::React { emoji } => {
                if !saved && !message.is_reminder() {
                    self.react(burst, *emoji).await?;
                }
                Ok("ok".to_string())
            }
            ToolCall::Remind(remind) => self.remind(remind, message.id),
            ToolCall::Search(search) => self.search(search).await,
            ToolCall::Cancel { id } => self.cancel(*id),
            ToolCall::Settings { key, value } => match key.as_str() {
                "time_zone" => self.set_time_zone(value),
                _ => Ok(format!("error: there is no setting named {key}")),
            },
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
    async fn save(&self, message_id: i64, urls: &[String], note: Option<&str>) -> Result<String> {
        let mut items = Vec::new();
        for url in urls {
            let Some(link) = Link::find_all(url).pop() else {
                return Ok(format!("error: {url} is not a URL"));
            };
            match File::parse(link.url.as_str()) {
                Some(file) => self.db.insert_saved(message_id, &file)?,
                None => self.db.insert_saved(message_id, &link)?,
            };
            let Some(row) = self.db.add_context(link.url.as_str(), note)? else {
                return Ok(format!("error: {url} is not saved"));
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
            None if remind.id.is_some() => return Ok(NOT_PENDING.to_string()),
            None => "ok".to_string(),
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

    fn cancel(&self, id: i64) -> Result<String> {
        let Some(reminder) = self.db.reminder(id)? else {
            return Ok(NOT_PENDING.to_string());
        };
        self.db.set_reminder(id, None)?;
        Ok(format!(
            "ok, canceled the \"{}\" reminder. Tell the user.",
            reminder.content
        ))
    }

    fn set_time_zone(&self, name: &str) -> Result<String> {
        let zone = match TimeZone::get(name) {
            Ok(zone) => zone,
            Err(error) => return Ok(format!("error: {error}")),
        };
        self.db.set_time_zone(zone.iana_name().unwrap_or(name))?;
        Ok(format!("ok. {}", Self::now_line(Some(&zone))))
    }

    async fn search(&self, search: &Search) -> Result<String> {
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let (after, before) = match search.bounds(&zone) {
            Ok(bounds) => bounds,
            Err(error) => return Ok(format!("error: {error}")),
        };
        let mut rows = self.db.searchable(after, before)?;
        if rows.is_empty() {
            return Ok("No saved items match.".to_string());
        }

        // With no query, the newest saves come first.
        let query = search.query.as_deref().map(str::trim);
        if let Some(query) = query.filter(|query| !query.is_empty()) {
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
        Ok(lines.join("\n"))
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
