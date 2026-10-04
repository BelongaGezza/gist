//! Performance benchmark for `RsvpSession::token_at_elapsed`, the function a
//! UI calls on every playback tick.
//!
//! Added for W4 role R1 (`docs/w4-agent-roles.md` §2 R1): W4's exit criterion
//! is "a 10-minute soak at 600 WPM shows no cumulative drift vs. wall clock",
//! which means the UI re-asks "which token is due now?" on every tick with a
//! cursor that has not moved since the last `resume`. At 600 WPM that is
//! roughly 6 000 tokens between the cursor and the answer by the end of the
//! soak, so the cost of answering has to be measured, not assumed.
//!
//! Run with `cargo bench -p gist-rsvp`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use gist_model::{Token, TokenKind};
use gist_rsvp::{Config, RsvpSession};

/// A token stream with the punctuation/numeral mix that makes
/// `token_duration_ms` do real Unicode work rather than hitting the trivial
/// "plain word" path every time.
fn synthetic_tokens(n: usize) -> Vec<Token> {
    let words = [
        "the",
        "quick",
        "brown",
        "fox,",
        "jumps",
        "over",
        "1,000",
        "lazy",
        "dogs.",
        "naïve",
        "résumé;",
        "3.14159",
    ];
    (0..n)
        .map(|i| {
            if i % 97 == 96 {
                Token {
                    text: String::new(),
                    kind: TokenKind::ParagraphBreak,
                    section_idx: 0,
                    block_idx: i / 97,
                    char_offset: 0,
                }
            } else {
                Token {
                    text: words[i % words.len()].to_string(),
                    kind: TokenKind::Word,
                    section_idx: 0,
                    block_idx: i / 97,
                    char_offset: 0,
                }
            }
        })
        .collect()
}

fn session(n: usize, wpm: u32) -> RsvpSession {
    let mut s = RsvpSession::new(
        synthetic_tokens(n),
        Config {
            wpm,
            ..Config::default()
        },
    );
    s.resume();
    s
}

/// Elapsed value that lands the answer `tokens_ahead` tokens past the cursor.
fn elapsed_for(session: &RsvpSession, tokens_ahead: usize) -> u64 {
    (0..tokens_ahead)
        .map(|i| session.token_duration_ms(i))
        .sum::<u64>()
        + 1
}

fn bench_token_at_elapsed(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_at_elapsed");

    // 600 WPM for 10 minutes ~= 6 000 word tokens from a cursor that has not
    // moved — the W4 soak's worst case, asked once per tick.
    for ahead in [1usize, 600, 6_000] {
        let s = session(20_000, 600);
        let elapsed = elapsed_for(&s, ahead);
        group.bench_function(format!("{ahead}_tokens_from_cursor"), |b| {
            b.iter(|| s.token_at_elapsed(elapsed))
        });
    }

    group.finish();
}

/// The per-tick path a UI actually runs: "which token now?" plus "when does
/// it stop being current?". Both have to be cheap, not just the first.
fn bench_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("rsvp_tick");

    for ahead in [1usize, 6_000] {
        let s = session(20_000, 600);
        let elapsed = elapsed_for(&s, ahead);
        group.bench_function(format!("index_and_boundary_{ahead}_from_cursor"), |b| {
            b.iter(|| {
                let idx = s.token_at_elapsed(elapsed);
                s.elapsed_at_token_end(idx)
            })
        });
    }

    group.finish();
}

fn bench_mutations(c: &mut Criterion) {
    let mut group = c.benchmark_group("rsvp_mutations");

    // `set_wpm` is the one mutation that changes every remaining token's
    // duration, so it is the interesting one for any memoisation scheme.
    group.bench_function("set_wpm_mid_session", |b| {
        b.iter_batched(
            || session(20_000, 600),
            |mut s| {
                s.set_wpm(300, 60_000);
                s
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_token_at_elapsed, bench_tick, bench_mutations);
criterion_main!(benches);
