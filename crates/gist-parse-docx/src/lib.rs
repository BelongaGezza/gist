use std::collections::HashMap;
use std::io::Read;

use quick_xml::events::Event;
use quick_xml::Reader;

// ── Error ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml: {0}")]
    Xml(String),
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded { limit: String, attempted: usize },
    #[error("malformed DOCX: {0}")]
    Malformed(String),
}

// ── Public entry point ─────────────────────────────────────────────────────

/// Parse a DOCX byte slice into a [`gist_model::Document`].
///
/// Enforces `limits` before significant allocation.
pub fn parse(
    bytes: &[u8],
    stem: &str,
    limits: &gist_model::ParseLimits,
) -> Result<gist_model::Document, ParseError> {
    // 1. File size limit
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_bytes={}", limits.max_bytes),
            attempted: bytes.len(),
        });
    }

    // 2. Open zip archive
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor)?;

    // 2b. Entry-count cap (F16) — before touching any entry's content, since
    // a crafted archive with a huge number of near-empty entries can stay
    // well under max_bytes while still being expensive to enumerate.
    if archive.len() > limits.max_zip_entries {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_zip_entries={}", limits.max_zip_entries),
            attempted: archive.len(),
        });
    }

    // 3-5. Parse styles.xml, numbering.xml, and word/document.xml, all
    // sharing one running `total_expanded` budget (N3) — mirrors
    // gist-parse-epub::parse_spine's cumulative accumulator, so a DOCX with
    // three maximally-compressible parts can't expand to ~3x
    // `max_expanded_bytes` by each part individually staying under the cap.
    let mut total_expanded: usize = 0;

    // 3. Parse styles.xml (needed for heading detection)
    let styles = parse_styles(&mut archive, limits, &mut total_expanded)?;

    // 4. Parse numbering.xml (needed for list detection)
    let numbering = parse_numbering(&mut archive, limits, &mut total_expanded)?;

    // 5. Parse word/document.xml (main content)
    let (blocks, has_tracked_changes) = parse_document(
        &mut archive,
        &styles,
        &numbering,
        limits,
        &mut total_expanded,
    )?;

    let mut metadata = gist_model::Metadata {
        title: stem.to_string(),
        author: None,
        source_type: "docx".to_string(),
        source_ref: None,
        source_copy_ref: None,
        import_date: None,
        language: None,
        word_count: 0,
    };

    // Package everything into one section
    let section = gist_model::Section {
        id: "s0".to_string(),
        heading: None,
        blocks,
    };

    if has_tracked_changes {
        // Store as a flag in source_type for now (M1); proper metadata field in M2
        metadata.source_type = "docx:tracked-changes".to_string();
    }

    let doc = gist_model::Document::new(metadata, vec![section]);

    Ok(doc)
}

// ── Styles ─────────────────────────────────────────────────────────────────

/// Resolved heading level for a paragraph style (1-based, or None if not a heading).
type StyleMap = HashMap<String, StyleEntry>;

struct StyleEntry {
    /// heading level 1-6 if this resolves to a Heading style
    heading_level: Option<u8>,
}

fn parse_styles(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    limits: &gist_model::ParseLimits,
    total_expanded: &mut usize,
) -> Result<StyleMap, ParseError> {
    let xml = read_zip_entry_limited(archive, "word/styles.xml", limits, total_expanded)?;

    // Build a raw map: styleId -> (w:name val, basedOn styleId)
    // Then walk basedOn chains to resolve heading levels.
    let mut raw: HashMap<String, (String, Option<String>)> = HashMap::new();

    let mut reader = Reader::from_str(&xml);
    let mut buf = Vec::new();
    let mut current_style_id: Option<String> = None;
    let mut current_name: Option<String> = None;
    let mut current_based_on: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);

                match tag_local {
                    "style" => {
                        // Flush previous style entry
                        if let Some(id) = current_style_id.take() {
                            raw.insert(
                                id,
                                (
                                    current_name.take().unwrap_or_default(),
                                    current_based_on.take(),
                                ),
                            );
                        }
                        // Extract styleId attribute
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "styleId" {
                                current_style_id = Some(attr.value.to_string());
                            }
                        }
                    }
                    "name" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                current_name = Some(attr.value.to_string());
                            }
                        }
                    }
                    "basedOn" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                current_based_on = Some(attr.value.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);
                if tag_local == "style" {
                    if let Some(id) = current_style_id.take() {
                        raw.insert(
                            id,
                            (
                                current_name.take().unwrap_or_default(),
                                current_based_on.take(),
                            ),
                        );
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    // Resolve chains: for each style, walk basedOn until we hit a Heading N style
    let mut result = StyleMap::new();
    for id in raw.keys() {
        let heading_level = resolve_heading_level(&raw, id, 0);
        result.insert(id.clone(), StyleEntry { heading_level });
    }

    Ok(result)
}

/// Walk the basedOn chain to find if style_id ultimately inherits from a Heading N style.
/// Returns the heading level (1-6) or None.
fn resolve_heading_level(
    raw: &HashMap<String, (String, Option<String>)>,
    style_id: &str,
    depth: usize,
) -> Option<u8> {
    if depth > 20 {
        return None; // cycle guard
    }

    let (name, based_on) = raw.get(style_id)?;

    // Check if this style's name is "heading N" or "Heading N"
    let lower = name.to_lowercase();
    if let Some(suffix) = lower.strip_prefix("heading") {
        let suffix = suffix.trim();
        if let Ok(n) = suffix.parse::<u8>() {
            if (1..=6).contains(&n) {
                return Some(n);
            }
        }
    }

    // Also check by styleId directly (e.g. "Heading1", "Heading2")
    let id_lower = style_id.to_lowercase();
    if let Some(suffix) = id_lower.strip_prefix("heading") {
        let suffix = suffix.trim();
        if let Ok(n) = suffix.parse::<u8>() {
            if (1..=6).contains(&n) {
                return Some(n);
            }
        }
    }

    // Walk basedOn chain
    if let Some(parent_id) = based_on {
        return resolve_heading_level(raw, parent_id, depth + 1);
    }

    None
}

// ── Numbering ──────────────────────────────────────────────────────────────

struct NumberingEntry {
    is_ordered: bool,
    #[allow(dead_code)]
    level: usize,
}

type NumberingMap = HashMap<(String, usize), NumberingEntry>;

fn parse_numbering(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    limits: &gist_model::ParseLimits,
    total_expanded: &mut usize,
) -> Result<NumberingMap, ParseError> {
    // numbering.xml may not exist in simple DOCX files
    let xml =
        match read_zip_entry_limited_opt(archive, "word/numbering.xml", limits, total_expanded)? {
            Some(s) => s,
            None => return Ok(NumberingMap::new()),
        };

    // For M1: simplified heuristic.
    // A proper implementation would walk abstractNum -> numFmt chains.
    // Here: if w:numFmt val="bullet" -> unordered; else -> ordered.

    let mut result = NumberingMap::new();
    let mut reader = Reader::from_str(&xml);
    let mut buf = Vec::new();
    let mut current_num_id: Option<String> = None;
    let mut current_level: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);

                match tag_local {
                    "num" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            if k.ends_with("numId") {
                                current_num_id = Some(attr.value.to_string());
                            }
                        }
                    }
                    "lvl" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            if k.ends_with("ilvl") {
                                current_level = attr.value.parse().unwrap_or(0);
                            }
                        }
                    }
                    "numFmt" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            if k.ends_with("val") {
                                let fmt_val = attr.value.to_string();
                                if let Some(id) = &current_num_id {
                                    let is_ordered = fmt_val != "bullet";
                                    result.insert(
                                        (id.clone(), current_level),
                                        NumberingEntry {
                                            is_ordered,
                                            level: current_level,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(result)
}

// ── Document body parsing ──────────────────────────────────────────────────

fn parse_document(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    styles: &StyleMap,
    numbering: &NumberingMap,
    limits: &gist_model::ParseLimits,
    total_expanded: &mut usize,
) -> Result<(Vec<gist_model::Block>, bool), ParseError> {
    let xml = read_zip_entry_limited(archive, "word/document.xml", limits, total_expanded)?;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut blocks = Vec::new();
    let mut has_tracked_changes = false;

    // State per paragraph
    let mut in_para = false;
    let mut current_style_id: Option<String> = None;
    let mut current_num_id: Option<String> = None;
    let mut current_num_level: usize = 0;
    let mut current_runs: Vec<gist_model::TextRun> = Vec::new();

    // Run-level formatting
    let mut run_bold = false;
    let mut run_italic = false;
    let mut run_code = false;
    let mut in_run = false;
    let mut in_del = false; // inside w:del (skip deleted text)
    let mut nesting_depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                nesting_depth += 1;
                if nesting_depth > limits.max_nesting_depth {
                    return Err(ParseError::ResourceLimitExceeded {
                        limit: format!("max_nesting_depth={}", limits.max_nesting_depth),
                        attempted: nesting_depth,
                    });
                }

                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);

                match tag_local {
                    "p" => {
                        if in_para {
                            flush_para(
                                &mut blocks,
                                &mut current_runs,
                                &current_style_id,
                                &current_num_id,
                                current_num_level,
                                styles,
                                numbering,
                            );
                        }
                        in_para = true;
                        current_style_id = None;
                        current_num_id = None;
                        current_num_level = 0;
                        current_runs.clear();
                    }
                    "r" => {
                        in_run = true;
                        run_bold = false;
                        run_italic = false;
                        run_code = false;
                    }
                    "ins" => {
                        has_tracked_changes = true;
                    }
                    "del" => {
                        in_del = true;
                        has_tracked_changes = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing elements: do not affect nesting_depth
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);

                match tag_local {
                    "pStyle" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                current_style_id = Some(attr.value.to_string());
                            }
                        }
                    }
                    "numId" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                let val = attr.value.to_string();
                                if val != "0" {
                                    current_num_id = Some(val);
                                }
                            }
                        }
                    }
                    "ilvl" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                current_num_level = attr.value.parse().unwrap_or(0);
                            }
                        }
                    }
                    "b" => {
                        if in_run {
                            run_bold = true;
                        }
                    }
                    "i" => {
                        if in_run {
                            run_italic = true;
                        }
                    }
                    "rStyle" => {
                        for attr in e.attributes().flatten() {
                            let k = attr.key.as_ref();
                            let k_local = k.rsplit(':').next().unwrap_or(k);
                            if k_local == "val" {
                                let val = attr.value.to_lowercase();
                                if val.contains("code") {
                                    run_code = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                nesting_depth = nesting_depth.saturating_sub(1);

                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                let tag_local = tag.rsplit(':').next().unwrap_or(tag);

                match tag_local {
                    "p" => {
                        flush_para(
                            &mut blocks,
                            &mut current_runs,
                            &current_style_id,
                            &current_num_id,
                            current_num_level,
                            styles,
                            numbering,
                        );
                        in_para = false;
                        current_runs.clear();
                    }
                    "r" => {
                        in_run = false;
                    }
                    "del" => {
                        in_del = false;
                    }
                    // w:b and w:i also appear as non-empty Start+End in some serialisers
                    "b" => {
                        if in_run {
                            run_bold = true;
                        }
                    }
                    "i" => {
                        if in_run {
                            run_italic = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_del {
                    buf.clear();
                    continue;
                }
                if !in_run || !in_para {
                    buf.clear();
                    continue;
                }

                let text = quick_xml::escape::unescape(e).unwrap_or_default();
                if !text.is_empty() {
                    current_runs.push(gist_model::TextRun {
                        text: text.into_owned(),
                        bold: run_bold,
                        italic: run_italic,
                        code: run_code,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("gist-parse-docx: XML error: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    // Final flush
    if in_para {
        flush_para(
            &mut blocks,
            &mut current_runs,
            &current_style_id,
            &current_num_id,
            current_num_level,
            styles,
            numbering,
        );
    }

    Ok((blocks, has_tracked_changes))
}

fn flush_para(
    blocks: &mut Vec<gist_model::Block>,
    runs: &mut Vec<gist_model::TextRun>,
    style_id: &Option<String>,
    num_id: &Option<String>,
    num_level: usize,
    styles: &StyleMap,
    numbering: &NumberingMap,
) {
    let text: String = runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    if text.trim().is_empty() {
        runs.clear();
        return;
    }

    // Check for list paragraph
    if let Some(nid) = num_id {
        let is_ordered = numbering
            .get(&(nid.clone(), num_level))
            .map(|e| e.is_ordered)
            .unwrap_or(false);
        // M1: each list paragraph becomes a single-item List block.
        // Proper grouping of consecutive items into one List block is a M2 improvement.
        blocks.push(gist_model::Block::List {
            ordered: is_ordered,
            items: vec![text.trim().to_string()],
        });
        runs.clear();
        return;
    }

    // Check for heading via style resolution
    if let Some(sid) = style_id {
        if let Some(entry) = styles.get(sid) {
            if let Some(level) = entry.heading_level {
                blocks.push(gist_model::Block::Heading {
                    level,
                    text: text.trim().to_string(),
                });
                runs.clear();
                return;
            }
        }
    }

    // Normal paragraph
    blocks.push(gist_model::Block::Paragraph {
        runs: std::mem::take(runs),
    });
}

// ── Zip helpers ────────────────────────────────────────────────────────────

/// Reads `path` from `archive`, enforcing `limits.max_expanded_bytes` against
/// `total_expanded` — a running total the caller shares across every part it
/// reads (styles.xml, numbering.xml, document.xml), not a fresh counter per
/// call (N3). Without a shared accumulator, each of those three parts could
/// independently expand up to the full budget, letting a crafted DOCX reach
/// roughly 3x the intended per-document decompression ceiling before any
/// single read trips the cap — this mirrors
/// `gist_parse_epub::parse_spine`'s cumulative `total_expanded` pattern.
fn read_zip_entry_limited(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
    limits: &gist_model::ParseLimits,
    total_expanded: &mut usize,
) -> Result<String, ParseError> {
    let mut entry = archive.by_name(path).map_err(|e| match e {
        zip::result::ZipError::FileNotFound => {
            ParseError::Malformed(format!("required entry not found: {path}"))
        }
        other => ParseError::Zip(other),
    })?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        let n = entry.read(&mut chunk).map_err(ParseError::Io)?;
        if n == 0 {
            break;
        }
        *total_expanded += n;
        if *total_expanded > limits.max_expanded_bytes {
            return Err(ParseError::ResourceLimitExceeded {
                limit: format!("max_expanded_bytes={}", limits.max_expanded_bytes),
                attempted: *total_expanded,
            });
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    String::from_utf8(buf)
        .map_err(|_| ParseError::Malformed(format!("non-UTF-8 content in {path}")))
}

fn read_zip_entry_limited_opt(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
    limits: &gist_model::ParseLimits,
    total_expanded: &mut usize,
) -> Result<Option<String>, ParseError> {
    let exists = match archive.by_name(path) {
        Err(zip::result::ZipError::FileNotFound) => false,
        Err(e) => return Err(ParseError::Zip(e)),
        Ok(_) => true,
    };
    if !exists {
        return Ok(None);
    }
    Ok(Some(read_zip_entry_limited(
        archive,
        path,
        limits,
        total_expanded,
    )?))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gist_model::ParseLimits;

    #[test]
    fn test_reject_oversized_file() {
        let limits = ParseLimits {
            max_bytes: 10,
            ..ParseLimits::default()
        };
        let result = parse(&[0u8; 100], "test", &limits);
        assert!(matches!(
            result,
            Err(ParseError::ResourceLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_zip_entry_count_cap_is_enforced() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        // Build a zip with 6 tiny entries and set the cap to 5 — should be
        // rejected before any entry's content is ever read (F16).
        let mut zip_bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_bytes);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = SimpleFileOptions::default();
            for i in 0..6 {
                writer
                    .start_file(format!("entry{}.txt", i), options)
                    .unwrap();
                writer.write_all(b"x").unwrap();
            }
            writer.finish().unwrap();
        }

        let limits = ParseLimits {
            max_zip_entries: 5,
            ..ParseLimits::default()
        };
        let result = parse(&zip_bytes, "test", &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded { .. })),
            "expected ResourceLimitExceeded for a 6-entry zip capped at 5, got {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_zip_returns_error() {
        let limits = ParseLimits::default();
        let result = parse(b"not a zip file", "test", &limits);
        assert!(result.is_err());
    }

    /// N3 regression test: styles.xml, numbering.xml, and document.xml are
    /// each individually well under `max_expanded_bytes`, but their combined
    /// size exceeds it — this must be rejected. Before N3, each part reset
    /// its own counter to zero, so three parts each just under the cap could
    /// together reach ~3x the intended per-document budget without ever
    /// tripping it.
    #[test]
    fn test_cumulative_expansion_across_parts_is_enforced() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        // Comments are always well-formed XML regardless of size, so padding
        // with them reaches a target byte count without affecting parsing.
        fn padded_xml(root: &str, target_len: usize) -> Vec<u8> {
            let open = format!("<{root}>");
            let close = format!("</{root}>");
            let overhead = open.len() + close.len() + "<!---->".len();
            let pad_len = target_len.saturating_sub(overhead);
            format!("{open}<!--{}-->{close}", "x".repeat(pad_len)).into_bytes()
        }

        // Each part is ~400 bytes (well under a 1000-byte cap individually),
        // but three of them sum to ~1200 > 1000.
        let styles = padded_xml("w:styles", 400);
        let numbering = padded_xml("w:numbering", 400);
        let document = padded_xml("w:document", 400);
        assert!(styles.len() < 1000 && numbering.len() < 1000 && document.len() < 1000);
        assert!(styles.len() + numbering.len() + document.len() > 1000);

        let mut zip_bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_bytes);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = SimpleFileOptions::default();
            writer.start_file("word/styles.xml", options).unwrap();
            writer.write_all(&styles).unwrap();
            writer.start_file("word/numbering.xml", options).unwrap();
            writer.write_all(&numbering).unwrap();
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(&document).unwrap();
            writer.finish().unwrap();
        }

        let limits = ParseLimits {
            max_expanded_bytes: 1000,
            ..ParseLimits::default()
        };
        let result = parse(&zip_bytes, "test", &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded { .. })),
            "expected ResourceLimitExceeded for parts cumulatively over the cap \
             even though each individually stays under it, got {:?}",
            result
        );
    }

    /// Sanity check that the cumulative accounting in the test above isn't
    /// simply over-eager: three parts that are each small AND cumulatively
    /// under the cap must still parse successfully.
    #[test]
    fn test_cumulative_expansion_within_cap_succeeds() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        fn padded_xml(root: &str, target_len: usize) -> Vec<u8> {
            let open = format!("<{root}>");
            let close = format!("</{root}>");
            let overhead = open.len() + close.len() + "<!---->".len();
            let pad_len = target_len.saturating_sub(overhead);
            format!("{open}<!--{}-->{close}", "x".repeat(pad_len)).into_bytes()
        }

        let styles = padded_xml("w:styles", 100);
        let numbering = padded_xml("w:numbering", 100);
        let document = padded_xml("w:document", 100);
        assert!(styles.len() + numbering.len() + document.len() < 1000);

        let mut zip_bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_bytes);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = SimpleFileOptions::default();
            writer.start_file("word/styles.xml", options).unwrap();
            writer.write_all(&styles).unwrap();
            writer.start_file("word/numbering.xml", options).unwrap();
            writer.write_all(&numbering).unwrap();
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(&document).unwrap();
            writer.finish().unwrap();
        }

        let limits = ParseLimits {
            max_expanded_bytes: 1000,
            ..ParseLimits::default()
        };
        let result = parse(&zip_bytes, "test", &limits);
        assert!(
            result.is_ok(),
            "expected Ok when parts are cumulatively under the cap, got {:?}",
            result
        );
    }

    #[test]
    fn test_resolve_heading_level_direct() {
        let mut raw: HashMap<String, (String, Option<String>)> = HashMap::new();
        raw.insert("Heading1".to_string(), ("heading 1".to_string(), None));
        raw.insert("Normal".to_string(), ("Normal".to_string(), None));
        raw.insert(
            "MyHeading".to_string(),
            ("My Heading".to_string(), Some("Heading1".to_string())),
        );

        assert_eq!(resolve_heading_level(&raw, "Heading1", 0), Some(1));
        assert_eq!(resolve_heading_level(&raw, "Normal", 0), None);
        assert_eq!(resolve_heading_level(&raw, "MyHeading", 0), Some(1));
    }
}
