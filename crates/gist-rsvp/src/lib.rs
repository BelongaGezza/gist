//! Pure RSVP pacing engine.
//!
//! No timer, no I/O, no async.  The SwiftUI shell drives it from `CVDisplayLink`
//! by calling [`RsvpSession::token_at_elapsed`] on every frame.

use gist_model::{Token, TokenKind};
use serde::{Deserialize, Serialize};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Words per minute: clamped to 100–1000.  Default 250.
    pub wpm: u32,
    /// Pause multiplier for sentence-ending punctuation (`.` `!` `?`).  Default 1.8.
    pub pause_sentence: f32,
    /// Pause multiplier for clause-ending punctuation (`,` `;` `:`).  Default 1.3.
    pub pause_comma: f32,
    /// Pause multiplier for paragraph/section break tokens.  Default 2.2.
    pub pause_paragraph: f32,
    /// Pause multiplier for numeral tokens.  Default 1.4.
    pub pause_numeral: f32,
    /// Words per flash (1–3).  Default 1.
    pub chunk_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            wpm: 250,
            pause_sentence: 1.8,
            pause_comma: 1.3,
            pause_paragraph: 2.2,
            pause_numeral: 1.4,
            chunk_size: 1,
        }
    }
}

// ── Play state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
}

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsvpSession {
    pub config: Config,
    /// The document's token stream.
    pub tokens: Vec<Token>,
    pub state: PlayState,
    /// Index into `tokens` marking where the current play window starts.
    pub cursor: usize,
    /// Accumulated play-time in ms captured at the moment of the last pause.
    pub elapsed_at_pause: u64,
    /// Running count of word tokens shown in this session.
    pub session_words_shown: u32,
}

impl RsvpSession {
    // ── Construction ──────────────────────────────────────────────────────────

    pub fn new(tokens: Vec<Token>, config: Config) -> Self {
        RsvpSession {
            config,
            tokens,
            state: PlayState::Paused,
            cursor: 0,
            elapsed_at_pause: 0,
            session_words_shown: 0,
        }
    }

    // ── Duration calculation ──────────────────────────────────────────────────

    /// Duration in milliseconds that token `idx` should be displayed.
    pub fn token_duration_ms(&self, idx: usize) -> u64 {
        if idx >= self.tokens.len() {
            return 0;
        }
        let base_ms = 60_000u64 / (self.config.wpm.clamp(100, 1000) as u64);
        let token = &self.tokens[idx];

        let multiplier: f32 = match token.kind {
            TokenKind::ParagraphBreak | TokenKind::SectionBreak => self.config.pause_paragraph,
            TokenKind::Word => {
                let text = token.text.as_str();
                // Numerals take priority for their multiplier, then punctuation.
                if is_numeral(text) {
                    // Combine with sentence/clause if the numeral also ends a sentence.
                    let punct = if ends_sentence(text) {
                        self.config.pause_sentence
                    } else if ends_clause(text) {
                        self.config.pause_comma
                    } else {
                        1.0
                    };
                    self.config.pause_numeral.max(punct)
                } else if ends_sentence(text) {
                    self.config.pause_sentence
                } else if ends_clause(text) {
                    self.config.pause_comma
                } else {
                    1.0
                }
            }
        };

        ((base_ms as f32) * multiplier).round() as u64
    }

    // ── Pure query ────────────────────────────────────────────────────────────

    /// Given `elapsed_ms` since the last [`resume`](RsvpSession::resume) call,
    /// return the index of the token that should be shown now.
    ///
    /// This is a **pure** function — it does not mutate any state.
    pub fn token_at_elapsed(&self, elapsed_ms: u64) -> usize {
        let mut accumulated = 0u64;
        let mut idx = self.cursor;
        while idx < self.tokens.len() {
            let dur = self.token_duration_ms(idx);
            // Stay on this token until its full duration has elapsed.
            if accumulated + dur > elapsed_ms {
                return idx;
            }
            accumulated += dur;
            idx += 1;
        }
        // Clamp to the last token.
        self.tokens.len().saturating_sub(1)
    }

    // ── Explicit mutations ────────────────────────────────────────────────────

    /// Seek to a specific token index.
    pub fn seek(&mut self, idx: usize) {
        self.cursor = idx.min(self.tokens.len().saturating_sub(1));
    }

    /// Pause playback, recording `elapsed_ms` so stats can be computed later.
    pub fn pause(&mut self, elapsed_ms: u64) {
        if self.state == PlayState::Playing {
            // Advance cursor to where we actually are.
            self.cursor = self.token_at_elapsed(elapsed_ms);
            self.elapsed_at_pause += elapsed_ms;
            self.state = PlayState::Paused;
        }
    }

    /// Resume from paused state.  The caller should reset its own elapsed counter.
    pub fn resume(&mut self) {
        self.state = PlayState::Playing;
    }

    /// Jump back `n` word tokens from the current position.
    ///
    /// The caller must reset its elapsed counter after this call.
    pub fn back_words(&mut self, n: usize, elapsed_ms: u64) {
        let current = self.token_at_elapsed(elapsed_ms);
        // Walk backward, counting only Word tokens.
        let mut words_skipped = 0;
        let mut target = current;
        while target > 0 && words_skipped < n {
            target -= 1;
            if self.tokens[target].kind == TokenKind::Word {
                words_skipped += 1;
            }
        }
        self.cursor = target;
    }

    /// Change WPM mid-playback.  The schedule is computed lazily, so this
    /// just snaps the cursor to the current token and updates the config.
    ///
    /// The caller must reset its elapsed counter after this call.
    pub fn set_wpm(&mut self, wpm: u32, elapsed_ms: u64) {
        self.cursor = self.token_at_elapsed(elapsed_ms);
        self.config.wpm = wpm.clamp(100, 1000);
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    pub fn stats(&self) -> SessionStats {
        let duration_ms = self.elapsed_at_pause;
        let estimated_wpm = if duration_ms > 0 {
            (self.session_words_shown as f64 / (duration_ms as f64 / 60_000.0)) as f32
        } else {
            0.0
        };
        SessionStats {
            words_shown: self.session_words_shown,
            estimated_wpm,
            duration_ms,
        }
    }
}

// ── Stats ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub words_shown: u32,
    pub estimated_wpm: f32,
    pub duration_ms: u64,
}

// ── ORP (Optimal Recognition Point) ──────────────────────────────────────────

/// Return the byte index of the ORP character within `word`.
///
/// Rule: target position ≈ 30% into the word; prefer a vowel at or after that
/// position, otherwise use the character at that index.
pub fn orp_index(word: &str) -> usize {
    if word.is_empty() {
        return 0;
    }
    let chars: Vec<char> = word.chars().collect();
    let target = ((chars.len() as f32) * 0.30).round() as usize;
    let target = target.min(chars.len() - 1);

    // Try to land on a vowel at or after the target (within the word).
    const VOWELS: &[char] = &['a', 'e', 'i', 'o', 'u', 'A', 'E', 'I', 'O', 'U'];
    let orp_char_idx = (target..chars.len())
        .find(|&i| VOWELS.contains(&chars[i]))
        .unwrap_or(target);

    // Convert char index to byte index.
    word.char_indices()
        .nth(orp_char_idx)
        .map(|(b, _)| b)
        .unwrap_or(0)
}

// ── Pause helpers ─────────────────────────────────────────────────────────────

/// True if `text` ends with a sentence-terminating character (`.`, `!`, `?`).
pub fn ends_sentence(text: &str) -> bool {
    matches!(
        text.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '!' && c != '?')
            .chars()
            .last(),
        Some('.' | '!' | '?')
    )
}

/// True if `text` ends with a clause-separating character (`,`, `;`, `:`).
pub fn ends_clause(text: &str) -> bool {
    matches!(text.chars().last(), Some(',' | ';' | ':'))
}

/// True if `text` is a numeral (digits with optional `.` or `,` separators).
pub fn is_numeral(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && text.chars().any(|c| c.is_ascii_digit())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn word_token(text: &str) -> Token {
        Token {
            text: text.to_string(),
            kind: TokenKind::Word,
            section_idx: 0,
            block_idx: 0,
            char_offset: 0,
        }
    }

    fn break_token() -> Token {
        Token {
            text: String::new(),
            kind: TokenKind::ParagraphBreak,
            section_idx: 0,
            block_idx: 1,
            char_offset: 0,
        }
    }

    #[test]
    fn test_ends_sentence() {
        assert!(ends_sentence("Hello."));
        assert!(ends_sentence("Really?"));
        assert!(ends_sentence("Wow!"));
        assert!(!ends_sentence("Hello,"));
        assert!(!ends_sentence("word"));
    }

    #[test]
    fn test_ends_clause() {
        assert!(ends_clause("however,"));
        assert!(ends_clause("note:"));
        assert!(ends_clause("yes;"));
        assert!(!ends_clause("word"));
    }

    #[test]
    fn test_is_numeral() {
        assert!(is_numeral("42"));
        assert!(is_numeral("3.14"));
        assert!(is_numeral("1,000"));
        assert!(!is_numeral("abc"));
        assert!(!is_numeral(""));
    }

    #[test]
    fn test_orp_index() {
        // "Hello" -> 30% of 5 = 1.5 -> 2 -> look for vowel from index 2
        // chars: H e l l o  -> index 2='l', not vowel; 3='l', not vowel; 4='o' vowel
        // byte index of 'o' = 4
        assert_eq!(orp_index("Hello"), 4);
        // Single char
        assert_eq!(orp_index("A"), 0);
        assert_eq!(orp_index(""), 0);
    }

    #[test]
    fn token_duration_sentence_pause() {
        let cfg = Config {
            wpm: 250,
            ..Config::default()
        };
        let tokens = vec![word_token("end.")];
        let session = RsvpSession::new(tokens, cfg);
        let base = 60_000 / 250;
        let expected = ((base as f32) * 1.8).round() as u64;
        assert_eq!(session.token_duration_ms(0), expected);
    }

    #[test]
    fn token_duration_paragraph_break() {
        let cfg = Config {
            wpm: 250,
            ..Config::default()
        };
        let tokens = vec![word_token("hello"), break_token()];
        let session = RsvpSession::new(tokens, cfg);
        let base = 60_000 / 250;
        let expected = ((base as f32) * 2.2).round() as u64;
        assert_eq!(session.token_duration_ms(1), expected);
    }

    #[test]
    fn token_at_elapsed_basic() {
        let cfg = Config {
            wpm: 600,
            ..Config::default()
        };
        // At 600 wpm, base = 100ms per word.
        let tokens = vec![word_token("one"), word_token("two"), word_token("three")];
        let mut session = RsvpSession::new(tokens, cfg);
        session.resume();
        // 0ms -> token 0; 50ms -> still token 0; 100ms -> token 1; 200ms -> token 2
        assert_eq!(session.token_at_elapsed(0), 0);
        assert_eq!(session.token_at_elapsed(50), 0);
        assert_eq!(session.token_at_elapsed(100), 1);
        assert_eq!(session.token_at_elapsed(200), 2);
    }

    #[test]
    fn seek_and_back_words() {
        let cfg = Config {
            wpm: 600,
            ..Config::default()
        };
        let tokens: Vec<Token> = (0..10).map(|i| word_token(&i.to_string())).collect();
        let mut session = RsvpSession::new(tokens, cfg);
        session.resume();
        session.seek(5);
        assert_eq!(session.cursor, 5);
        // back_words(2, 0) from cursor=5, elapsed=0 means current=5; back 2 words -> 3
        session.back_words(2, 0);
        assert_eq!(session.cursor, 3);
    }

    #[test]
    fn set_wpm_clamps() {
        let mut session = RsvpSession::new(vec![word_token("x")], Config::default());
        session.set_wpm(50, 0);
        assert_eq!(session.config.wpm, 100);
        session.set_wpm(9999, 0);
        assert_eq!(session.config.wpm, 1000);
    }
}
