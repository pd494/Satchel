use crate::agent::Agent;
use crate::db::{Db, Pending};
use crate::linq::{LinqClient, OutboundMessage, Part};
use crate::media::{Image, Link, Video};
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
                    if self.save(&message).await? {
                        return Ok(Reply::Tapback);
                    }
                    if let Some(media) = requested_media(&message.content) {
                        if let Some(url) = self.db.latest_saved(media)? {
                            return Ok(Reply::Text(url));
                        }
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
                    let sent = self.send_text(space_id, &message.event_id, &text).await;
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

    /// Sends each part of `text` as its own message. Linq requires a link or media part to be
    /// the only part. A saved photo or video URL goes back as the file itself.
    async fn send_text(&self, space_id: &str, event_id: &str, text: &str) -> Result<()> {
        let (rest, urls) = Link::split(text);
        let text = (!rest.is_empty()).then_some(Part::Text(rest));
        let files = urls.into_iter().map(|url| {
            let id = Image::parse(&url)
                .map(|image| image.id)
                .or_else(|| Video::parse(&url).map(|video| video.id));
            id.map_or(Part::Link(url), Part::Media)
        });
        for (i, part) in text.into_iter().chain(files).enumerate() {
            let outbound = OutboundMessage {
                space_id: space_id.to_string(),
                part,
                idempotency_key: format!("{event_id}:{i}"),
            };
            self.linq.send(&outbound).await?;
        }
        Ok(())
    }

    async fn save(&self, message: &Pending) -> Result<bool> {
        let mut found = Vec::new();
        let mut saved_file = false;
        for link in Link::find_all(&message.content) {
            if let Some(image) = Image::parse(link.url.as_str()) {
                self.db.insert_saved(message.id, &image)?;
                saved_file = true;
            } else if let Some(video) = Video::parse(link.url.as_str()) {
                self.db.insert_saved(message.id, &video)?;
                saved_file = true;
            } else {
                found.push(link);
            }
        }
        if found.is_empty() {
            return Ok(saved_file);
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
        Ok(saved_file || !links.is_empty())
    }
}

/// The saved kind a text asks to get back: "send me that picture" is an image, "that video" a video.
fn requested_media(text: &str) -> Option<&'static str> {
    let text = text.to_lowercase();
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).collect();
    let has = |options: &[&str]| words.iter().any(|word| options.contains(word));
    if !has(&["send", "show", "give", "resend"]) && !has(&["that", "the", "my"]) {
        return None;
    }
    if has(&["video", "vid", "clip"]) {
        Some("video")
    } else if has(&[
        "picture", "pic", "photo", "image", "pics", "pictures", "photos",
    ]) {
        Some("image")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::requested_media;

    #[test]
    fn maps_requests_to_the_latest_kind() {
        assert_eq!(requested_media("send me that picture"), Some("image"));
        assert_eq!(requested_media("that photo"), Some("image"));
        assert_eq!(requested_media("the pic"), Some("image"));
        assert_eq!(requested_media("send me that video"), Some("video"));
        assert_eq!(requested_media("lets go on a picnic"), None);
        assert_eq!(requested_media("send me the zillow link"), None);
    }
}
