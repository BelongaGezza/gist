# ADR-004: DRM detection

**Status:** Accepted
**Date:** 2026-09

## Decision

Parse `META-INF/encryption.xml` before reading any ePub content. Inspect every
`<enc:EncryptionMethod Algorithm="...">` URI found in the file:

- Algorithm URI `http://www.idpf.org/2008/embedding` is IDPF font obfuscation.
  This is **not** DRM; it scrambles font files to discourage direct extraction
  but does not encrypt prose content. Continue parsing normally.
- Any other algorithm URI indicates commercial DRM (Adobe ADEPT, Readium LCP,
  etc.). Return `ParseError::DrmProtected` immediately. No content bytes are
  read past this point.

If `META-INF/encryption.xml` is absent the file is unencrypted; proceed.

## Reasoning

- The product spec makes the no-DRM-circumvention policy non-negotiable.
  Encoding it at the parser entry point ensures it cannot be accidentally
  bypassed by callers.
- Checking `encryption.xml` before any content IO is the cheapest possible
  detection: the file is small and at a fixed path inside the ePub ZIP.
- IDPF font obfuscation is part of the open ePub 3 spec; rejecting it would
  break a large corpus of legitimate files. The algorithm URI is stable and
  unambiguous.
- Returning a typed error variant (`ParseError::DrmProtected`) rather than a
  string lets callers pattern-match and display a localised message without
  parsing error text.

## Consequences

- The user sees the message "This ePub is DRM-protected and cannot be imported."
  GIST does not attempt to read, cache, or partially render the content.
- The parser never circumvents DRM, preserving legal compliance on all
  platforms.
- Adding support for a new DRM scheme in future requires only an allowlist
  update in the URI matching logic; the architectural gate stays in place.
- Files using only IDPF font obfuscation import without friction, matching
  user expectations for legitimately purchased ePubs.
