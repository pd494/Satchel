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

/// The most results one search returns.
const SEARCH_RESULTS: usize = 10;
/// The reciprocal rank fusion constant. A larger value weighs the top ranks less.
const RRF_K: f32 = 60.0;
/// The most characters of search text one search result shows.
const SNIPPET: usize = 300;

/// The error for a `remind` or `cancel` id that is not a pending reminder.
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
    /// with tool calls is ignored. The model writes its reply after it sees the results. A
    /// tapback is the whole reply, and its text is empty.
    async fn run_agent(&self, burst: &[UnansweredMessage]) -> Result<String> {
        let message = burst.last().expect("a burst has a text");
        let history = self.db.load_history(message.id, HISTORY_LIMIT)?;
        let mut context = Agent::context(&history, &self.prompt_context()?);
        let saved = self.auto_save(burst, &mut context).await?;

        // The URLs the model may send: the ones in the chat, and the ones a tool returns.
        let mut known: HashSet<String> = history
            .iter()
            .flat_map(|row| Self::urls(&row.content))
            .collect();

        let mut step = 0;
        let text = loop {
            let (text, calls) = self
                .agent
                .run_model(&mut context, &message.event_id, step)
                .await?;
            step += 1;
            if step == MAX_STEPS {
                break text;
            }

            if calls.is_empty() {
                let Some(note) = Self::check_reply(&text, &known) else {
                    break text;
                };
                context.push(json!({ "role": "user", "content": note }));
                continue;
            }

            let mut results = Vec::new();
            for call in &calls {
                let result = self.run_tool(call, burst, saved).await?;
                known.extend(Self::urls(&result));
                results.push(result);
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
        self.finish(text, burst, first_message, saved).await
    }

    /// Saves the links and files the user sent to keep, and reads the ones they only asked
    /// about. The model sees this as its own `save` call. It then knows what was saved. A due
    /// reminder's links were saved before. A URL sent twice counts once. Returns whether
    /// anything was saved.
    async fn auto_save(
        &self,
        burst: &[UnansweredMessage],
        context: &mut Vec<Value>,
    ) -> Result<bool> {
        let message = burst.last().expect("a burst has a text");
        let texts: Vec<&str> = burst.iter().map(|text| text.content.as_str()).collect();
        let mut seen = HashSet::new();
        let links = texts
            .iter()
            .flat_map(|text| Link::find_all(text))
            .filter(|link| {
                !link.suspicious && !message.is_reminder() && seen.insert(link.url.clone())
            })
            .collect();
        let (links, asked) = self.agent.keep(&texts, links).await?;
        let saved = !links.is_empty();
        if !saved && asked.is_empty() {
            return Ok(false);
        }

        let urls: Vec<String> = links.iter().map(|link| link.url.to_string()).collect();
        let mut result = "Nothing saved.".to_string();
        if saved {
            // The user's other words in the burst describe its links. Search reads them.
            let words: Vec<String> = texts
                .iter()
                .map(|text| Link::split(text).0)
                .filter(|rest| !rest.is_empty())
                .collect();
            let note = (!words.is_empty()).then(|| words.join("\n"));
            result = self.save(message.id, &urls, note.as_deref()).await?;
            self.react(burst, Tapback::Like).await?;
        }

        // The model sees the photos the user asked about.
        let mut images = Vec::new();
        for link in &asked {
            let (text, media) = self.reader.read(&link.url, None, &self.agent).await;
            images.extend(media.filter(|(mime, _)| mime.starts_with("image/")));
            result += &format!(
                "\nNot saved, since the user only asked about it: {}: {text}",
                link.url
            );
        }
        result += "\n\nAnswer every other question or request in the newest messages in text.";

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
        Ok(saved)
    }

    /// Runs one tool call and returns its result for the model.
    async fn run_tool(
        &self,
        call: &std::result::Result<ToolCall, String>,
        burst: &[UnansweredMessage],
        saved: bool,
    ) -> Result<String> {
        let message = burst.last().expect("a burst has a text");
        let call = match call {
            Ok(call) => call,
            Err(error) => return Ok(format!("error: {error}")),
        };
        match call {
            ToolCall::Save { urls, note } => self.save(message.id, urls, note.as_deref()).await,
            // A saved burst already has its 👍. A reminder has no text to react to.
            ToolCall::React { emoji } => {
                if !saved && !message.is_reminder() {
                    self.react(burst, *emoji).await?;
                }
                Ok("ok".to_string())
            }
            ToolCall::Remind(remind) => self.remind(remind, message.id),
            ToolCall::Search(search) => self.search(search).await,
            ToolCall::Cancel { id } => self.cancel(*id),
            ToolCall::TimeZone { time_zone } => self.set_time_zone(time_zone),
        }
    }

    /// Returns a note that sends the reply back to the model if it has a URL that is not in
    /// the chat or a tool result. The model may have changed or made up that URL. Returns
    /// `None` if the reply can be sent.
    fn check_reply(text: &str, known: &HashSet<String>) -> Option<String> {
        let unknown: Vec<String> = Self::urls(text)
            .filter(|url| !known.contains(url))
            .collect();
        if unknown.is_empty() {
            return None;
        }
        Some(format!(
            "(Satchel check, not from the user) Your reply was not sent. It has {}, which is not in the chat or a search result. Write your reply to the user's last message again. Copy each URL exactly, or call `search` to find it.",
            unknown.join(", ")
        ))
    }

    /// Returns the text to send. The first message gets the greeting. The model sometimes
    /// writes a tapback as text, such as "👍". That goes on the burst as a tapback, and the
    /// text is empty. A reminder turn has no Linq message to react to.
    async fn finish(
        &self,
        text: String,
        burst: &[UnansweredMessage],
        first_message: bool,
        saved: bool,
    ) -> Result<String> {
        if first_message {
            return Ok(GREETING.to_string());
        }
        let message = burst.last().expect("a burst has a text");
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
                .embed(Agent::DOCUMENT, &text, media.as_ref())
                .await
            {
                Err(error) if media.take().is_some() => {
                    console_error!("embed {url} with its media failed: {error}");
                    self.agent.embed(Agent::DOCUMENT, &text, None).await?
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
            let vector = self.agent.embed(Agent::DOCUMENT, note, None).await?;
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

        // A new reminder with the same note as a pending one gets that row back, and moves it.
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

    /// Finds saved items for the model. Dates are read in the saved time zone, or UTC.
    async fn search(&self, search: &Search) -> Result<String> {
        let zone = self.time_zone()?.unwrap_or(TimeZone::UTC);
        let (after, before) = match search.bounds(&zone) {
            Ok(bounds) => bounds,
            Err(error) => return Ok(format!("error: {error}")),
        };
        let query = search.query.as_deref().map(str::trim).unwrap_or_default();
        let mut rows = self.db.searchable(after, before)?;
        if rows.is_empty() {
            return Ok("No saved items match.".to_string());
        }
        if !query.is_empty() {
            let vector = self.agent.embed(Agent::QUERY, query, None).await?;
            rows = Self::rank(query, &vector, rows);
        }
        rows.truncate(SEARCH_RESULTS);
        let lines: Vec<String> = rows
            .iter()
            .map(|row| Self::search_result(row, &zone))
            .collect();
        Ok(lines.join("\n"))
    }

    /// Ranks `rows` two ways, then merges the two ranks with reciprocal rank fusion, best
    /// first. One rank counts the query words in each row's search text. The other measures
    /// how close each row's vector is to `query_vector`. Ties keep the newer row first.
    fn rank(query: &str, query_vector: &[u8], rows: Vec<Saved>) -> Vec<Saved> {
        let query_words: Vec<String> = Self::words(query).collect();
        let query_vector = Self::floats(query_vector);

        let mut word_hits = Vec::new();
        let mut closeness = Vec::new();
        for row in &rows {
            let text = row.description.as_deref().unwrap_or_default();
            let row_words: Vec<String> = Self::words(text).collect();
            let hits = query_words
                .iter()
                .filter(|word| row_words.iter().any(|seen| seen.starts_with(word.as_str())))
                .count();
            word_hits.push(hits);

            let vector = Self::floats(row.vector.as_deref().unwrap_or_default());
            closeness.push(Self::cosine(&query_vector, &vector));
        }

        // Row indexes, best first. A row with no word hit is not in the word rank.
        let mut by_words: Vec<usize> = (0..rows.len()).filter(|&i| word_hits[i] > 0).collect();
        by_words.sort_by(|&a, &b| word_hits[b].cmp(&word_hits[a]));
        let mut by_meaning: Vec<usize> = (0..rows.len()).collect();
        by_meaning.sort_by(|&a, &b| closeness[b].total_cmp(&closeness[a]));

        // Each rank gives a row 1 / (RRF_K + its place). Place 1 is the best.
        let mut scores = vec![0.0; rows.len()];
        for ranked in [by_words, by_meaning] {
            for (place, i) in (1..).zip(ranked) {
                scores[i] += 1.0 / (RRF_K + place as f32);
            }
        }

        let mut scored: Vec<(f32, Saved)> = scores.into_iter().zip(rows).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, row)| row).collect()
    }

    /// One search result for the model. A link or file starts with its URL. A note has none.
    fn search_result(row: &Saved, zone: &TimeZone) -> String {
        let saved = Timestamp::from_second(row.created_at)
            .map(|at| {
                at.to_zoned_datetime(zone.clone())
                    .strftime("%a %b %-d %Y")
                    .to_string()
            })
            .unwrap_or_default();
        // One line, at most `SNIPPET` characters.
        let text = row.description.as_deref().unwrap_or_default();
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let snippet: String = one_line.chars().take(SNIPPET).collect();

        if row.media == "note" {
            return format!("- note, saved {saved}: {snippet}");
        }
        format!(
            "- {} ({}, saved {saved}): {snippet}",
            row.content, row.media
        )
    }

    /// The lowercase words in `text` longer than 2 characters.
    fn words(text: &str) -> impl Iterator<Item = String> + '_ {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() > 2)
            .map(str::to_lowercase)
    }

    /// The `f32` values in little-endian `bytes`.
    fn floats(bytes: &[u8]) -> Vec<f32> {
        let (chunks, _) = bytes.as_chunks::<4>();
        chunks
            .iter()
            .map(|&chunk| f32::from_le_bytes(chunk))
            .collect()
    }

    /// The cosine similarity of `a` and `b`: 1 for the same direction, 0 for unrelated.
    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let length = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (length(a) * length(b)).max(f32::EPSILON)
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

    /// The URLs in `text` that `send_text` sends as links.
    fn urls(text: &str) -> impl Iterator<Item = String> {
        Link::find_all(text)
            .into_iter()
            .filter(|link| !link.suspicious)
            .map(|link| link.url.to_string())
    }

    /// Sends each part of `text` as its own message. Linq requires a link or media part to be
    /// the only part. A URL written twice is sent once.
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
