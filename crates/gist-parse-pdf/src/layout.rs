//! Backend-independent layout analysis: glyphs -> lines -> reading order ->
//! header/footer stripping -> headings and paragraphs -> `Document`.
//!
//! Everything here is pure (no pdfium, no I/O), so it is unit-tested with
//! synthetic glyph data and also runs on platforms without the pdfium library.
//! Coordinates follow PDF convention: origin bottom-left, y grows upward.

use std::collections::BTreeMap;

use gist_model::{Block, Document, Metadata, ParseLimits, Section, TextRun};

use crate::PdfError;

/// One extracted character with its bounding box and font size, in page points.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub ch: char,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub size: f32,
}

/// All glyphs of one page, in the order the backend produced them (content
/// stream order). Dropped as soon as the page has been turned into lines.
#[derive(Debug, Clone)]
pub struct RawPage {
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<Glyph>,
}

/// Fraction of page height treated as the header / footer margin band.
const BAND_FRACTION: f32 = 0.10;
/// A margin-band line must repeat on at least this share of pages (percent)
/// - and on at least `MIN_REPEAT_PAGES` pages - to count as a running header
///   or footer (ADR-002's repetition heuristic).
const REPEAT_PERCENT: usize = 35;
const MIN_REPEAT_PAGES: usize = 3;
/// A line is a heading candidate when its dominant font size is at least this
/// multiple of the body size.
const HEADING_RATIO: f32 = 1.15;
/// Longest text (in chars) still considered a heading.
const MAX_HEADING_CHARS: usize = 200;
/// Deepest heading level emitted.
const MAX_HEADING_LEVEL: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    Top,
    Body,
    Bottom,
}

#[derive(Debug, Clone)]
struct Frag {
    text: String,
    x0: f32,
    x1: f32,
    ymid: f32,
    size: f32,
    chars: usize,
}

/// A reconstructed text line.
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub ymid: f32,
    pub size: f32,
    pub chars: usize,
    /// Reading-order group: lines with the same `group` on the same page are
    /// in one column / one band, so vertical distance between them is
    /// meaningful. A change of group is a hard boundary.
    group: usize,
    band: Band,
}

/// Lines of one page after reading-order reconstruction.
#[derive(Debug, Clone)]
pub struct PageLines {
    pub lines: Vec<Line>,
}

fn finite(v: f32) -> bool {
    v.is_finite() && v.abs() < 1.0e7
}

fn size_key(s: f32) -> i32 {
    (s * 2.0).round() as i32
}

fn dominant_size(hist: &BTreeMap<i32, usize>) -> f32 {
    hist.iter()
        .max_by_key(|(k, c)| (**c, **k))
        .map(|(k, _)| *k as f32 / 2.0)
        .unwrap_or(0.0)
}

/// Step 1: glyph stream -> fragments (runs that share a baseline and are
/// horizontally contiguous).
fn glyphs_to_frags(glyphs: &[Glyph]) -> Vec<Frag> {
    struct Cur {
        text: String,
        x0: f32,
        x1: f32,
        ysum: f32,
        hist: BTreeMap<i32, usize>,
        chars: usize,
        last_ymid: f32,
        last_size: f32,
    }
    fn flush(cur: Option<Cur>, out: &mut Vec<Frag>) {
        if let Some(c) = cur {
            let text = c.text.trim().to_string();
            if text.is_empty() || c.chars == 0 {
                return;
            }
            out.push(Frag {
                text,
                x0: c.x0,
                x1: c.x1,
                ymid: c.ysum / c.chars as f32,
                size: dominant_size(&c.hist),
                chars: c.chars,
            });
        }
    }

    let mut out = Vec::new();
    let mut cur: Option<Cur> = None;
    for g in glyphs {
        if g.ch == '\u{00AD}' {
            continue; // soft hyphen: invisible, would pollute words
        }
        if g.ch == '\n' || g.ch == '\r' {
            flush(cur.take(), &mut out);
            continue;
        }
        if g.ch.is_control() && g.ch != '\t' {
            continue;
        }
        if !(finite(g.x0) && finite(g.x1) && finite(g.y0) && finite(g.y1) && finite(g.size)) {
            continue;
        }
        let ch = if g.ch == '\t' { ' ' } else { g.ch };
        let size = if g.size > 0.0 {
            g.size
        } else {
            (g.y1 - g.y0).abs().max(1.0)
        };
        let ymid = (g.y0 + g.y1) / 2.0;
        let (x0, x1) = if g.x0 <= g.x1 {
            (g.x0, g.x1)
        } else {
            (g.x1, g.x0)
        };

        let breaks = match &cur {
            None => false,
            Some(c) => {
                let sz = size.max(c.last_size);
                (ymid - c.last_ymid).abs() > 0.5 * sz || x0 < c.x1 - sz || x0 - c.x1 > 3.0 * sz
            }
        };
        if breaks {
            flush(cur.take(), &mut out);
        }
        match cur.as_mut() {
            None => {
                let mut hist = BTreeMap::new();
                if !ch.is_whitespace() {
                    *hist.entry(size_key(size)).or_insert(0) += 1;
                }
                cur = Some(Cur {
                    text: ch.to_string(),
                    x0,
                    x1,
                    ysum: if ch.is_whitespace() { 0.0 } else { ymid },
                    hist,
                    chars: usize::from(!ch.is_whitespace()),
                    last_ymid: ymid,
                    last_size: size,
                });
            }
            Some(c) => {
                if !ch.is_whitespace() && x0 - c.x1 > 0.3 * size && !c.text.ends_with(' ') {
                    c.text.push(' ');
                }
                c.text.push(ch);
                if !ch.is_whitespace() {
                    *c.hist.entry(size_key(size)).or_insert(0) += 1;
                    c.chars += 1;
                    c.ysum += ymid;
                    c.last_ymid = ymid;
                    c.last_size = size;
                    c.x1 = c.x1.max(x1);
                    c.x0 = c.x0.min(x0);
                } else {
                    c.x1 = c.x1.max(x1);
                }
            }
        }
    }
    flush(cur.take(), &mut out);
    out
}

/// Step 2: find vertical whitespace gutters (column separators) with a
/// projection profile over fragment x-extents.
fn find_gutters(frags: &[Frag], page_w: f32) -> Vec<f32> {
    let n = frags.len();
    if n < 8 || !finite(page_w) || page_w <= 0.0 {
        return Vec::new();
    }
    let xmin = frags.iter().map(|f| f.x0).fold(f32::INFINITY, f32::min);
    let xmax = frags.iter().map(|f| f.x1).fold(f32::NEG_INFINITY, f32::max);
    let span = xmax - xmin;
    if !finite(span) || span < 50.0 {
        return Vec::new();
    }
    let bw = (span / 1000.0).max(1.0);
    let nbins = ((span / bw).ceil() as usize + 1).min(4000);
    let mut counts = vec![0usize; nbins];
    for f in frags {
        let a = (((f.x0 - xmin) / bw).floor().max(0.0) as usize).min(nbins - 1);
        let b = (((f.x1 - xmin) / bw).ceil().max(0.0) as usize).min(nbins - 1);
        for c in &mut counts[a..=b] {
            *c += 1;
        }
    }
    let threshold = if n >= 12 { (n / 20).max(1) } else { 0 };
    let mut sizes: Vec<f32> = frags.iter().map(|f| f.size).collect();
    sizes.sort_by(|a, b| a.total_cmp(b));
    let median = sizes[sizes.len() / 2].max(1.0);
    let min_width = (1.2 * median).max(8.0);

    let lo = xmin + 0.15 * span;
    let hi = xmax - 0.15 * span;
    let mut gutters = Vec::new();
    let mut i = 0;
    while i < nbins {
        if counts[i] <= threshold {
            let start = i;
            while i < nbins && counts[i] <= threshold {
                i += 1;
            }
            let gx0 = xmin + start as f32 * bw;
            let gx1 = xmin + i as f32 * bw;
            let center = (gx0 + gx1) / 2.0;
            if gx1 - gx0 >= min_width && center > lo && center < hi {
                let left = frags.iter().filter(|f| f.x1 <= gx0 + bw).count();
                let right = frags.iter().filter(|f| f.x0 >= gx1 - bw).count();
                if left >= 3 && right >= 3 {
                    gutters.push(center);
                }
            }
        } else {
            i += 1;
        }
    }
    gutters
}

fn band_of(ymid: f32, height: f32) -> Band {
    if !finite(height) || height <= 0.0 {
        return Band::Body;
    }
    if ymid > height * (1.0 - BAND_FRACTION) {
        Band::Top
    } else if ymid < height * BAND_FRACTION {
        Band::Bottom
    } else {
        Band::Body
    }
}

/// Group fragments of one column/band into lines (top to bottom).
fn frags_to_lines(mut frags: Vec<Frag>, group: usize, height: f32, out: &mut Vec<Line>) {
    frags.sort_by(|a, b| b.ymid.total_cmp(&a.ymid).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<Frag>> = Vec::new();
    for f in frags {
        match rows.last_mut() {
            Some(row) if (row[0].ymid - f.ymid).abs() <= 0.5 * row[0].size.max(f.size).max(1.0) => {
                row.push(f)
            }
            _ => rows.push(vec![f]),
        }
    }
    for mut row in rows {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut text = String::new();
        let mut hist: BTreeMap<i32, usize> = BTreeMap::new();
        let (mut x0, mut x1, mut ysum, mut chars) = (f32::INFINITY, f32::NEG_INFINITY, 0.0, 0usize);
        for f in &row {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&f.text);
            *hist.entry(size_key(f.size)).or_insert(0) += f.chars;
            x0 = x0.min(f.x0);
            x1 = x1.max(f.x1);
            ysum += f.ymid * f.chars as f32;
            chars += f.chars;
        }
        if chars == 0 {
            continue;
        }
        let ymid = ysum / chars as f32;
        out.push(Line {
            text,
            x0,
            x1,
            ymid,
            size: dominant_size(&hist),
            chars,
            group,
            band: band_of(ymid, height),
        });
    }
}

/// Convert one page's glyphs into lines in reading order.
pub fn page_to_lines(page: &RawPage) -> PageLines {
    let frags = glyphs_to_frags(&page.glyphs);
    let gutters = find_gutters(&frags, page.width);
    let mut lines = Vec::new();
    let mut group = 0usize;

    if gutters.is_empty() {
        frags_to_lines(frags, group, page.height, &mut lines);
        return PageLines { lines };
    }

    let crosses = |f: &Frag| gutters.iter().any(|g| f.x0 < *g && f.x1 > *g);
    let mut sorted = frags;
    sorted.sort_by(|a, b| b.ymid.total_cmp(&a.ymid));
    let mut cols: Vec<Vec<Frag>> = vec![Vec::new(); gutters.len() + 1];

    let flush = |cols: &mut Vec<Vec<Frag>>, group: &mut usize, lines: &mut Vec<Line>| {
        for col in cols.iter_mut() {
            if !col.is_empty() {
                frags_to_lines(std::mem::take(col), *group, page.height, lines);
                *group += 1;
            }
        }
    };

    for f in sorted {
        if crosses(&f) {
            flush(&mut cols, &mut group, &mut lines);
            frags_to_lines(vec![f], group, page.height, &mut lines);
            group += 1;
        } else {
            let center = (f.x0 + f.x1) / 2.0;
            let idx = gutters.iter().filter(|g| **g < center).count();
            cols[idx].push(f);
        }
    }
    flush(&mut cols, &mut group, &mut lines);
    PageLines { lines }
}

// ── header / footer / page number stripping ──────────────────────────────────

fn is_page_number_text(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    let core = lower
        .trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c == '.' || c.is_whitespace());
    if core.chars().all(|c| c.is_ascii_digit()) && !core.is_empty() && core.len() <= 6 {
        return true;
    }
    if !core.is_empty() && core.len() <= 8 && core.chars().all(|c| "ivxlcdm".contains(c)) {
        return true;
    }
    // "page 3", "page 3 of 10", "3 of 10", "3 / 10"
    let words: Vec<&str> = core
        .split(|c: char| c.is_whitespace() || c == '/')
        .filter(|w| !w.is_empty())
        .collect();
    let numeric = |w: &str| !w.is_empty() && w.len() <= 6 && w.chars().all(|c| c.is_ascii_digit());
    match words.as_slice() {
        ["page", n] | ["p.", n] => numeric(n),
        ["page", n, "of", m] | [n, "of", m] => numeric(n) && numeric(m),
        [n, m] => numeric(n) && numeric(m),
        _ => false,
    }
}

/// Normalise for repetition comparison: lowercase, digits masked, whitespace
/// collapsed.
fn norm_for_repeat(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            last_space = false;
            if c.is_ascii_digit() {
                out.push('#');
            } else {
                out.extend(c.to_lowercase());
            }
        }
    }
    out.trim().to_string()
}

/// Mark margin-band lines that are page numbers or repeat across pages.
/// Returns a per-page, per-line "strip" mask.
fn strip_mask(pages: &[PageLines]) -> Vec<Vec<bool>> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in pages {
        let mut seen = std::collections::BTreeSet::new();
        for l in &p.lines {
            if l.band != Band::Body {
                seen.insert(norm_for_repeat(&l.text));
            }
        }
        for k in seen {
            *counts.entry(k).or_insert(0) += 1;
        }
    }
    let n = pages.len();
    pages
        .iter()
        .map(|p| {
            p.lines
                .iter()
                .map(|l| {
                    if l.band == Band::Body {
                        return false;
                    }
                    if is_page_number_text(&l.text) {
                        return true;
                    }
                    let key = norm_for_repeat(&l.text);
                    let c = counts.get(&key).copied().unwrap_or(0);
                    n >= MIN_REPEAT_PAGES && c >= MIN_REPEAT_PAGES && c * 100 >= n * REPEAT_PERCENT
                })
                .collect()
        })
        .collect()
}

// ── paragraphs, headings, sections ───────────────────────────────────────────

#[derive(Debug)]
enum Event {
    Heading(u8, String),
    Para(String),
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end()
        .chars()
        .last()
        .map(|c| {
            matches!(
                c,
                '.' | '!' | '?' | ':' | '"' | '”' | '’' | ')' | '。' | '？' | '！'
            )
        })
        .unwrap_or(true)
}

/// Append a continuation line to a paragraph buffer, de-hyphenating a word
/// broken across lines ("exam-" + "ple" -> "example").
fn append_line(buf: &mut String, line: &str) {
    if buf.is_empty() {
        buf.push_str(line);
        return;
    }
    let ends_hyphen = buf.ends_with('-')
        && buf.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic())
        && line.chars().next().is_some_and(|c| c.is_lowercase());
    if ends_hyphen {
        buf.pop();
        buf.push_str(line);
    } else {
        buf.push(' ');
        buf.push_str(line);
    }
}

fn median(v: &mut [f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// Turn all pages' lines into a `Document`.
pub fn build_document(
    pages: Vec<PageLines>,
    metadata: Metadata,
    limits: &ParseLimits,
) -> Result<Document, PdfError> {
    let mask = strip_mask(&pages);

    // Body font size: char-weighted mode over non-stripped lines.
    let mut hist: BTreeMap<i32, usize> = BTreeMap::new();
    for (p, m) in pages.iter().zip(&mask) {
        for (l, strip) in p.lines.iter().zip(m) {
            if !strip {
                *hist.entry(size_key(l.size)).or_insert(0) += l.chars;
            }
        }
    }
    if hist.is_empty() {
        return Err(PdfError::NoTextLayer);
    }
    let body = dominant_size(&hist).max(1.0);

    // Typical line pitch within a group, for paragraph-break detection.
    let mut pitches = Vec::new();
    for (p, m) in pages.iter().zip(&mask) {
        let kept: Vec<&Line> = p
            .lines
            .iter()
            .zip(m)
            .filter(|(_, s)| !**s)
            .map(|(l, _)| l)
            .collect();
        for w in kept.windows(2) {
            let (a, b) = (w[0], w[1]);
            let pitch = a.ymid - b.ymid;
            if a.group == b.group && pitch > 0.8 * body && pitch < 2.5 * body {
                pitches.push(pitch);
            }
        }
    }
    let pitch_typ = median(&mut pitches).unwrap_or(body * 1.2);

    // A "full" line width (80th percentile of body-size line widths), so a
    // line much shorter than this that ends a sentence ends its paragraph.
    let mut widths: Vec<f32> = Vec::new();
    for (p, m) in pages.iter().zip(&mask) {
        for (l, strip) in p.lines.iter().zip(m) {
            if !strip && (l.size - body).abs() < 0.6 {
                widths.push(l.x1 - l.x0);
            }
        }
    }
    widths.sort_by(|a, b| a.total_cmp(b));
    let full_w = widths
        .get((widths.len() * 4 / 5).min(widths.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);

    // Heading levels: rank of distinct heading sizes, largest first.
    let mut heading_sizes: Vec<i32> = hist
        .keys()
        .copied()
        .filter(|k| (*k as f32 / 2.0) >= body * HEADING_RATIO)
        .collect();
    heading_sizes.sort_by(|a, b| b.cmp(a));
    let level_of = |size: f32| -> u8 {
        let k = size_key(size);
        let rank = heading_sizes.iter().position(|s| *s == k).unwrap_or(0);
        ((rank + 1) as u8).min(MAX_HEADING_LEVEL)
    };
    let is_heading =
        |l: &Line| l.size >= body * HEADING_RATIO && l.text.chars().count() <= MAX_HEADING_CHARS;

    let mut events: Vec<Event> = Vec::new();
    let mut para = String::new();
    let mut heading: Option<(u8, String, f32, usize, f32)> = None; // level,text,size,group,ymid
    let mut prev: Option<(usize, usize, f32)> = None; // (page, group, ymid) of last body line
    let mut prev_text_end = String::new();
    let mut prev_width = 0.0f32;

    macro_rules! flush_para {
        () => {
            if !para.trim().is_empty() {
                events.push(Event::Para(std::mem::take(&mut para)));
            } else {
                para.clear();
            }
        };
    }
    macro_rules! flush_heading {
        () => {
            if let Some((lvl, text, _, _, _)) = heading.take() {
                events.push(Event::Heading(lvl, text));
            }
        };
    }

    for (pi, (p, m)) in pages.iter().zip(&mask).enumerate() {
        for (l, strip) in p.lines.iter().zip(m) {
            if *strip {
                continue;
            }
            let text = l.text.trim();
            if text.is_empty() {
                continue;
            }
            if is_heading(l) {
                flush_para!();
                match heading.as_mut() {
                    Some((_, htext, hsize, hgroup, hy))
                        if (*hsize - l.size).abs() < 0.6
                            && *hgroup == l.group
                            && (*hy - l.ymid) < 2.5 * l.size =>
                    {
                        append_line(htext, text);
                        *hy = l.ymid;
                    }
                    _ => {
                        flush_heading!();
                        heading =
                            Some((level_of(l.size), text.to_string(), l.size, l.group, l.ymid));
                    }
                }
                prev = Some((pi, l.group, l.ymid));
                prev_text_end.clear();
                continue;
            }
            flush_heading!();

            let new_para = match prev {
                None => true,
                Some((pp, pg, py)) => {
                    if pp != pi || pg != l.group {
                        // Page or column boundary: continue the sentence if
                        // the previous line did not finish one.
                        ends_sentence(&prev_text_end)
                    } else {
                        let pitch = py - l.ymid;
                        pitch > 1.5 * pitch_typ
                            || (ends_sentence(&prev_text_end)
                                && prev_width < 0.65 * full_w
                                && text.chars().next().is_some_and(|c| c.is_uppercase()))
                    }
                }
            };
            if new_para {
                flush_para!();
            }
            append_line(&mut para, text);
            prev = Some((pi, l.group, l.ymid));
            prev_text_end = text.to_string();
            prev_width = l.x1 - l.x0;
        }
    }
    flush_heading!();
    flush_para!();

    if events.is_empty() {
        return Err(PdfError::NoTextLayer);
    }

    // Expanded-text budget (also enforced during extraction; re-checked here
    // because header/footer stripping can only shrink it, never grow it).
    let total: usize = events
        .iter()
        .map(|e| match e {
            Event::Heading(_, t) | Event::Para(t) => t.len(),
        })
        .sum();
    if total > limits.max_expanded_bytes {
        return Err(PdfError::ResourceLimitExceeded {
            limit: format!("max_expanded_bytes={}", limits.max_expanded_bytes),
            attempted: total,
        });
    }

    // Sections start at level 1-2 headings; everything before the first is
    // a headless opening section. The heading is both `Section.heading`
    // (for the TOC) and the first `Block::Heading` (so it renders and is part
    // of the token stream), matching how headings appear elsewhere in the IR.
    let mut sections: Vec<Section> = Vec::new();
    let mut cur: Option<Section> = None;
    for ev in events {
        match ev {
            Event::Heading(lvl, text) if lvl <= 2 => {
                if let Some(s) = cur.take() {
                    sections.push(s);
                }
                cur = Some(Section {
                    id: format!("s{}", sections.len()),
                    heading: Some((lvl, text.clone())),
                    blocks: vec![Block::Heading { level: lvl, text }],
                });
            }
            Event::Heading(lvl, text) => {
                cur.get_or_insert_with(|| Section {
                    id: format!("s{}", sections.len()),
                    heading: None,
                    blocks: Vec::new(),
                })
                .blocks
                .push(Block::Heading { level: lvl, text });
            }
            Event::Para(text) => {
                cur.get_or_insert_with(|| Section {
                    id: format!("s{}", sections.len()),
                    heading: None,
                    blocks: Vec::new(),
                })
                .blocks
                .push(Block::Paragraph {
                    runs: vec![TextRun::plain(text)],
                });
            }
        }
    }
    if let Some(s) = cur.take() {
        sections.push(s);
    }
    Ok(Document::new(metadata, sections))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay out `text` as glyphs starting at (x, y) with a fixed advance.
    fn put(glyphs: &mut Vec<Glyph>, text: &str, x: f32, y: f32, size: f32) {
        let adv = size * 0.5;
        for (i, ch) in text.chars().enumerate() {
            glyphs.push(Glyph {
                ch,
                x0: x + i as f32 * adv,
                x1: x + (i + 1) as f32 * adv,
                y0: y - size * 0.2,
                y1: y + size * 0.8,
                size,
            });
        }
    }

    fn page(glyphs: Vec<Glyph>) -> RawPage {
        RawPage {
            width: 612.0,
            height: 792.0,
            glyphs,
        }
    }

    fn doc_of(pages: Vec<RawPage>) -> Result<Document, PdfError> {
        let pl = pages.iter().map(page_to_lines).collect();
        build_document(pl, Metadata::minimal("t"), &ParseLimits::default())
    }

    fn all_text(d: &Document) -> String {
        d.sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .map(|b| b.plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn paragraphs_split_on_large_gap_and_join_lines() {
        let mut g = Vec::new();
        put(&mut g, "The quick brown fox jumps over", 72.0, 700.0, 12.0);
        put(&mut g, "the lazy dog again and again.", 72.0, 686.0, 12.0);
        put(&mut g, "Second paragraph starts here.", 72.0, 650.0, 12.0);
        put(&mut g, "and continues on this line.", 72.0, 636.0, 12.0);
        let d = doc_of(vec![page(g)]).unwrap();
        let blocks: Vec<_> = d.sections.iter().flat_map(|s| &s.blocks).collect();
        assert_eq!(blocks.len(), 2, "{:?}", blocks);
        assert!(blocks[0].plain_text().contains("over the lazy dog"));
    }

    #[test]
    fn large_font_becomes_heading_and_section() {
        let mut g = Vec::new();
        put(&mut g, "Chapter One", 72.0, 720.0, 24.0);
        for i in 0..6 {
            put(
                &mut g,
                "Body text of the chapter goes here.",
                72.0,
                680.0 - i as f32 * 14.0,
                12.0,
            );
        }
        let d = doc_of(vec![page(g)]).unwrap();
        assert_eq!(d.sections.len(), 1);
        assert_eq!(d.sections[0].heading.as_ref().unwrap().1, "Chapter One");
        assert!(matches!(
            d.sections[0].blocks[0],
            Block::Heading { level: 1, .. }
        ));
    }

    #[test]
    fn dehyphenates_across_lines() {
        let mut g = Vec::new();
        put(&mut g, "an exam-", 72.0, 700.0, 12.0);
        put(&mut g, "ple of wrapping.", 72.0, 686.0, 12.0);
        let d = doc_of(vec![page(g)]).unwrap();
        assert!(all_text(&d).contains("example of wrapping"));
    }

    #[test]
    fn two_columns_read_column_by_column() {
        let mut g = Vec::new();
        for i in 0..8 {
            let y = 700.0 - i as f32 * 14.0;
            put(
                &mut g,
                &format!("left col line {i} alpha beta"),
                72.0,
                y,
                12.0,
            );
            put(
                &mut g,
                &format!("right col line {i} gamma delta"),
                330.0,
                y,
                12.0,
            );
        }
        let d = doc_of(vec![page(g)]).unwrap();
        let t = all_text(&d);
        let l7 = t.find("left col line 7").unwrap();
        let r0 = t.find("right col line 0").unwrap();
        assert!(l7 < r0, "left column must be fully read before right: {t}");
    }

    #[test]
    fn strips_repeated_headers_footers_and_page_numbers() {
        let mut pages = Vec::new();
        for n in 1..=5 {
            let mut g = Vec::new();
            put(&mut g, "ACME Annual Report", 72.0, 760.0, 9.0);
            put(
                &mut g,
                &format!("Unique body sentence number {n} is here."),
                72.0,
                600.0,
                12.0,
            );
            put(&mut g, &format!("{n}"), 300.0, 30.0, 9.0);
            pages.push(page(g));
        }
        let d = doc_of(pages).unwrap();
        let t = all_text(&d);
        assert!(!t.contains("ACME"), "{t}");
        assert!(!t.contains("\n1\n") && !t.ends_with("\n5"), "{t}");
        assert!(t.contains("Unique body sentence number 3"));
    }

    #[test]
    fn page_number_forms() {
        for s in ["12", "- 4 -", "Page 3", "page 3 of 10", "iv", "3 / 10"] {
            assert!(is_page_number_text(s), "{s}");
        }
        for s in ["Chapter 3", "2024 results", "hello"] {
            assert!(!is_page_number_text(s), "{s}");
        }
    }

    #[test]
    fn no_glyphs_is_no_text_layer() {
        let r = doc_of(vec![page(Vec::new()), page(Vec::new())]);
        assert!(matches!(r, Err(PdfError::NoTextLayer)));
    }

    #[test]
    fn whitespace_only_is_no_text_layer() {
        let mut g = Vec::new();
        put(&mut g, "   ", 72.0, 700.0, 12.0);
        assert!(matches!(doc_of(vec![page(g)]), Err(PdfError::NoTextLayer)));
    }

    #[test]
    fn non_finite_and_degenerate_glyphs_do_not_panic() {
        let mut g = Vec::new();
        for (x, y, s) in [
            (f32::NAN, 1.0, 12.0),
            (1.0, f32::INFINITY, 12.0),
            (1.0, 1.0, 0.0),
            (1.0, 1.0, -5.0),
            (1.0e30, 1.0, 12.0),
        ] {
            g.push(Glyph {
                ch: 'a',
                x0: x,
                x1: x + 1.0,
                y0: y,
                y1: y + 1.0,
                size: s,
            });
        }
        let _ = doc_of(vec![page(g)]);
    }

    #[test]
    fn expanded_budget_enforced() {
        let mut g = Vec::new();
        put(&mut g, "some body text here", 72.0, 700.0, 12.0);
        let pl = vec![page_to_lines(&page(g))];
        let lim = ParseLimits {
            max_expanded_bytes: 4,
            ..ParseLimits::default()
        };
        assert!(matches!(
            build_document(pl, Metadata::minimal("t"), &lim),
            Err(PdfError::ResourceLimitExceeded { .. })
        ));
    }
}
