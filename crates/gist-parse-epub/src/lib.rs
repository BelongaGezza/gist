use gist_model::{Block, Document, Metadata, Section, TextRun};

/// Cap on decompressed bytes for the three container/metadata entries
/// (`META-INF/container.xml`, the OPF manifest, `META-INF/encryption.xml`).
/// These are never legitimately more than a few hundred KB even for the
/// largest real ePubs — capped independently of `ParseLimits.max_expanded_bytes`
/// (which budgets spine *content*) so a compression bomb targeting these three
/// files can't exhaust memory before any other limit engages (security
/// register F15). Applied per-file, not cumulatively across the three.
const MAX_METADATA_EXPANDED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml: {0}")]
    Xml(String),
    #[error("this ePub is DRM-protected and cannot be imported")]
    DrmProtected,
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded { limit: String, attempted: usize },
    #[error("malformed ePub: {0}")]
    Malformed(String),
}

/// Check META-INF/encryption.xml for commercial DRM.
/// Returns Ok(()) if no DRM; Err(ParseError::DrmProtected) if found.
/// IDPF font obfuscation (algorithm="http://www.idpf.org/2008/embedding")
/// is NOT DRM and must not trigger rejection.
///
/// Rejects on ambiguity (F20): an `EncryptionMethod` element present but
/// missing its `Algorithm` attribute — malformed-but-otherwise-valid XML —
/// is treated as DRM rather than silently passed through as "no DRM",
/// per ADR-004's intent that GIST never attempts to read encrypted content.
fn check_drm(archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>) -> Result<(), ParseError> {
    let enc_file = match archive.by_name("META-INF/encryption.xml") {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(()), // no encryption.xml = no DRM
        Err(e) => return Err(ParseError::Zip(e)),
    };

    // Read encryption.xml with an expansion cap (F15) — this file is never
    // read via read_zip_entry_limited since by_name() already borrowed the
    // archive here, so we cap it directly with the same accumulator logic.
    let mut accumulated = 0usize;
    let buf = read_capped(enc_file, MAX_METADATA_EXPANDED_BYTES, &mut accumulated)?;
    let content = String::from_utf8(buf)
        .map_err(|_| ParseError::Malformed("non-UTF-8 encryption.xml".into()))?;

    // Parse with roxmltree and check EncryptionMethod Algorithm attributes
    let doc = roxmltree::Document::parse(&content).map_err(|e| ParseError::Xml(e.to_string()))?;

    const IDPF_OBFUSCATION: &str = "http://www.idpf.org/2008/embedding";

    for node in doc.descendants() {
        if node.has_tag_name("EncryptionMethod") {
            match node.attribute("Algorithm") {
                Some(algo) if algo == IDPF_OBFUSCATION => {} // exempted, not DRM
                // No Algorithm attribute at all, or any other algorithm —
                // reject on ambiguity rather than assume "no DRM" (F20).
                _ => return Err(ParseError::DrmProtected),
            }
        }
    }
    Ok(())
}

/// Parse an ePub byte slice into a [`gist_model::Document`].
///
/// `limits` must be checked before significant allocation.
/// Returns [`ParseError::DrmProtected`] if the file has commercial DRM.
pub fn parse(
    bytes: &[u8],
    stem: &str,
    limits: &gist_model::ParseLimits,
) -> Result<Document, ParseError> {
    // 1. Enforce file size limit
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

    // 3. DRM check (before reading any content)
    check_drm(&mut archive)?;

    // 4. Find and parse OPF (container.xml → rootfile path)
    let opf_path = find_opf_path(&mut archive)?;
    let (metadata, spine_items) = parse_opf(&mut archive, &opf_path, stem)?;

    // 5. Enforce max_pages (spine items)
    if spine_items.len() > limits.max_pages {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_pages={}", limits.max_pages),
            attempted: spine_items.len(),
        });
    }

    // 6. Parse each spine item into sections, enforcing max_expanded_bytes
    let sections = parse_spine(&mut archive, &spine_items, limits)?;

    Ok(Document::new(metadata, sections))
}

fn find_opf_path(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
) -> Result<String, ParseError> {
    let container = read_zip_entry_string(archive, "META-INF/container.xml")?;
    let doc = roxmltree::Document::parse(&container).map_err(|e| ParseError::Xml(e.to_string()))?;
    doc.descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .map(|s| s.to_string())
        .ok_or_else(|| ParseError::Malformed("no rootfile in container.xml".into()))
}

fn parse_opf(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    opf_path: &str,
    stem: &str,
) -> Result<(Metadata, Vec<SpineItem>), ParseError> {
    let opf_str = read_zip_entry_string(archive, opf_path)?;
    let doc = roxmltree::Document::parse(&opf_str).map_err(|e| ParseError::Xml(e.to_string()))?;

    // Extract metadata
    let title = doc
        .descendants()
        .find(|n| n.has_tag_name("title"))
        .map(|n| n.text().unwrap_or(stem).to_string())
        .unwrap_or_else(|| stem.to_string());

    let author = doc
        .descendants()
        .find(|n| n.has_tag_name("creator"))
        .and_then(|n| n.text())
        .map(|s| s.to_string());

    // Build manifest: id → href
    let mut manifest: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for node in doc.descendants() {
        if node.has_tag_name("item") {
            if let (Some(id), Some(href)) = (node.attribute("id"), node.attribute("href")) {
                manifest.insert(id.to_string(), href.to_string());
            }
        }
    }

    // Build spine: ordered list of manifest hrefs
    let mut spine_items = Vec::new();
    for node in doc.descendants() {
        if node.has_tag_name("itemref") {
            if let Some(idref) = node.attribute("idref") {
                if let Some(href) = manifest.get(idref) {
                    // Resolve href relative to OPF directory
                    let opf_dir = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    let resolved = if opf_dir.is_empty() {
                        href.clone()
                    } else {
                        format!("{opf_dir}/{href}")
                    };
                    spine_items.push(SpineItem {
                        _id: idref.to_string(),
                        href: resolved,
                    });
                }
            }
        }
    }

    let metadata = Metadata {
        title,
        author,
        source_type: "epub".to_string(),
        source_ref: None,
        source_copy_ref: None,
        import_date: None,
        language: None,
        word_count: 0, // recomputed by Document::new
    };

    Ok((metadata, spine_items))
}

struct SpineItem {
    _id: String,
    href: String,
}

fn parse_spine(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    spine_items: &[SpineItem],
    limits: &gist_model::ParseLimits,
) -> Result<Vec<Section>, ParseError> {
    let mut sections = Vec::new();
    let mut total_expanded: usize = 0;

    for (si, item) in spine_items.iter().enumerate() {
        // Read XHTML with expanded-bytes tracking
        let content = read_zip_entry_limited(
            archive,
            &item.href,
            limits.max_expanded_bytes,
            &mut total_expanded,
        )?;

        let section_id = format!("s{si}");
        let blocks = xhtml_to_blocks(&content, limits.max_nesting_depth)?;

        sections.push(Section {
            id: section_id,
            heading: None, // extracted from blocks if first is Heading
            blocks,
        });
    }

    Ok(sections)
}

/// Read `reader` fully into a `Vec<u8>`, enforcing `max_bytes` incrementally
/// (not after full decompression) so a compression bomb is caught mid-read
/// rather than after it has already been fully expanded into memory.
/// `accumulated` lets callers share one running budget across multiple reads
/// (e.g. spine content, per `parse_spine`) or track a single read in
/// isolation by passing a fresh `0`.
fn read_capped(
    mut reader: impl std::io::Read,
    max_bytes: usize,
    accumulated: &mut usize,
) -> Result<Vec<u8>, ParseError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).map_err(ParseError::Io)?;
        if n == 0 {
            break;
        }
        *accumulated += n;
        if *accumulated > max_bytes {
            return Err(ParseError::ResourceLimitExceeded {
                limit: format!("max_expanded_bytes={max_bytes}"),
                attempted: *accumulated,
            });
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

/// Read a zip entry as a String, enforcing the cumulative expanded-bytes limit.
fn read_zip_entry_limited(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
    max_expanded_bytes: usize,
    accumulated: &mut usize,
) -> Result<String, ParseError> {
    let entry = archive
        .by_name(path)
        .map_err(|_| ParseError::Malformed(format!("spine item not found in archive: {path}")))?;
    let buf = read_capped(entry, max_expanded_bytes, accumulated)?;
    String::from_utf8(buf).map_err(|_| ParseError::Malformed("non-UTF-8 XHTML".into()))
}

/// Read a container/metadata zip entry (container.xml, OPF) as a String,
/// capped at `MAX_METADATA_EXPANDED_BYTES` (F15) — independent of
/// `ParseLimits.max_expanded_bytes`, which budgets spine content, not
/// metadata read before the spine is even known.
fn read_zip_entry_string(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
) -> Result<String, ParseError> {
    let mut accumulated = 0usize;
    read_zip_entry_limited(archive, path, MAX_METADATA_EXPANDED_BYTES, &mut accumulated)
}

/// Map XHTML content to a Vec<Block>.
/// Whitelist: h1..h6 → Heading, p/div → Paragraph, ul/ol → List,
/// em/strong/i/b/code → TextRun marks.
/// Enforces nesting depth limit.
fn xhtml_to_blocks(xhtml: &str, max_depth: usize) -> Result<Vec<Block>, ParseError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut blocks = Vec::new();
    let mut reader = Reader::from_str(xhtml);

    let mut depth: usize = 0;
    let mut current_runs: Vec<TextRun> = Vec::new();
    let mut in_block = false;
    let mut current_bold = false;
    let mut current_italic = false;
    let mut current_code = false;
    let mut list_ordered = false;
    let mut in_list = false;
    let mut list_items: Vec<String> = Vec::new();
    let mut current_heading_level: Option<u8> = None;
    let mut current_heading_text = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                if depth > max_depth {
                    return Err(ParseError::ResourceLimitExceeded {
                        limit: format!("max_nesting_depth={max_depth}"),
                        attempted: depth,
                    });
                }
                let name = e.name().as_ref().to_lowercase();
                // Strip namespace prefix if present (e.g. "xhtml:p" → "p")
                let name = name.rsplit(':').next().unwrap_or(&name).to_string();

                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        // Flush any pending block
                        flush_block(&mut blocks, &mut current_runs, &mut in_block);
                        current_heading_level =
                            Some(name.chars().last().unwrap().to_digit(10).unwrap_or(1) as u8);
                        current_heading_text.clear();
                        in_block = true;
                    }
                    "p" | "div" => {
                        flush_block(&mut blocks, &mut current_runs, &mut in_block);
                        in_block = true;
                    }
                    "ul" => {
                        in_list = true;
                        list_ordered = false;
                        list_items.clear();
                    }
                    "ol" => {
                        in_list = true;
                        list_ordered = true;
                        list_items.clear();
                    }
                    "li" => {
                        // list item text accumulated in Text event below
                    }
                    "em" | "i" => current_italic = true,
                    "strong" | "b" => current_bold = true,
                    "code" => current_code = true,
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                depth = depth.saturating_sub(1);
                let name = e.name().as_ref().to_lowercase();
                let name = name.rsplit(':').next().unwrap_or(&name).to_string();

                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if let Some(level) = current_heading_level.take() {
                            if !current_heading_text.trim().is_empty() {
                                blocks.push(Block::Heading {
                                    level,
                                    text: current_heading_text.trim().to_string(),
                                });
                            }
                        }
                        in_block = false;
                    }
                    "p" | "div" => {
                        flush_block(&mut blocks, &mut current_runs, &mut in_block);
                    }
                    "ul" | "ol" => {
                        if !list_items.is_empty() {
                            blocks.push(Block::List {
                                ordered: list_ordered,
                                items: list_items.clone(),
                            });
                        }
                        in_list = false;
                        list_items.clear();
                    }
                    "li" => {
                        // Finalise pending runs into a list item string
                        let text: String = current_runs
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<Vec<_>>()
                            .join("");
                        if !text.trim().is_empty() {
                            list_items.push(text.trim().to_string());
                        }
                        current_runs.clear();
                    }
                    "em" | "i" => current_italic = false,
                    "strong" | "b" => current_bold = false,
                    "code" => current_code = false,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = quick_xml::escape::unescape(e).unwrap_or_default();
                let text = text.trim();
                if text.is_empty() {
                    buf.clear();
                    continue;
                }

                if current_heading_level.is_some() {
                    current_heading_text.push_str(text);
                } else if in_list {
                    // Accumulate into runs for current li
                    current_runs.push(TextRun {
                        text: text.to_string(),
                        bold: current_bold,
                        italic: current_italic,
                        code: current_code,
                    });
                } else if in_block {
                    current_runs.push(TextRun {
                        text: text.to_string(),
                        bold: current_bold,
                        italic: current_italic,
                        code: current_code,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("gist-parse-epub: XML parse error (skipping): {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    // Flush any trailing block
    flush_block(&mut blocks, &mut current_runs, &mut in_block);

    // Filter empty blocks
    let blocks: Vec<_> = blocks
        .into_iter()
        .filter(|b| match b {
            Block::Paragraph { runs } => runs.iter().any(|r| !r.text.trim().is_empty()),
            Block::Heading { text, .. } => !text.trim().is_empty(),
            Block::List { items, .. } => !items.is_empty(),
            Block::Image { .. } => true,
        })
        .collect();

    Ok(if blocks.is_empty() {
        vec![Block::Paragraph {
            runs: vec![TextRun::plain("")],
        }]
    } else {
        blocks
    })
}

fn flush_block(blocks: &mut Vec<Block>, runs: &mut Vec<TextRun>, in_block: &mut bool) {
    if *in_block && !runs.is_empty() {
        blocks.push(Block::Paragraph {
            runs: std::mem::take(runs),
        });
    }
    *in_block = false;
    runs.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gist_model::ParseLimits;

    #[test]
    fn test_parse_error_on_empty_bytes() {
        let limits = ParseLimits::default();
        // Empty bytes → zip error (not a valid zip)
        let result = parse(&[], "test", &limits);
        assert!(result.is_err());
    }

    #[test]
    fn test_resource_limit_exceeded_on_large_file() {
        let limits = ParseLimits {
            max_bytes: 10,
            ..ParseLimits::default()
        };
        let dummy = vec![0u8; 100];
        let result = parse(&dummy, "test", &limits);
        assert!(matches!(
            result,
            Err(ParseError::ResourceLimitExceeded { .. })
        ));
    }

    /// Builds a minimal zip whose only entry is `META-INF/encryption.xml`
    /// with the given content — sufficient to exercise `check_drm`, since it
    /// runs before container.xml/OPF are ever read.
    fn build_zip_with_encryption_xml(content: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut zip_bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_bytes);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = SimpleFileOptions::default();
            writer
                .start_file("META-INF/encryption.xml", options)
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        zip_bytes
    }

    #[test]
    fn test_ambiguous_encryption_method_is_rejected_as_drm() {
        // EncryptionMethod present but missing its Algorithm attribute —
        // malformed-but-valid XML that must reject on ambiguity (F20),
        // not silently pass through as "no DRM".
        let xml = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
            <EncryptedData>
                <EncryptionMethod/>
            </EncryptedData>
        </encryption>"#;
        let zip_bytes = build_zip_with_encryption_xml(xml);
        let limits = ParseLimits::default();
        let result = parse(&zip_bytes, "test", &limits);
        assert!(
            matches!(result, Err(ParseError::DrmProtected)),
            "expected DrmProtected for an ambiguous EncryptionMethod, got {:?}",
            result
        );
    }

    #[test]
    fn test_idpf_font_obfuscation_is_not_drm() {
        // The one recognized exemption must still pass through cleanly.
        let xml = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
            <EncryptedData>
                <EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
            </EncryptedData>
        </encryption>"#;
        let zip_bytes = build_zip_with_encryption_xml(xml);
        let limits = ParseLimits::default();
        let result = parse(&zip_bytes, "test", &limits);
        // Parsing continues past check_drm and fails later (no container.xml
        // in this minimal fixture) — the point is it's NOT DrmProtected.
        assert!(
            !matches!(result, Err(ParseError::DrmProtected)),
            "IDPF font obfuscation must not be treated as DRM, got {:?}",
            result
        );
    }

    #[test]
    fn test_commercial_drm_algorithm_is_rejected() {
        let xml = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
            <EncryptedData>
                <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>
            </EncryptedData>
        </encryption>"#;
        let zip_bytes = build_zip_with_encryption_xml(xml);
        let limits = ParseLimits::default();
        let result = parse(&zip_bytes, "test", &limits);
        assert!(matches!(result, Err(ParseError::DrmProtected)));
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
    fn test_xhtml_to_blocks_heading() {
        let xhtml = "<html><body><h1>Title</h1><p>Hello <em>world</em></p></body></html>";
        let blocks = xhtml_to_blocks(xhtml, 200).unwrap();
        assert!(blocks
            .iter()
            .any(|b| matches!(b, Block::Heading { level: 1, text } if text == "Title")));
    }

    /// Builds a minimal zip with a single, highly compressible entry at
    /// `path` that expands to `expanded_size` bytes — a stand-in for a
    /// zip-bomb targeting one of the container/metadata reads (F15).
    fn build_zip_bomb(path: &str, expanded_size: usize) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let content = vec![b'A'; expanded_size]; // trivially compressible
        let mut zip_bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_bytes);
            let mut writer = zip::ZipWriter::new(cursor);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(path, options).unwrap();
            writer.write_all(&content).unwrap();
            writer.finish().unwrap();
        }
        zip_bytes
    }

    #[test]
    fn test_container_xml_zip_bomb_is_capped() {
        let bomb = build_zip_bomb("META-INF/container.xml", MAX_METADATA_EXPANDED_BYTES + 1024);
        let limits = ParseLimits::default();
        let result = parse(&bomb, "test", &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded { .. })),
            "expected ResourceLimitExceeded for oversized container.xml, got {:?}",
            result
        );
    }

    #[test]
    fn test_encryption_xml_zip_bomb_is_capped() {
        // check_drm runs before container.xml is even read, so a bomb here
        // must be caught without needing a valid rootfile/OPF at all.
        let bomb = build_zip_bomb(
            "META-INF/encryption.xml",
            MAX_METADATA_EXPANDED_BYTES + 1024,
        );
        let limits = ParseLimits::default();
        let result = parse(&bomb, "test", &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded { .. })),
            "expected ResourceLimitExceeded for oversized encryption.xml, got {:?}",
            result
        );
    }

    #[test]
    fn test_xhtml_to_blocks_list() {
        let xhtml = "<html><body><ul><li>One</li><li>Two</li></ul></body></html>";
        let blocks = xhtml_to_blocks(xhtml, 200).unwrap();
        assert!(blocks
            .iter()
            .any(|b| matches!(b, Block::List { ordered: false, items } if items.len() == 2)));
    }
}
