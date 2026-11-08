//! The message log: the lines that tell the player what happened, oldest
//! first, with a bounded history.
//!
//! Exposes [`Message`], its [`Tone`], [`MessageLog`] and the size of its
//! history ([`HISTORY_LIMIT`]). This module only stores messages: their
//! wording comes from the `messages` module, and drawing the most recent
//! ones under the map is the job of `ui::log`.

use std::collections::VecDeque;

/// Number of messages the log keeps; older ones are dropped first.
///
/// The game screen shows about five lines, so this keeps twenty screens of
/// history, enough for a future scroll-back view, while bounding memory over
/// a long run (a message is a short sentence).
pub const HISTORY_LIMIT: usize = 100;

/// What kind of news a message brings, which gives its prefix and colour
/// (see `ui::log`): the prefix carries the tone without colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Neutral information, such as a blow the player struck (no prefix).
    Info,
    /// A gain, such as a foe killed (prefix `+`).
    Success,
    /// A danger: damage taken, death, a backlash (prefix `!`).
    Danger,
    /// A spell cast and what it did (prefix `~`).
    Sigil,
}

/// One line of news, in English, as the player reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Kind of news, drawn as a prefix and a colour.
    pub tone: Tone,
    /// The sentence itself, without its prefix; it may be longer than a
    /// screen line, the log wraps it.
    pub text: String,
}

impl Message {
    /// Creates a message of `tone` saying `text`.
    pub fn new(tone: Tone, text: impl Into<String>) -> Self {
        Self {
            tone,
            text: text.into(),
        }
    }
}

/// Every message of the run still kept, oldest first, at most
/// [`HISTORY_LIMIT`] of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageLog {
    /// Kept messages, oldest at the front.
    messages: VecDeque<Message>,
}

impl MessageLog {
    /// Appends `message` as the most recent one, dropping the oldest when
    /// the log already holds [`HISTORY_LIMIT`] messages.
    pub fn push(&mut self, message: Message) {
        if self.messages.len() == HISTORY_LIMIT {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
    }

    /// Appends every message of `messages`, in order, as [`MessageLog::push`]
    /// does.
    pub fn extend(&mut self, messages: impl IntoIterator<Item = Message>) {
        for message in messages {
            self.push(message);
        }
    }

    /// Iterates over the kept messages from the most recent to the oldest,
    /// the order in which the log screen fills its lines.
    pub fn newest_first(&self) -> impl Iterator<Item = &Message> {
        self.messages.iter().rev()
    }

    /// Returns the number of kept messages, at most [`HISTORY_LIMIT`].
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral message saying `number`, to tell messages apart.
    fn numbered(number: usize) -> Message {
        Message::new(Tone::Info, number.to_string())
    }

    #[test]
    fn messages_come_back_newest_first() {
        let mut log = MessageLog::default();
        log.extend((1..=3).map(numbered));

        let texts: Vec<_> = log.newest_first().map(|m| m.text.as_str()).collect();

        assert_eq!(texts, ["3", "2", "1"]);
    }

    #[test]
    fn the_history_keeps_only_the_most_recent_messages() {
        let mut log = MessageLog::default();
        log.extend((1..=HISTORY_LIMIT + 5).map(numbered));

        assert_eq!(log.len(), HISTORY_LIMIT);
        let newest = log.newest_first().next().map(|m| m.text.clone());
        let oldest = log.newest_first().last().map(|m| m.text.clone());
        assert_eq!(newest, Some((HISTORY_LIMIT + 5).to_string()));
        assert_eq!(oldest, Some(6.to_string()), "the five oldest were dropped");
    }
}
