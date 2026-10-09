//! Pure RSVP pacing engine.
//!
//! No timer, no I/O, no async.  The SwiftUI shell drives it from `CVDisplayLink`
//! by calling [`RsvpSession::token_at_elapsed`] on every frame.

use gist_model::{Token, TokenKind};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use unicode_segmentation::UnicodeSegmentation;

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
    /// Whether sentence/clause/numeral pauses apply to word tokens. When
    /// `false`, every word gets the base duration; paragraph/section-break
    /// pauses are structural and always apply. Default `true`.
    #[serde(default = "default_pause_on_punctuation")]
    pub pause_on_punctuation: bool,
}

fn default_pause_on_punctuation() -> bool {
    true
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
            pause_on_punctuation: true,
        }
    }
}

// ── Play state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
}

// ── Schedule cache ────────────────────────────────────────────────────────────

/// Everything the cached schedule below depends on, reduced to a value that
/// can be compared for equality: the whole of [`Config`] (every field
/// [`RsvpSession::token_duration_ms`] reads), the token count, and `cursor`
/// (the schedule is anchored there).
///
/// The schedule is keyed on this rather than invalidated by the methods that
/// *should* change it, because every field of [`RsvpSession`] is `pub`: a
/// caller can assign `session.config`, `session.tokens` or `session.cursor`
/// directly without going through [`RsvpSession::set_wpm`]/
/// [`RsvpSession::seek`] — `gist_core::Core::start_rsvp` does exactly that
/// with `cursor` — and a method-triggered invalidation scheme would then
/// silently serve a stale schedule. Re-checking one `Eq` comparison per call
/// cannot drift.
///
/// Floats are compared by bit pattern (`to_bits`) so this stays `Eq` — a
/// `NaN` multiplier merely rebuilds the schedule every tick, which is
/// correct-but-slow rather than wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScheduleKey {
    wpm: u32,
    pause_sentence: u32,
    pause_comma: u32,
    pause_paragraph: u32,
    pause_numeral: u32,
    chunk_size: usize,
    pause_on_punctuation: bool,
    token_count: usize,
    cursor: usize,
}

/// Incrementally built cumulative-duration table, anchored at `key.cursor`:
/// `cumulative[k]` is the total display time of the `k` tokens
/// `cursor .. cursor + k`, so `cumulative[0] == 0` and
/// `cumulative[k + 1] - cumulative[k] == token_duration_ms(cursor + k)`.
///
/// Deliberately **not** pre-filled for the whole stream. It is extended only
/// as far as a query actually needs (see
/// [`RsvpSession::token_at_elapsed`]'s performance note): building the whole
/// table up front made a mid-session `set_wpm` on a 20 000-token stream cost
/// 263 µs instead of 8 µs, because it paid for every token in the document
/// rather than the handful the reader has reached. Extending lazily makes the
/// first query after a cursor/config change cost the same as the old linear
/// scan and every subsequent query O(log k).
#[derive(Debug, Clone)]
struct Schedule {
    key: ScheduleKey,
    cumulative: Vec<u64>,
}

impl Schedule {
    /// Number of tokens whose durations are already tabulated.
    fn built(&self) -> usize {
        self.cumulative.len() - 1
    }

    /// Total display time of the tokens tabulated so far.
    fn total(&self) -> u64 {
        match self.cumulative.last() {
            Some(&t) => t,
            // Unreachable: `cumulative` is never empty (it starts `[0]`).
            None => 0,
        }
    }
}

// ── Token-length cap (F44) ────────────────────────────────────────────────────

/// Longest word token, in bytes, a session will present as one unit.
///
/// A real word never approaches this. A hostile or degenerate document (a
/// 20M-character "word") otherwise makes every per-tick `orp_split` and
/// punctuation scan cost hundreds of milliseconds on the UI thread. Longer
/// word tokens are split into consecutive chunks at construction, so every
/// per-token operation is bounded.
pub const MAX_TOKEN_BYTES: usize = 256;

/// Split any word token longer than [`MAX_TOKEN_BYTES`] into consecutive
/// chunks (grapheme-aligned where a grapheme fits, otherwise at a char
/// boundary). `char_offset` advances by each chunk's byte length, so every
/// chunk still points at its own slice of the plain text. Tokens at or under
/// the cap, and non-word tokens, pass through untouched, so ordinary
/// documents keep identical indices.
fn split_oversized_tokens(tokens: Vec<Token>) -> Vec<Token> {
    if tokens
        .iter()
        .all(|t| t.kind != TokenKind::Word || t.text.len() <= MAX_TOKEN_BYTES)
    {
        return tokens;
    }
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        if token.kind != TokenKind::Word || token.text.len() <= MAX_TOKEN_BYTES {
            out.push(token);
            continue;
        }
        let push_chunk = |from: usize, to: usize, out: &mut Vec<Token>| {
            if to > from {
                out.push(Token {
                    text: token.text[from..to].to_string(),
                    kind: token.kind.clone(),
                    section_idx: token.section_idx,
                    block_idx: token.block_idx,
                    char_offset: token.char_offset + from,
                });
            }
        };
        let mut start = 0usize;
        let mut end = 0usize;
        for (gi, g) in token.text.grapheme_indices(true) {
            if g.len() > MAX_TOKEN_BYTES {
                // A single enormous grapheme (combining-mark abuse): flush,
                // then cut it by chars.
                push_chunk(start, end, &mut out);
                let mut cs = gi;
                for (ci, c) in g.char_indices() {
                    let abs = gi + ci;
                    if abs + c.len_utf8() - cs > MAX_TOKEN_BYTES {
                        push_chunk(cs, abs, &mut out);
                        cs = abs;
                    }
                }
                push_chunk(cs, gi + g.len(), &mut out);
                start = gi + g.len();
                end = start;
                continue;
            }
            if gi + g.len() - start > MAX_TOKEN_BYTES {
                push_chunk(start, end, &mut out);
                start = gi;
            }
            end = gi + g.len();
        }
        push_chunk(start, end, &mut out);
    }
    out
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
    ///
    /// **Nothing in this crate ever increments it** — it is a `pub` field a
    /// caller may maintain itself, and [`RsvpSession::stats`] reports
    /// whatever it holds. Callers that want live, engine-computed figures
    /// should use [`RsvpSession::stats_at_elapsed`] instead, which derives
    /// words-shown from the token stream and ignores this field.
    pub session_words_shown: u32,

    /// Memoised cumulative-duration table backing
    /// [`RsvpSession::token_at_elapsed`]. `#[serde(skip)]`: it is a pure
    /// function of the other fields, and `Core::start_rsvp` serialises this
    /// struct straight to the clients — the on-the-wire JSON must stay
    /// byte-compatible with what Apple's shipped `RsvpPlayer` decodes.
    ///
    /// `RefCell` rather than `Mutex`: `RsvpSession` has no internal
    /// threading, nothing needs it to be `Sync`, and the FFI wrapper in
    /// `gist-ffi` already puts the whole session behind a `Mutex` (which is
    /// `Sync` as long as its contents are `Send`, and `RefCell<T>` is `Send`
    /// when `T` is).
    #[serde(skip)]
    schedule: RefCell<Option<Schedule>>,
}

impl RsvpSession {
    // ── Construction ──────────────────────────────────────────────────────────

    pub fn new(tokens: Vec<Token>, config: Config) -> Self {
        RsvpSession {
            config,
            tokens: split_oversized_tokens(tokens),
            state: PlayState::Paused,
            cursor: 0,
            elapsed_at_pause: 0,
            session_words_shown: 0,
            schedule: RefCell::new(None),
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
                if !self.config.pause_on_punctuation {
                    // Punctuation pacing is off: plain base duration.
                    1.0
                } else
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
    /// This is a **pure** function — it does not mutate any observable state.
    /// (It may populate the private memoised schedule described below; the
    /// answer is identical either way, which
    /// `memoised_token_at_elapsed_matches_naive_scan_exhaustively` proves.)
    ///
    /// # Performance
    ///
    /// Measured with `cargo bench -p gist-rsvp --bench pacing` on
    /// `x86_64-pc-windows-msvc` (release, 2026-10-04) over a 20 000-token
    /// synthetic stream with a realistic punctuation/numeral/accent mix.
    /// "steady state" is a repeated query with the schedule already
    /// tabulated that far, i.e. an ordinary playback tick. The "linear scan"
    /// column was measured on this same bench against the pre-memoisation
    /// code, in the same session but a separate run — run-to-run variance on
    /// this machine reaches ~2×, so treat the ratios as orders of magnitude,
    /// not exact factors:
    ///
    /// | tokens between `cursor` and the answer | linear scan | memoised (steady state) |
    /// |---|---|---|
    /// | 1     | 21.0 ns | 8.70 ns |
    /// | 600   | 7.72 µs | 33.8 ns |
    /// | 6 000 | 78.8 µs | 41.0 ns |
    ///
    /// A whole tick — this plus
    /// [`elapsed_at_token_end`](RsvpSession::elapsed_at_token_end), which is
    /// what a client needs to know when to wake next — measured 17.7 ns at
    /// 1 token ahead and 53.0 ns at 6 000.
    ///
    /// 6 000 is W4's exit criterion made concrete — a 10-minute soak at 600
    /// WPM with no intervening pause, re-asked on every tick. The linear
    /// scan was already affordable *there* (78.8 µs is 0.47 % of a 60 Hz
    /// frame budget, and the reader ticks at token boundaries, ~10 Hz, not
    /// per frame), so this is not a fix for an observed stall. It was taken
    /// because the scan's cost grows with *uninterrupted* reading time and
    /// has no natural ceiling — a 100 000-token book read straight through
    /// reaches ~1.3 ms per tick — while the memoised path stays in the tens
    /// of nanoseconds: ~1 900× faster at the soak point, and flat beyond it.
    ///
    /// The cost is paid on the first query after any [`ScheduleKey`] change
    /// (`set_wpm`, `seek`, `back_words`, `pause`, or a direct assignment to
    /// a `pub` field), which tabulates forward from the new cursor to the
    /// answer — the same work the linear scan did, done once instead of per
    /// tick. `set_wpm` mid-session on this stream measured 9.70–16.9 µs
    /// memoised across runs vs 8.04 µs scanning — the same order, paid once
    /// per speed change rather than per tick. Space is 8 bytes per *traversed*
    /// token, not per token in the document, because the table is extended
    /// lazily and is discarded whenever the cursor moves.
    ///
    /// An earlier draft tabulated the whole stream eagerly from token 0
    /// instead; that made the same `set_wpm` cost 263 µs (it paid for every
    /// token in the document rather than the ones the reader had reached),
    /// which is why the table is cursor-anchored and lazily grown.
    pub fn token_at_elapsed(&self, elapsed_ms: u64) -> usize {
        let len = self.tokens.len();
        let last = len.saturating_sub(1);
        // `cursor` is a `pub` field, so it is not guaranteed to be in range.
        // The naïve scan this replaces skipped its loop entirely in that
        // case and clamped to the last token; match that exactly.
        if self.cursor >= len {
            return last;
        }

        self.extend_schedule(elapsed_ms, 0);
        let slot = self.schedule.borrow();
        let Some(schedule) = slot.as_ref() else {
            // Unreachable: `extend_schedule` just populated it.
            // Falling back to the reference scan rather than panicking keeps
            // this crate free of `unwrap()`/`expect()`/`unreachable!()`.
            drop(slot);
            return self.token_at_elapsed_naive(elapsed_ms);
        };
        // First tabulated offset `k >= 1` whose cumulative time is strictly
        // greater than `elapsed_ms`; the token to show is `cursor + k - 1`.
        let ahead = schedule.cumulative[1..].partition_point(|&c| c <= elapsed_ms);
        let idx = self.cursor + ahead;
        if idx >= len {
            last
        } else {
            idx
        }
    }

    /// The pre-memoisation linear scan, kept as the reference implementation
    /// [`RsvpSession::token_at_elapsed`] is proven equivalent to by test (and
    /// as its defensive fallback). O(n) from `cursor`, recomputing
    /// [`RsvpSession::token_duration_ms`] for every token it walks past.
    fn token_at_elapsed_naive(&self, elapsed_ms: u64) -> usize {
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

    /// Elapsed-ms value, on the same clock as
    /// [`token_at_elapsed`](RsvpSession::token_at_elapsed)'s argument, at
    /// which `idx` stops being the token to show — i.e. when a client's
    /// timer should next wake. Waking at the boundary and re-deriving the
    /// index from measured time is what keeps a late or coalesced tick from
    /// compounding into drift.
    ///
    /// `0` for an index before `cursor` or an empty stream. For the last
    /// token it is the remaining total, which is also the point past which
    /// `token_at_elapsed` simply clamps.
    ///
    /// O(log k) once the schedule is tabulated that far, where a client
    /// summing `token_duration_ms` itself would be O(k) **per tick** — the
    /// cost the memoisation exists to remove.
    pub fn elapsed_at_token_end(&self, idx: usize) -> u64 {
        let len = self.tokens.len();
        if len == 0 || self.cursor >= len || idx < self.cursor {
            return 0;
        }
        // Number of tabulated entries needed: one per token from `cursor`
        // through `idx`, clamped to the end of the stream.
        let offset = idx.min(len - 1) - self.cursor + 1;
        self.extend_schedule(0, offset);
        let slot = self.schedule.borrow();
        match slot.as_ref().and_then(|s| s.cumulative.get(offset)) {
            Some(&total) => total,
            // Unreachable: `extend_schedule` tabulates `offset` entries.
            // Sum directly rather than panicking.
            None => {
                drop(slot);
                (self.cursor..=idx.min(len - 1))
                    .map(|i| self.token_duration_ms(i))
                    .fold(0u64, u64::saturating_add)
            }
        }
    }

    /// Make the memoised schedule usable for a query: discard it if it was
    /// built for a different [`ScheduleKey`], then tabulate forward until
    /// its running total exceeds `cover_elapsed_ms` **and** it holds at
    /// least `min_entries` entries — stopping early when the token stream
    /// runs out. Never tabulates past the token a query can reach.
    fn extend_schedule(&self, cover_elapsed_ms: u64, min_entries: usize) {
        let key = self.schedule_key();
        let mut slot = self.schedule.borrow_mut();
        if !slot.as_ref().is_some_and(|s| s.key == key) {
            *slot = Some(Schedule {
                key,
                cumulative: vec![0],
            });
        }
        let Some(schedule) = slot.as_mut() else {
            return;
        };
        // Saturating: `cursor` is a `pub` field and may be out of range.
        // (callers already return early in that case; this keeps the helper
        // sound on its own terms.)
        let remaining = self.tokens.len().saturating_sub(key.cursor);
        let mut total = schedule.total();
        while schedule.built() < remaining
            && (total <= cover_elapsed_ms || schedule.built() < min_entries)
        {
            total = total.saturating_add(self.token_duration_ms(key.cursor + schedule.built()));
            schedule.cumulative.push(total);
        }
    }

    fn schedule_key(&self) -> ScheduleKey {
        ScheduleKey {
            wpm: self.config.wpm,
            pause_sentence: self.config.pause_sentence.to_bits(),
            pause_comma: self.config.pause_comma.to_bits(),
            pause_paragraph: self.config.pause_paragraph.to_bits(),
            pause_numeral: self.config.pause_numeral.to_bits(),
            chunk_size: self.config.chunk_size,
            pause_on_punctuation: self.config.pause_on_punctuation,
            token_count: self.tokens.len(),
            cursor: self.cursor,
        }
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
            // Saturating: `elapsed_ms` is client-supplied (`F43`). A plain add
            // panics in debug and silently wraps in release.
            self.elapsed_at_pause = self.elapsed_at_pause.saturating_add(elapsed_ms);
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

    /// Turn sentence/clause/numeral pauses on or off mid-playback. Like
    /// [`set_wpm`](RsvpSession::set_wpm), the position current at
    /// `elapsed_ms` is pinned **under the old setting** first, so toggling
    /// never jumps the reader.
    ///
    /// The caller must reset its elapsed counter after this call.
    pub fn set_pause_on_punctuation(&mut self, enabled: bool, elapsed_ms: u64) {
        self.cursor = self.token_at_elapsed(elapsed_ms);
        self.config.pause_on_punctuation = enabled;
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    /// Number of [`TokenKind::Word`] tokens strictly before `idx` — i.e. the
    /// words a reader has already been shown when `idx` is the token on
    /// screen. Paragraph and section breaks never count as a word read.
    ///
    /// `idx` is clamped to the token count, so an out-of-range index returns
    /// the whole stream's word count rather than panicking.
    pub fn words_shown_through(&self, idx: usize) -> u32 {
        let end = idx.min(self.tokens.len());
        let count = self.tokens[..end]
            .iter()
            .filter(|t| t.kind == TokenKind::Word)
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// Live session stats at `elapsed_ms` since the last
    /// [`resume`](RsvpSession::resume), derived from the token stream rather
    /// than from the caller-maintained
    /// [`session_words_shown`](RsvpSession::session_words_shown) field that
    /// [`stats`](RsvpSession::stats) reports.
    ///
    /// This is what a reading UI should display: `words_shown` counts the
    /// word tokens before the position actually on screen now, and
    /// `duration_ms` is total play time (`elapsed_at_pause` plus the current
    /// run, excluding paused gaps). While
    /// [`state`](RsvpSession::state) is [`PlayState::Paused`] the
    /// `elapsed_ms` argument is ignored entirely — the caller's clock is not
    /// running, so the position is `cursor`, where [`pause`](RsvpSession::pause)
    /// pinned it. `estimated_wpm` is therefore the pace
    /// *achieved*, which differs from `config.wpm` once punctuation pauses
    /// and back-word rewinds are factored in.
    ///
    /// Added for W4 (`docs/windows-development-plan.md` §4.3) so neither
    /// native shell has to hand-port this arithmetic: Apple's
    /// `RsvpStats.wordsShown`/`achievedWpm` in `RsvpView.swift` is a Swift
    /// copy of exactly this, and Windows must not become a third one.
    pub fn stats_at_elapsed(&self, elapsed_ms: u64) -> SessionStats {
        // While paused, `elapsed_ms` is meaningless — the caller's clock is
        // not running — so neither the duration nor the position may use it.
        // `pause` already pinned `cursor` to the token that was on screen.
        let (running, position) = if self.state == PlayState::Playing {
            (elapsed_ms, self.token_at_elapsed(elapsed_ms))
        } else {
            (0, self.cursor.min(self.tokens.len().saturating_sub(1)))
        };
        let duration_ms = self.elapsed_at_pause.saturating_add(running);
        let words_shown = self.words_shown_through(position);
        let estimated_wpm = if duration_ms > 0 {
            (f64::from(words_shown) / (duration_ms as f64 / 60_000.0)) as f32
        } else {
            0.0
        };
        SessionStats {
            words_shown,
            estimated_wpm,
            duration_ms,
        }
    }

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

/// Return the byte index of the ORP grapheme cluster within `word`.
///
/// Rule: target position ≈ 30% into the word; prefer a vowel at or after that
/// position, otherwise use the cluster at that index. Counting is over extended
/// grapheme clusters (matching Swift `Character`), so the result is always a
/// cluster boundary and never splits an emoji, flag or accented letter.
pub fn orp_index(word: &str) -> usize {
    if word.is_empty() {
        return 0;
    }
    let clusters: Vec<(usize, &str)> = word.grapheme_indices(true).collect();
    let target = ((clusters.len() as f32) * 0.30).round() as usize;
    let target = target.min(clusters.len() - 1);

    // Try to land on a vowel at or after the target (within the word).
    const VOWELS: &[&str] = &["a", "e", "i", "o", "u", "A", "E", "I", "O", "U"];
    let idx = (target..clusters.len())
        .find(|&i| VOWELS.contains(&clusters[i].1))
        .unwrap_or(target);

    clusters[idx].0
}

/// Split `word` into `(before, focus, after)` at its ORP, where `focus` is
/// the single grapheme cluster [`orp_index`] selects.
///
/// Exists so clients never have to do the slicing themselves: [`orp_index`]
/// returns a **byte** offset into UTF-8, which is meaningless in C# (UTF-16)
/// and error-prone in Swift, and slicing at the wrong offset would undo the
/// very thing the cluster-based ORP rule is careful about — never splitting
/// an emoji, flag or accented letter. For an empty `word` all three pieces
/// are empty.
pub fn orp_split(word: &str) -> (&str, &str, &str) {
    let start = orp_index(word);
    let (before, rest) = match (word.get(..start), word.get(start..)) {
        (Some(b), Some(r)) => (b, r),
        // Unreachable: `orp_index` always returns a cluster boundary within
        // `word`. Degrade to "all focus" rather than panicking.
        _ => ("", word),
    };
    let focus_len = rest.graphemes(true).next().map_or(0, str::len);
    match (rest.get(..focus_len), rest.get(focus_len..)) {
        (Some(f), Some(a)) => (before, f, a),
        _ => (before, rest, ""),
    }
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

    /// `n` ordinary word tokens, each of which gets the plain 1.0 duration
    /// multiplier. Deliberately `w0`, `w1`, … rather than `0`, `1`, …:
    /// a bare digit string is a numeral, which `token_duration_ms` stretches
    /// by `pause_numeral`, so a stream of them has no simple ms-per-word.
    fn plain_tokens(n: usize) -> Vec<Token> {
        (0..n).map(|i| word_token(&format!("w{i}"))).collect()
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
    fn orp_index_lands_on_whole_grapheme_clusters() {
        // Thumbs-up + skin tone: one cluster, so focus is the start.
        assert_eq!(orp_index("👍🏽"), 0);
        // Flag = two regional indicators, one cluster.
        assert_eq!(orp_index("🇬🇧"), 0);
        // ZWJ family (one cluster) + 'x': 2 clusters, target 1 -> 'x'.
        let family_x = "👨\u{200D}👩x";
        assert_eq!(orp_index(family_x), family_x.len() - 1);
        // Decomposed ñ: single cluster.
        assert_eq!(orp_index("n\u{0303}"), 0);
    }

    #[test]
    fn orp_index_is_always_a_cluster_boundary() {
        for w in [
            "Hello",
            "naïve",
            "re\u{0301}sume\u{0301}",
            "🇬🇧🇫🇷 flags",
            "👨\u{200D}👩\u{200D}👧 family",
            "a👍🏽b",
            "日本語のテキスト",
        ] {
            let i = orp_index(w);
            assert!(
                w.grapheme_indices(true).any(|(b, _)| b == i),
                "{w:?} -> {i} is not a cluster boundary"
            );
        }
    }

    #[test]
    fn pause_saturates_instead_of_overflowing() {
        let mut session =
            RsvpSession::new(vec![word_token("a"), word_token("b")], Config::default());
        session.resume();
        session.elapsed_at_pause = u64::MAX - 5;
        session.pause(1_000);
        assert_eq!(session.elapsed_at_pause, u64::MAX);
    }

    #[test]
    fn oversized_word_tokens_are_split_with_consistent_offsets() {
        let big = "\u{e9}".repeat(1000); // 2000 bytes, 1000 graphemes
        let mut t = word_token(&big);
        t.char_offset = 10;
        let session =
            RsvpSession::new(vec![word_token("a"), t, word_token("b")], Config::default());
        assert!(session.tokens.len() > 3);
        assert_eq!(session.tokens[0].text, "a");
        assert_eq!(session.tokens.last().unwrap().text, "b");
        let chunks = &session.tokens[1..session.tokens.len() - 1];
        assert!(chunks.iter().all(|c| c.text.len() <= MAX_TOKEN_BYTES));
        let rejoined: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(rejoined, big, "no content lost or duplicated");
        let mut expect = 10;
        for c in chunks {
            assert_eq!(c.char_offset, expect);
            expect += c.text.len();
        }
    }

    #[test]
    fn a_single_giant_grapheme_is_still_bounded() {
        let big = format!("e{}", "\u{0301}".repeat(5000)); // one grapheme, ~10 KB
        let session = RsvpSession::new(vec![word_token(&big)], Config::default());
        assert!(session
            .tokens
            .iter()
            .all(|c| c.text.len() <= MAX_TOKEN_BYTES));
        let rejoined: String = session.tokens.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(rejoined, big);
    }

    #[test]
    fn ordinary_tokens_keep_their_indices() {
        let tokens: Vec<Token> = (0..50).map(|_| word_token("word")).collect();
        let session = RsvpSession::new(tokens, Config::default());
        assert_eq!(session.tokens.len(), 50);
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

    // ── Punctuation-pause toggle ──────────────────────────────────────────

    #[test]
    fn punctuation_pauses_off_gives_words_the_base_duration_but_keeps_break_pauses() {
        let tokens = vec![
            word_token("end."),
            word_token("a,"),
            word_token("1,000"),
            word_token("plain"),
            Token {
                text: String::new(),
                kind: TokenKind::ParagraphBreak,
                ..word_token("x")
            },
        ];
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600, // 100 ms base
                ..Config::default()
            },
        );
        assert_eq!(session.token_duration_ms(0), 180);
        assert_eq!(session.token_duration_ms(1), 130);
        assert_eq!(session.token_duration_ms(2), 140);

        session.set_pause_on_punctuation(false, 0);
        for i in 0..4 {
            assert_eq!(session.token_duration_ms(i), 100, "token {i}");
        }
        assert_eq!(
            session.token_duration_ms(4),
            220,
            "break pause is structural"
        );
    }

    #[test]
    fn set_pause_on_punctuation_pins_the_cursor_under_the_old_setting() {
        let tokens = vec![
            word_token("one."),
            word_token("two"),
            word_token("three"),
            word_token("four"),
        ];
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();
        // With pauses on: "one." lasts 180 ms, so 250 ms is token 1. With
        // them off it would be token 2 -- a cursor of 2 would mean the new
        // setting was applied before the position was pinned.
        session.set_pause_on_punctuation(false, 250);
        assert_eq!(session.cursor, 1);
        assert!(!session.config.pause_on_punctuation);
        assert_eq!(session.token_at_elapsed(0), 1);
        assert_eq!(session.token_at_elapsed(100), 2, "100 ms/word now");
    }

    #[test]
    fn config_json_without_the_toggle_defaults_it_on() {
        let json = r#"{"wpm":300,"pause_sentence":1.8,"pause_comma":1.3,
            "pause_paragraph":2.2,"pause_numeral":1.4,"chunk_size":1}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.pause_on_punctuation);
    }

    // ── Memoised schedule (W4 role R1) ────────────────────────────────────

    /// A stream whose durations vary token to token: plain words, clause and
    /// sentence punctuation, numerals, accents, an empty token and a break,
    /// so the cumulative table is not a flat multiple of the base duration.
    fn mixed_tokens() -> Vec<Token> {
        let mut tokens = Vec::new();
        for text in [
            "the",
            "quick,",
            "brown",
            "fox.",
            "1,000",
            "naïve",
            "résumé;",
            "3.14",
            "",
            "x",
            "end!",
        ] {
            tokens.push(word_token(text));
        }
        tokens.insert(4, break_token());
        tokens.push(break_token());
        tokens
    }

    /// The whole point of the memoisation: identical answers to the linear
    /// scan it replaced, for every cursor, every WPM, and every elapsed
    /// value including exact token boundaries, 0, and past the end.
    #[test]
    fn memoised_token_at_elapsed_matches_naive_scan_exhaustively() {
        for wpm in [100u32, 250, 600, 1000] {
            let mut session = RsvpSession::new(
                mixed_tokens(),
                Config {
                    wpm,
                    ..Config::default()
                },
            );
            session.resume();
            let len = session.tokens.len();

            for cursor in 0..len {
                session.cursor = cursor;

                // Every exact token boundary from this cursor, plus one
                // millisecond either side of each — the off-by-one-prone
                // cases — plus 0 and well past the end.
                let mut boundaries = vec![0u64, u64::MAX, u64::MAX - 1];
                let mut acc = 0u64;
                for idx in cursor..len {
                    acc += session.token_duration_ms(idx);
                    boundaries.push(acc.saturating_sub(1));
                    boundaries.push(acc);
                    boundaries.push(acc + 1);
                }
                boundaries.push(acc * 2 + 10_000);

                for elapsed in boundaries {
                    assert_eq!(
                        session.token_at_elapsed(elapsed),
                        session.token_at_elapsed_naive(elapsed),
                        "wpm {wpm}, cursor {cursor}, elapsed {elapsed}"
                    );
                }
            }
        }
    }

    /// `elapsed_at_token_end` must equal the direct sum of durations from
    /// the cursor through the index — that is the contract a client's timer
    /// relies on — and must be exactly the elapsed value at which
    /// `token_at_elapsed` moves on.
    #[test]
    fn elapsed_at_token_end_matches_the_direct_sum_and_is_the_switch_point() {
        for wpm in [100u32, 250, 1000] {
            let mut session = RsvpSession::new(
                mixed_tokens(),
                Config {
                    wpm,
                    ..Config::default()
                },
            );
            session.resume();
            let len = session.tokens.len();

            for cursor in 0..len {
                session.cursor = cursor;
                let mut expected = 0u64;
                for idx in cursor..len {
                    expected += session.token_duration_ms(idx);
                    assert_eq!(
                        session.elapsed_at_token_end(idx),
                        expected,
                        "wpm {wpm}, cursor {cursor}, idx {idx}"
                    );
                    // One ms before the boundary the token is still current;
                    // at the boundary the next one is (unless we ran out).
                    assert_eq!(session.token_at_elapsed(expected - 1), idx);
                    if idx + 1 < len {
                        assert_eq!(session.token_at_elapsed(expected), idx + 1);
                    } else {
                        assert_eq!(session.token_at_elapsed(expected), idx);
                    }
                }
                // Indices at or before the cursor, and past the end.
                if cursor > 0 {
                    assert_eq!(session.elapsed_at_token_end(cursor - 1), 0);
                }
                assert_eq!(session.elapsed_at_token_end(usize::MAX), expected);
            }
        }
    }

    #[test]
    fn elapsed_at_token_end_is_zero_for_a_degenerate_session() {
        let empty = RsvpSession::new(Vec::new(), Config::default());
        assert_eq!(empty.elapsed_at_token_end(0), 0);
        assert_eq!(empty.elapsed_at_token_end(usize::MAX), 0);

        let mut out_of_range = RsvpSession::new(plain_tokens(4), Config::default());
        out_of_range.cursor = 99;
        assert_eq!(out_of_range.elapsed_at_token_end(99), 0);
    }

    /// Changing WPM must re-derive the schedule, not reuse the old one:
    /// halving the speed must double how long the first token is held.
    #[test]
    fn schedule_is_rebuilt_after_a_wpm_change() {
        let tokens = plain_tokens(20);
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();
        // Warm the schedule at 600 wpm (100 ms/word) all the way to the end.
        assert_eq!(session.token_at_elapsed(1_000), 10);

        session.set_wpm(300, 0); // 200 ms/word, cursor pinned at 0
        assert_eq!(session.cursor, 0);
        assert_eq!(session.token_at_elapsed(100), 0, "300 wpm holds 200ms/word");
        assert_eq!(session.token_at_elapsed(1_000), 5);
    }

    /// `seek`/`back_words` move the anchor the schedule is built from, so a
    /// stale table would answer relative to the wrong token.
    #[test]
    fn schedule_follows_the_cursor_after_seek_and_back_words() {
        let tokens = plain_tokens(20);
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();
        assert_eq!(session.token_at_elapsed(550), 5);

        session.seek(10);
        assert_eq!(session.token_at_elapsed(0), 10);
        assert_eq!(session.token_at_elapsed(550), 15);

        session.back_words(4, 0); // from 10, back 4 words -> 6
        assert_eq!(session.cursor, 6);
        assert_eq!(session.token_at_elapsed(0), 6);
        assert_eq!(session.token_at_elapsed(550), 11);
    }

    /// A direct assignment to the `pub` `config`/`cursor` fields bypasses
    /// every mutator, which is exactly what `gist_core::Core::start_rsvp`
    /// does with `cursor`. The key comparison has to catch it.
    #[test]
    fn schedule_key_catches_direct_pub_field_assignment() {
        let tokens = plain_tokens(20);
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();
        assert_eq!(session.token_at_elapsed(550), 5);

        session.cursor = 8; // as Core::start_rsvp restores saved progress
        assert_eq!(session.token_at_elapsed(0), 8);

        session.config.wpm = 300; // not via set_wpm
        assert_eq!(session.token_at_elapsed(100), 8);
        assert_eq!(session.token_at_elapsed(200), 9);
    }

    /// The memoised field must never appear in the serialised session:
    /// `Core::start_rsvp` ships this JSON to the clients, and Apple's
    /// shipped `RsvpPlayer` decodes it.
    #[test]
    fn serialised_session_json_has_no_schedule_field() {
        let mut session = RsvpSession::new(mixed_tokens(), Config::default());
        session.resume();
        session.token_at_elapsed(5_000); // populate the cache
        let json = serde_json::to_string(&session).expect("session serialises");
        assert!(!json.contains("schedule"), "{json}");
        assert!(!json.contains("cumulative"), "{json}");
        for expected in [
            "config",
            "tokens",
            "state",
            "cursor",
            "elapsed_at_pause",
            "session_words_shown",
        ] {
            assert!(json.contains(expected), "missing {expected} in {json}");
        }

        // And a session with no `schedule` key still deserialises.
        let round_tripped: RsvpSession =
            serde_json::from_str(&json).expect("session deserialises without a schedule field");
        assert_eq!(round_tripped.cursor, session.cursor);
        assert_eq!(round_tripped.tokens.len(), session.tokens.len());
        assert_eq!(
            round_tripped.token_at_elapsed(1_500),
            session.token_at_elapsed(1_500)
        );
    }

    // ── Degenerate / hostile streams ──────────────────────────────────────

    #[test]
    fn empty_token_stream_never_panics() {
        let mut session = RsvpSession::new(Vec::new(), Config::default());
        session.resume();
        assert_eq!(session.token_duration_ms(0), 0);
        assert_eq!(session.token_at_elapsed(0), 0);
        assert_eq!(session.token_at_elapsed(u64::MAX), 0);
        assert_eq!(session.words_shown_through(0), 0);
        assert_eq!(session.words_shown_through(usize::MAX), 0);
        session.seek(usize::MAX);
        assert_eq!(session.cursor, 0);
        session.back_words(5, 0);
        assert_eq!(session.cursor, 0);
        session.pause(1_000);
        assert_eq!(session.stats_at_elapsed(0).words_shown, 0);
    }

    #[test]
    fn single_token_stream_clamps_instead_of_running_off_the_end() {
        let mut session = RsvpSession::new(vec![word_token("only")], Config::default());
        session.resume();
        assert_eq!(session.token_at_elapsed(0), 0);
        assert_eq!(session.token_at_elapsed(u64::MAX), 0);
        session.seek(99);
        assert_eq!(session.cursor, 0);
        session.back_words(3, 0);
        assert_eq!(session.cursor, 0);
    }

    #[test]
    fn cursor_assigned_out_of_range_clamps_to_the_last_token() {
        let mut session = RsvpSession::new(plain_tokens(5), Config::default());
        session.resume();
        session.cursor = 500; // `pub` field, nothing stops this
        assert_eq!(session.token_at_elapsed(0), 4);
        assert_eq!(session.token_at_elapsed(u64::MAX), 4);
        assert_eq!(
            session.token_at_elapsed(123),
            session.token_at_elapsed_naive(123)
        );
    }

    #[test]
    fn degenerate_token_text_is_handled() {
        // Empty text, a lone combining mark, an emoji ZWJ sequence and a
        // string that is only punctuation all have to produce a duration and
        // an ORP offset inside the string, not a panic or a split cluster.
        for text in ["", "\u{0303}", "👨\u{200D}👩\u{200D}👧", ".,;", "…"] {
            let session = RsvpSession::new(vec![word_token(text)], Config::default());
            let dur = session.token_duration_ms(0);
            assert!(dur > 0, "{text:?} produced no duration");
            let orp = orp_index(text);
            assert!(orp <= text.len(), "{text:?} orp {orp} past end");
            if !text.is_empty() {
                assert!(
                    text.is_char_boundary(orp),
                    "{text:?} orp {orp} is not a char boundary"
                );
            }
        }
    }

    #[test]
    fn back_words_from_the_start_does_not_underflow() {
        let mut session = RsvpSession::new(plain_tokens(3), Config::default());
        session.resume();
        session.back_words(usize::MAX, 0);
        assert_eq!(session.cursor, 0);
        session.back_words(1, 0);
        assert_eq!(session.cursor, 0);
    }

    // ── pause / resume / stats semantics ──────────────────────────────────

    /// Paused time must not count: `elapsed_ms` after a resume is measured
    /// from the resume, and the cursor must stay where the pause pinned it.
    #[test]
    fn pause_then_resume_excludes_paused_time_and_does_not_jump() {
        let tokens = plain_tokens(40);
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600, // 100 ms/word
                ..Config::default()
            },
        );
        session.resume();

        session.pause(550); // 5.5 words in
        assert_eq!(session.cursor, 5);
        assert_eq!(session.elapsed_at_pause, 550);

        // However long the pause lasts, the caller restarts its own clock at
        // resume, so elapsed 0 must still be token 5 — not token 5 plus
        // however many tokens' worth of wall clock went by while paused.
        session.resume();
        assert_eq!(session.token_at_elapsed(0), 5);
        assert_eq!(session.token_at_elapsed(99), 5);
        assert_eq!(session.token_at_elapsed(100), 6);

        session.pause(250);
        assert_eq!(session.cursor, 7);
        assert_eq!(
            session.elapsed_at_pause, 800,
            "play time only, no pause gap"
        );
    }

    /// `set_wpm` has to pin the cursor under the *old* speed before applying
    /// the new one. Apple's `RsvpWallClockEngine` mirrors this exactly; a
    /// regression makes every speed change jump the reader's position.
    #[test]
    fn set_wpm_pins_the_cursor_under_the_old_speed() {
        let tokens = plain_tokens(40);
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600, // 100 ms/word
                ..Config::default()
            },
        );
        session.resume();

        // 650 ms at 600 wpm is token 6. At the new 120 ms/word it would be
        // token 5 — so a cursor of 5 here would mean the new speed was
        // applied before the position was pinned.
        session.set_wpm(500, 650);
        assert_eq!(session.cursor, 6);
        assert_eq!(session.config.wpm, 500);
        assert_eq!(session.token_at_elapsed(0), 6);
        assert_eq!(session.token_at_elapsed(120), 7, "120 ms/word at 500 wpm");
    }

    #[test]
    fn stats_at_elapsed_counts_words_not_breaks_and_excludes_paused_time() {
        let mut tokens = plain_tokens(10);
        tokens.insert(3, break_token());
        let mut session = RsvpSession::new(
            tokens,
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();

        // 100 ms/word, and the break at index 3 lasts 220 ms (2.2x).
        let at_start = session.stats_at_elapsed(0);
        assert_eq!(at_start.words_shown, 0);
        assert_eq!(at_start.duration_ms, 0);
        assert_eq!(at_start.estimated_wpm, 0.0);

        // 350 ms: tokens 0,1,2 (300 ms) shown, now on the break.
        let mid = session.stats_at_elapsed(350);
        assert_eq!(mid.words_shown, 3, "the break is not a word");
        assert_eq!(mid.duration_ms, 350);

        session.pause(350);
        // Paused: the running clock no longer contributes.
        let paused = session.stats_at_elapsed(10_000);
        assert_eq!(paused.duration_ms, 350);
        assert_eq!(paused.words_shown, 3);

        session.resume();
        let after = session.stats_at_elapsed(220);
        assert_eq!(after.duration_ms, 570, "350 played + 220 played");
        assert_eq!(after.words_shown, 3, "still on the break's last instant");
    }

    #[test]
    fn words_shown_through_clamps_and_ignores_breaks() {
        let mut tokens = plain_tokens(4);
        tokens.insert(2, break_token());
        let session = RsvpSession::new(tokens, Config::default());
        assert_eq!(session.words_shown_through(0), 0);
        assert_eq!(session.words_shown_through(2), 2);
        assert_eq!(session.words_shown_through(3), 2, "index 2 is the break");
        assert_eq!(session.words_shown_through(4), 3);
        assert_eq!(session.words_shown_through(usize::MAX), 4);
    }

    /// `stats()` reports the caller-maintained `session_words_shown` field,
    /// which this crate never increments. Pinned deliberately so a future
    /// change to it is a conscious one — `stats_at_elapsed` is the engine-
    /// computed alternative.
    #[test]
    fn stats_reports_the_caller_maintained_counter_verbatim() {
        let mut session = RsvpSession::new(
            plain_tokens(10),
            Config {
                wpm: 600,
                ..Config::default()
            },
        );
        session.resume();
        session.pause(500);
        assert_eq!(session.stats().words_shown, 0, "engine never sets this");
        assert_eq!(session.stats().duration_ms, 500);

        session.session_words_shown = 5;
        assert_eq!(session.stats().words_shown, 5);
        assert_eq!(session.stats_at_elapsed(0).words_shown, 5, "really token 5");
    }
}
