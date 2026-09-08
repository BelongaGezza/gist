//! gist-web — URL fetch + readability extraction
//!
//! Policy: ADR-005 (TLS-only, 5-redirect max, 50 MB cap, 30s connect / 60s read,
//! no cookies, robots.txt pre-check).

use gist_model::{Block, Document, Metadata, Section, TextRun};
use scraper::{Html, Selector};
use url::Url;

// ── Limits ────────────────────────────────────────────────────────────────────

/// Maximum bytes we will buffer from any HTTP response body.
/// Enforces the 50 MB web cap from ADR-005 regardless of what ParseLimits says.
const WEB_MAX_BYTES: usize = 50 * 1024 * 1024;

const MAX_REDIRECTS: u32 = 5;

const USER_AGENT_MAIN: &str = "GIST/1.0 (+https://github.com/your-org/gist)";
const USER_AGENT_ROBOTS: &str = "GIST/1.0";

/// Resource-limit policy. Mirrors gist-core::ParseLimits so callers that already
/// have that struct can pass the same values. Defined locally because gist-core
/// does not yet have a Cargo.toml in the workspace.
#[derive(Debug, Clone)]
pub struct ParseLimits {
    /// Maximum input file size in bytes (default 256 MB).
    pub max_bytes: usize,
    /// Maximum page / spine-item count (default 2 000).
    pub max_pages: usize,
    /// Maximum XML element nesting depth for DOCX/ePub (default 200).
    pub max_nesting_depth: usize,
    /// Maximum decompressed bytes for zip-based formats (default 512 MB).
    pub max_expanded_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        ParseLimits {
            max_bytes: 256 * 1024 * 1024,
            max_pages: 2_000,
            max_nesting_depth: 200,
            max_expanded_bytes: 512 * 1024 * 1024,
        }
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Malformed URL, non-HTTPS scheme, or HTTP-level error.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Response body exceeded the 50 MB cap, or more than 5 redirects were followed.
    #[error("resource limit exceeded")]
    ResourceLimitExceeded,

    /// robots.txt disallows the requested URL for user-agent `*` or `GIST`.
    #[error("robots.txt disallows this URL")]
    RobotsDisallowed,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetch `raw_url` and return a [`Document`] with the readable content.
///
/// Enforces ADR-005: TLS-only, max 5 redirects, 50 MB response cap,
/// 30 s connect / 60 s read timeouts, no cookies, robots.txt pre-check.
pub fn fetch_url(raw_url: &str, limits: &ParseLimits) -> Result<Document, ParseError> {
    // 1. Parse URL.
    let parsed = Url::parse(raw_url)
        .map_err(|e| ParseError::InvalidInput(format!("URL parse: {}", e)))?;

    // 2. Enforce HTTPS.
    if parsed.scheme() != "https" {
        return Err(ParseError::InvalidInput(
            "only HTTPS URLs are supported (ADR-005)".into(),
        ));
    }

    // 3. robots.txt pre-check.
    let robots_url = format!(
        "{}://{}/robots.txt",
        parsed.scheme(),
        parsed.host_str().unwrap_or("")
    );
    let robots_agent = build_agent();
    match robots_agent
        .get(&robots_url)
        .set("User-Agent", USER_AGENT_ROBOTS)
        .call()
    {
        Ok(resp) => {
            let body = read_limited(resp.into_reader(), 512 * 1024).unwrap_or_default();
            let text = String::from_utf8_lossy(&body);
            if is_path_disallowed(&text, parsed.path()) {
                return Err(ParseError::RobotsDisallowed);
            }
        }
        // 4xx/5xx or network error → treat as allowed (ADR-005).
        Err(_) => {}
    }

    // 4. Build agent for main request.
    let agent = build_agent();

    // 5. GET the URL.
    let response = agent
        .get(raw_url)
        .set("User-Agent", USER_AGENT_MAIN)
        .call()
        .map_err(|e| match &e {
            ureq::Error::Status(code, _) if (300..400).contains(code) => {
                // Redirect limit exceeded — ureq surfaces it as a 3xx status error.
                ParseError::ResourceLimitExceeded
            }
            ureq::Error::Status(code, _) => {
                ParseError::InvalidInput(format!("HTTP {}", code))
            }
            _ => ParseError::InvalidInput(e.to_string()),
        })?;

    // Double-check for an unconsumed redirect response (ureq may vary by build).
    let status = response.status();
    if (300..400).contains(&status) {
        return Err(ParseError::ResourceLimitExceeded);
    }

    // 6. Stream body, capped at min(limits.max_bytes, WEB_MAX_BYTES).
    let cap = limits.max_bytes.min(WEB_MAX_BYTES);
    let body_bytes = read_limited(response.into_reader(), cap)?;
    let html = String::from_utf8_lossy(&body_bytes);

    // 7-8. Extract content and build Document.
    Ok(build_document(&html, raw_url))
}

// ── Agent builder ─────────────────────────────────────────────────────────────

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(MAX_REDIRECTS)
        .timeout_connect(std::time::Duration::from_secs(30))
        .timeout_read(std::time::Duration::from_secs(60))
        .build()
}

// ── Body streaming ────────────────────────────────────────────────────────────

/// Read from `reader` into a `Vec<u8>`, returning `Err(ResourceLimitExceeded)`
/// if more than `max_bytes` are available before EOF.
pub(crate) fn read_limited(
    mut reader: impl std::io::Read,
    max_bytes: usize,
) -> Result<Vec<u8>, ParseError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > max_bytes {
                    return Err(ParseError::ResourceLimitExceeded);
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => return Err(ParseError::InvalidInput(e.to_string())),
        }
    }
    Ok(buf)
}

// ── robots.txt ────────────────────────────────────────────────────────────────

/// Returns `true` when `path` is disallowed by the `*` or `GIST` blocks.
///
/// Rules:
/// - An empty `Disallow:` line means "allow all" for that block → ignored.
/// - `Disallow: /` disallows everything.
/// - Matching is prefix-based (no wildcard expansion).
/// - If a `GIST`-specific block is present, its rules are checked alongside `*`.
pub(crate) fn is_path_disallowed(robots_body: &str, path: &str) -> bool {
    let mut star_rules: Vec<String> = Vec::new();
    let mut gist_rules: Vec<String> = Vec::new();

    let mut in_star = false;
    let mut in_gist = false;

    for raw_line in robots_body.lines() {
        let line = raw_line.trim();

        // Blank line ends a rule group.
        if line.is_empty() {
            in_star = false;
            in_gist = false;
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("User-agent:") {
            let agent = rest.trim();
            if agent == "*" {
                in_star = true;
            } else if agent.eq_ignore_ascii_case("GIST") {
                in_gist = true;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("Disallow:") {
            let dp = rest.trim();
            if dp.is_empty() {
                // Empty = nothing disallowed for this block.
                continue;
            }
            if in_star {
                star_rules.push(dp.to_string());
            }
            if in_gist {
                gist_rules.push(dp.to_string());
            }
        }
    }

    // Check GIST-specific rules and wildcard rules (either can disallow).
    gist_rules
        .iter()
        .chain(star_rules.iter())
        .any(|rule| path.starts_with(rule.as_str()))
}

// ── HTML readability extraction ───────────────────────────────────────────────

/// Tags whose subtree should be completely ignored.
const SKIP_TAGS: &[&str] = &[
    "script", "style", "nav", "header", "footer", "noscript", "aside", "iframe",
];

/// Tags that flush the current inline text run into a paragraph.
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "blockquote", "pre",
    "section", "article", "main", "figure", "figcaption", "table", "tr", "td", "th",
    "ul", "ol", "dl", "dt", "dd", "br",
];

/// Build a `Document` from raw HTML, without any network access.
pub(crate) fn build_document(html: &str, url: &str) -> Document {
    let (title, blocks) = extract_content(html);

    let section = Section {
        id: "s0".to_string(),
        heading: None,
        blocks,
    };

    let metadata = Metadata {
        title,
        author: None,
        source_type: "web".to_string(),
        source_ref: Some(url.to_string()),
        import_date: None,
        language: None,
        word_count: 0, // Document::new recomputes this
    };

    Document::new(metadata, vec![section])
}

/// Extract `(title, blocks)` from an HTML string.
pub(crate) fn extract_content(html: &str) -> (String, Vec<Block>) {
    let document = Html::parse_document(html);

    // Title from <title>.
    let title = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Untitled".to_string());

    // Content root: <article> → <main> → <body>.
    let content_html = {
        let try_sel = |s: &str| Selector::parse(s).ok();
        let article = try_sel("article").and_then(|sel| document.select(&sel).next());
        let main = try_sel("main").and_then(|sel| document.select(&sel).next());
        let body = try_sel("body").and_then(|sel| document.select(&sel).next());
        article.or(main).or(body)
    };

    let blocks = match content_html {
        None => vec![],
        Some(root) => collect_blocks(root),
    };

    (title, blocks)
}

/// Walk `el`'s subtree and collect non-empty paragraphs as `Block::Paragraph`.
fn collect_blocks(el: scraper::ElementRef<'_>) -> Vec<Block> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    collect_text(el, &mut current, &mut paragraphs);
    flush(&mut current, &mut paragraphs);

    paragraphs
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| Block::Paragraph {
            runs: vec![TextRun::plain(p.trim().to_string())],
        })
        .collect()
}

fn flush(current: &mut String, paragraphs: &mut Vec<String>) {
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        paragraphs.push(trimmed);
    }
    current.clear();
}

fn collect_text(
    el: scraper::ElementRef<'_>,
    current: &mut String,
    paragraphs: &mut Vec<String>,
) {
    use scraper::node::Node;

    for child in el.children() {
        match child.value() {
            Node::Text(text) => {
                let t = text.trim();
                if !t.is_empty() {
                    if !current.is_empty() && !current.ends_with(' ') {
                        current.push(' ');
                    }
                    current.push_str(t);
                }
            }
            Node::Element(elem) => {
                let tag = elem.name();

                // Skip unwanted subtrees entirely.
                if SKIP_TAGS.contains(&tag) {
                    continue;
                }

                // Block elements flush the current inline run first.
                let is_block = BLOCK_TAGS.contains(&tag);
                if is_block {
                    flush(current, paragraphs);
                }

                if let Some(child_el) = scraper::ElementRef::wrap(child) {
                    collect_text(child_el, current, paragraphs);
                }

                // Flush again after a block element closes.
                if is_block {
                    flush(current, paragraphs);
                }
            }
            _ => {}
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── robots.txt ────────────────────────────────────────────────────────

    #[test]
    fn test_robots_disallowed() {
        // `*` block disallows everything with `Disallow: /`.
        let robots = "User-agent: *\nDisallow: /\n";
        let result = if is_path_disallowed(robots, "/some/page") {
            Err(ParseError::RobotsDisallowed)
        } else {
            Ok(())
        };
        assert!(
            matches!(result, Err(ParseError::RobotsDisallowed)),
            "expected RobotsDisallowed"
        );
    }

    #[test]
    fn test_robots_allowed_with_partial_block() {
        // `*` disallows /private but not /public.
        let robots = "User-agent: *\nDisallow: /private\n";
        assert!(
            !is_path_disallowed(robots, "/public/page"),
            "/public/page should be allowed"
        );
        assert!(
            is_path_disallowed(robots, "/private/secret"),
            "/private/secret should be disallowed"
        );
    }

    #[test]
    fn test_robots_empty_disallow_means_allowed() {
        // An empty Disallow means nothing is blocked.
        let robots = "User-agent: *\nDisallow:\n";
        assert!(!is_path_disallowed(robots, "/anything"));
    }

    #[test]
    fn test_robots_gist_block_respected() {
        // GIST-specific block disallows /gist-only.
        let robots =
            "User-agent: *\nDisallow: /public-block\n\nUser-agent: GIST\nDisallow: /gist-only\n";
        assert!(
            is_path_disallowed(robots, "/gist-only/page"),
            "/gist-only should be disallowed for GIST"
        );
        // /public-block is disallowed by * as well.
        assert!(is_path_disallowed(robots, "/public-block/x"));
    }

    // ── Size limit ────────────────────────────────────────────────────────

    #[test]
    fn test_size_limit() {
        // Feed a synthetic 51 MB reader — should hit ResourceLimitExceeded.
        use std::io::Read as _;
        let fifty_one_mb = 51 * 1024 * 1024;
        let source = std::io::repeat(b'x').take(fifty_one_mb as u64);
        let result = read_limited(source, WEB_MAX_BYTES);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded)),
            "expected ResourceLimitExceeded for 51 MB body"
        );
    }

    #[test]
    fn test_size_within_limit() {
        // A 1-byte body should succeed.
        let source = std::io::Cursor::new(b"hello");
        let result = read_limited(source, WEB_MAX_BYTES);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"hello");
    }

    // ── HTML extraction ───────────────────────────────────────────────────

    #[test]
    fn test_title_extraction() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>My Test Page</title></head>
<body>
<article>
<p>First paragraph with some content.</p>
<p>Second paragraph here.</p>
</article>
</body>
</html>"#;

        let doc = build_document(html, "https://example.com/page");

        assert_eq!(doc.metadata.title, "My Test Page");
        assert_eq!(doc.metadata.source_ref.as_deref(), Some("https://example.com/page"));

        // At least one paragraph block.
        let blocks = &doc.sections[0].blocks;
        assert!(!blocks.is_empty(), "expected at least one block");

        let all_text: String = blocks.iter().map(|b| b.plain_text()).collect::<Vec<_>>().join(" ");
        assert!(
            all_text.contains("First paragraph"),
            "expected article text in output, got: {:?}", all_text
        );
    }

    #[test]
    fn test_strips_script_nav() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Strip Test</title></head>
<body>
<nav>Navigation link</nav>
<script>alert('injected')</script>
<article>
<p>Real content here.</p>
</article>
<footer>Footer text</footer>
</body>
</html>"#;

        let doc = build_document(html, "https://example.com/strip");
        let blocks = &doc.sections[0].blocks;
        let all_text: String = blocks.iter().map(|b| b.plain_text()).collect::<Vec<_>>().join(" ");

        assert!(
            all_text.contains("Real content"),
            "article text should be present"
        );
        assert!(
            !all_text.contains("Navigation link"),
            "nav text must be stripped, got: {:?}", all_text
        );
        assert!(
            !all_text.contains("injected"),
            "script text must be stripped, got: {:?}", all_text
        );
        assert!(
            !all_text.contains("Footer text"),
            "footer text must be stripped, got: {:?}", all_text
        );
    }

    #[test]
    fn test_fallback_to_main_then_body() {
        // No <article>; should fall back to <main>.
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Fallback</title></head>
<body>
<main><p>Main content</p></main>
</body>
</html>"#;

        let doc = build_document(html, "https://example.com/fallback");
        let all_text: String = doc.sections[0]
            .blocks
            .iter()
            .map(|b| b.plain_text())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(all_text.contains("Main content"));
    }

    #[test]
    fn test_source_type_is_web() {
        let html = "<html><head><title>T</title></head><body><p>x</p></body></html>";
        let doc = build_document(html, "https://example.com/");
        assert_eq!(doc.metadata.source_type, "web");
    }
}
