use gist_model::{Block, Document, Metadata, Section, TextRun};

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
fn check_drm(archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>) -> Result<(), ParseError> {
    let enc_file = match archive.by_name("META-INF/encryption.xml") {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(()), // no encryption.xml = no DRM
        Err(e) => return Err(ParseError::Zip(e)),
    };

    // Read the encryption.xml
    let mut content = String::new();
    use std::io::Read;
    let mut enc_file = enc_file;
    enc_file.read_to_string(&mut content).map_err(ParseError::Io)?;

    // Parse with roxmltree and check EncryptionMethod Algorithm attributes
    let doc = roxmltree::Document::parse(&content)
        .map_err(|e| ParseError::Xml(e.to_string()))?;

    const IDPF_OBFUSCATION: &str = "http://www.idpf.org/2008/embedding";

    for node in doc.descendants() {
        if node.has_tag_name("EncryptionMethod") {
            if let Some(algo) = node.attribute("Algorithm") {
                if algo != IDPF_OBFUSCATION {
                    return Err(ParseError::DrmProtected);
                }
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
    limits: &gist_core::ParseLimits,
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
    let doc = roxmltree::Document::parse(&container)
        .map_err(|e| ParseError::Xml(e.to_string()))?;
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
    let doc = roxmltree::Document::parse(&opf_str)
        .map_err(|e| ParseError::Xml(e.to_string()))?;

    // Extract metadata
    let title = doc.descendants()
        .find(|n| n.has_tag_name("title"))
        .map(|n| n.text().unwrap_or(stem).to_string())
        .unwrap_or_else(|| stem.to_string());

    let author = doc.descendants()
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
                        format!("{}/{}", opf_dir, href)
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
    limits: &gist_core::ParseLimits,
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

        let section_id = format!("s{}", si);
        let blocks = xhtml_to_blocks(&content, limits.max_nesting_depth)?;

        sections.push(Section {
            id: section_id,
            heading: None, // extracted from blocks if first is Heading
            blocks,
        });
    }

    Ok(sections)
}

/// Read a zip entry as a String, enforcing the cumulative expanded-bytes limit.
fn read_zip_entry_limited(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
    max_expanded_bytes: usize,
    accumulated: &mut usize,
) -> Result<String, ParseError> {
    let mut entry = archive.by_name(path).map_err(|_| {
        ParseError::Malformed(format!("spine item not found in archive: {}", path))
    })?;

    use std::io::Read;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = entry.read(&mut chunk).map_err(ParseError::Io)?;
        if n == 0 {
            break;
        }
        *accumulated += n;
        if *accumulated > max_expanded_bytes {
            return Err(ParseError::ResourceLimitExceeded {
                limit: format!("max_expanded_bytes={}", max_expanded_bytes),
                attempted: *accumulated,
            });
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(buf).map_err(|_| ParseError::Malformed("non-UTF-8 XHTML".into()))
}

fn read_zip_entry_string(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    path: &str,
) -> Result<String, ParseError> {
    let mut dummy = 0usize;
    read_zip_entry_limited(archive, path, usize::MAX, &mut dummy)
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
                        limit: format!("max_nesting_depth={}", max_depth),
                        attempted: depth,
                    });
                }
                let name = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
                // Strip namespace prefix if present (e.g. "xhtml:p" → "p")
                let name = name.rsplit(':').next().unwrap_or(&name).to_string();

                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        // Flush any pending block
                        flush_block(&mut blocks, &mut current_runs, &mut in_block);
                        current_heading_level = Some(
                            name.chars()
                                .last()
                                .unwrap()
                                .to_digit(10)
                                .unwrap_or(1) as u8,
                        );
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
                let name = std::str::from_utf8(e.name().as_ref())
                    .unwrap_or("")
                    .to_lowercase();
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
                let text = e.unescape().unwrap_or_default();
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
    use gist_core::ParseLimits;

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
        assert!(matches!(result, Err(ParseError::ResourceLimitExceeded { .. })));
    }

    #[test]
    fn test_xhtml_to_blocks_heading() {
        let xhtml = "<html><body><h1>Title</h1><p>Hello <em>world</em></p></body></html>";
        let blocks = xhtml_to_blocks(xhtml, 200).unwrap();
        assert!(blocks
            .iter()
            .any(|b| matches!(b, Block::Heading { level: 1, text } if text == "Title")));
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
