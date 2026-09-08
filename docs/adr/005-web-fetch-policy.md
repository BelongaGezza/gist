# ADR-005: Web fetch policy

**Status:** Accepted
**Date:** 2026-09

## Decision

URL import in `gist-web` follows these hard rules:

| Constraint | Value |
|---|---|
| Transport | TLS only via `rustls`; plain HTTP connections are refused |
| Redirects | Maximum 5; abort with `FetchError::TooManyRedirects` if exceeded |
| Response size | 50 MB; reject before buffering if `Content-Length` exceeds limit or streaming bytes exceed limit |
| Connect timeout | 30 s |
| Total transfer timeout | 60 s |
| Cookie jar | None; no cookies sent or stored |
| User-Agent | `GIST/1.0 (+https://github.com/your-org/gist)` |
| `robots.txt` | Fetched and respected before the target URL; `User-agent: *` Disallow rules are honoured |
| `robots.txt` timeout | 5 s; failure to fetch → proceed with import |
| Paywall / auth errors | HTTP 401, 402, 403 → surface specific `FetchError` variant; no bypass attempted |
| Data egress | Only the target URL leaves the device; no image data, no personal data |

## Reasoning

- `rustls` is a pure-Rust TLS stack with no system-library dependency and a
  strong security track record; it eliminates an entire class of OS-level TLS
  misconfiguration bugs.
- A 5-redirect cap prevents redirect loops while accommodating common CDN and
  canonical-URL patterns.
- The 50 MB response cap protects device memory and avoids accidental ingestion
  of binary blobs served at text URLs.
- Separate connect and transfer timeouts handle both unresponsive hosts and
  throttled connections.
- No cookie jar ensures GIST cannot be used as a persistent session tracker;
  each import is stateless.
- Honouring `robots.txt` with a 5 s deadline balances politeness with user
  experience: an unresponsive robots.txt should not block an import indefinitely.
- Surfacing 401/402/403 as typed errors lets the UI display actionable messages
  ("This page requires a subscription") rather than an opaque failure.

## Consequences

- GIST cannot import plain-HTTP URLs. Users attempting to import such URLs
  receive `FetchError::PlainHttp`.
- Redirecting chains longer than 5 hops (unusual in practice) fail at import
  time with a clear error.
- Pages served behind authentication walls are not importable; this is
  intentional and disclosed to the user.
- No personal data or cookies leave the device as a side-effect of a URL
  import.
- `robots.txt` compliance is best-effort given the 5 s timeout; sites that
  rely on crawl-delay directives are not throttled.
