//! Metadata value helpers (SC-03/SC-06).
//!
//! Pure, I/O-free (so `gist-model` stays wasm32-clean) helpers shared by the
//! importers. Every value that originates in an untrusted document is passed
//! through one of these before it reaches `Metadata`.

/// Maximum chars kept for free-text metadata such as a byline.
pub const MAX_META_TEXT_CHARS: usize = 200;
/// Maximum bytes kept for a canonical URL.
pub const MAX_META_URL_BYTES: usize = 2048;
/// Maximum bytes kept for a language tag (BCP-47 tags are far shorter).
pub const MAX_LANGUAGE_TAG_BYTES: usize = 35;

/// Collapse whitespace/control characters to single spaces, trim, and cap at
/// `max_chars` characters. `None` when nothing is left.
pub fn sanitize_meta_text(raw: &str, max_chars: usize) -> Option<String> {
    let mut out = String::new();
    let mut count = 0usize;
    let mut pending_space = false;
    for c in raw.chars() {
        if c.is_whitespace() || c.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            if count + 1 >= max_chars {
                break;
            }
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max_chars {
            break;
        }
        out.push(c);
        count += 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Normalise a language tag (`en`, `en_GB`, `pt-BR`, `zh-Hant-TW`) to
/// hyphenated form. Rejects anything that is not a 1–8 letter primary subtag
/// followed by 1–8 character alphanumeric subtags, or longer than 35 bytes.
pub fn normalize_language_tag(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.len() > MAX_LANGUAGE_TAG_BYTES || !t.is_ascii() {
        return None;
    }
    let t = t.replace('_', "-");
    let mut parts = t.split('-');
    let primary = parts.next()?;
    if primary.is_empty() || primary.len() > 8 || !primary.bytes().all(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    for p in parts {
        if p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
    }
    Some(t)
}

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format Unix seconds as an RFC 3339 UTC timestamp (`2026-10-11T12:34:56Z`).
pub fn format_rfc3339_utc(unix_secs: u64) -> String {
    // Clamp so the year always has four digits.
    let secs = unix_secs.min(253_402_300_799) as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn is_leap(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Validate the leading `YYYY-MM-DD` of an ISO-8601/RFC 3339-ish string (a
/// trailing time/zone part is ignored) and return exactly `YYYY-MM-DD`.
/// Years outside 1000–9999, impossible dates (month 13, Feb 30) and
/// non-ASCII-digit input are rejected, as is a remainder that does not start
/// with `T` or a space.
pub fn normalize_published_date(raw: &str) -> Option<String> {
    let t = raw.trim();
    let b = t.as_bytes();
    if b.len() < 10 || b.len() > 64 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if b.len() > 10 && b[10] != b'T' && b[10] != b' ' {
        return None;
    }
    let num = |s: &[u8]| -> Option<u32> {
        if s.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(s).ok()?.parse().ok()
        } else {
            None
        }
    };
    let (y, m, d) = (num(&b[0..4])?, num(&b[5..7])?, num(&b[8..10])?);
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => return None,
    };
    if !(1000..=9999).contains(&y) || d < 1 || d > dim {
        return None;
    }
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_known_values() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_rfc3339_utc(1_781_181_296), "2026-06-11T12:34:56Z");
        assert_eq!(format_rfc3339_utc(u64::MAX), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn sanitize_collapses_caps_and_rejects_empty() {
        assert_eq!(
            sanitize_meta_text("  Jane \n\t Doe\u{0}  ", 200).as_deref(),
            Some("Jane Doe")
        );
        assert_eq!(sanitize_meta_text(" \n\t ", 200), None);
        let huge = "x".repeat(1_000_000);
        assert_eq!(sanitize_meta_text(&huge, 200).unwrap().chars().count(), 200);
        let multi = "é".repeat(500);
        assert_eq!(sanitize_meta_text(&multi, 10).unwrap().chars().count(), 10);
        let spaced = "a ".repeat(500);
        assert!(sanitize_meta_text(&spaced, 50).unwrap().chars().count() <= 50);
    }

    #[test]
    fn language_tags() {
        assert_eq!(normalize_language_tag("en").as_deref(), Some("en"));
        assert_eq!(normalize_language_tag(" en_GB ").as_deref(), Some("en-GB"));
        assert_eq!(
            normalize_language_tag("zh-Hant-TW").as_deref(),
            Some("zh-Hant-TW")
        );
        for bad in [
            "",
            "   ",
            "e n",
            "en-",
            "-en",
            "1en",
            "toolongprimary",
            "en-<script>",
            "日本語",
            &"a-".repeat(100),
        ] {
            assert_eq!(normalize_language_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn published_dates() {
        assert_eq!(
            normalize_published_date("2024-02-29T10:00:00+02:00").as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            normalize_published_date(" 2023-12-01 ").as_deref(),
            Some("2023-12-01")
        );
        for bad in [
            "",
            "yesterday",
            "2023-02-29",
            "2023-13-01",
            "2023-00-10",
            "2023-04-31",
            "0999-01-01",
            "2023-01-0199",
            "２０２３-01-01",
            "2023/01/01",
            &format!("2023-01-01T{}", "9".repeat(10_000)),
        ] {
            assert_eq!(normalize_published_date(bad), None, "{bad:?}");
        }
    }
}
