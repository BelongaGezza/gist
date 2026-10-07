# ADR 024 — Android storage sandbox, SAF file ingestion, and backup policy

**Date:** 2026-10-07  
**Status:** Proposed  

## Context

GIST follows strict local-first and sandboxed data rules:
- No data leaves the device (except explicit URL-import requests per ADR-005).
- All library data, documents, and originals live in a sandboxed application directory (ADR-006, ADR-007, ADR-012, ADR-017).
- Copy-on-import (ADR-006): imported source files are copied into `<storage_dir>/originals/` using content-addressed SHA-256 hashes; deletion never touches the user's external original file.
- Zero telemetry and no unexpected cloud uploads (`docs/PRIVACY.md`).

Android has specific storage APIs:
1. **Scoped Storage (Android 10+):** Apps have unrestricted access to their internal directories (`context.filesDir`), but cannot freely access external shared storage without explicit user interaction.
2. **Storage Access Framework (SAF):** User-selected files are accessed through content URIs (`content://...`) via system document pickers.
3. **Android Auto Backup / Cloud Backup:** By default, Android automatically backs up an application's private files to Google Drive unless explicitly disabled or configured.

## Decision

1. **Internal Storage Sandboxing:**
   - Root storage directory is located in the app's internal files directory: `context.filesDir` (typically `/data/user/0/<package>/files/`).
   - Exact subfolder hierarchy matches Apple and Windows:
     - `gist.sqlite3`: SQLite database (metadata, collections, tags, reading progress, FTS5).
     - `storage/`: Parsed Document JSON (`<id>.json`), token streams (`<id>.tokens.json`), and BLAKE3 checksums (`<id>.json.blake3`).
     - `originals/`: Content-addressed SHA-256 copies of imported files (`<sha256>.<ext>`) per ADR-006.
     - `keys/`: Wrapped encryption key (`content-key.keystore`) per ADR-023.
2. **SAF File Ingestion (Copy-on-Import):**
   - User document selection uses the modern Activity Result contract `ActivityResultContracts.OpenDocument()`.
   - The app opens a stream via `contentResolver.openInputStream(uri)`.
   - Content is read in bounded chunks (respecting `ParseLimits`) and copied into a temporary staging file before handing it to `gist-core`'s import pipeline.
   - Once ingested into `originals/` and the SQLite database commits, the temporary file is deleted. The app does **not** retain persistent URI permissions (`takePersistableUriPermission`) to the user's external original file.
   - On library item removal (`delete_source_files: true`), only the internal sandboxed copy in `originals/` is deleted, preserving the external original file.
3. **Android Backup Policy — Exclude Private & Encrypted Data:**
   - Set `android:allowBackup="false"` or configure granular `<data-extraction-rules>` and `<full-backup-content>` XML exclusions:
     - Disallow backing up `gist.sqlite3`, `storage/`, `originals/`, and `keys/` to Google Cloud Drive.
     - Rationale 1 (Privacy): GIST promises users that no personal reading material or library metadata leaves the device without explicit user intent (`docs/PRIVACY.md`). Silent Google Drive backups would violate this guarantee.
     - Rationale 2 (Cryptographic Integrity): Restoring an encrypted library to a new device without the original hardware Keystore master key (ADR-023) leaves all encrypted items permanently undecryptable.
4. **Android Permissions Policy:**
   - Explicitly declare only `android.permission.INTERNET` in `AndroidManifest.xml` (strictly required for URL-paste import per ADR-005).
   - **No broad storage permissions:** Do NOT request `READ_EXTERNAL_STORAGE`, `WRITE_EXTERNAL_STORAGE`, or `MANAGE_EXTERNAL_STORAGE`. All file access is ephemeral and picker-gated via SAF.

## Consequences

- Full compliance with modern Google Play storage requirements and Android 14/15/16 security guidelines.
- Preserves the core privacy guarantee: reading data never leaves the device.
- Uninstallation of the app cleanly removes the entire database, cached originals, and cryptographic keys.
