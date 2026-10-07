# GIST for Android — Development Plan

**Status:** Draft v1, 2026-10-07.  
**Companion Documents:** [`docs/ARCHITECTURE.md`](./ARCHITECTURE.md), [`docs/product-spec-reader-app-v3.md`](./product-spec-reader-app-v3.md), [`docs/iconspecification.md`](./iconspecification.md), and ADRs 021–026.  
**Purpose:** Define the architectural blueprint, platform integration, and phased implementation milestones for the native Android client (`apps/android`).

---

## 1. Verified Starting Point & Toolchain Baseline

Measured on the development environment (Linux x86_64 / Darkstar, 2026-10-07):

| Requirement | Status / Evidence |
|---|---|
| **Rust Core & FFI Crate** | **Verified.** Rust 1.99.0 toolchain pinned in `rust-toolchain.toml`. `cargo build -p gist-ffi` produces `target/debug/libgist_ffi.so` and `libgist_ffi.a` cleanly. All 117 workspace tests pass. |
| **UniFFI Kotlin Generation** | **Verified (A0 spike, 2026-09-07).** Upstream UniFFI 0.32 bundled in `gist-ffi` supports Kotlin out-of-the-box (`--language kotlin`). `./target/debug/uniffi-bindgen generate target/debug/libgist_ffi.so --language kotlin --out-dir ...` successfully generates `uniffi/gist_ffi/gist_ffi.kt` (5,781 lines) with complete interfaces for `GistCoreInterface`, `KeyProvider`, and `OcrEngine`. |
| **JDK Environment** | **Verified.** OpenJDK 21 installed (`/usr/lib/jvm/java-21-openjdk-amd64`). Compatible with Gradle 8.10+ and Kotlin 2.x. |
| **Android SDK & Platforms** | **Installed locally.** Android SDK at `/home/gerry/Android/Sdk` with API platforms `android-34`, `android-35`, and `android-36`; platform-tools (`adb`) present. |
| **Android NDK** | **Installed locally.** NDK r28 (`28.2.13676358`) installed at `/home/gerry/Android/Sdk/ndk/28.2.13676358`. Supports modern Clang/LLVM toolchains for `aarch64-linux-android` (`arm64-v8a`), `armv7-linux-androideabi` (`armeabi-v7a`), and `x86_64-linux-android`. |
| **Cross-compilation Tool** | `cargo-ndk` is the established standard tool for binding Cargo cross-compilation with Android NDK sysroots and target ABIs. |

---

## 2. Architecture

The Android shell follows the **Rust core + native UI shell** architecture established by Apple (SwiftUI) and Windows (WinUI 3).

```
apps/android/
├── build.gradle.kts                # Root project build configuration
├── settings.gradle.kts             # Module definitions (:core, :app)
├── gradle/wrapper/                 # Pinned Gradle wrapper (8.10+)
├── core/                           # Android library module (headless, pure JVM/Android testable)
│   ├── build.gradle.kts            # Dependencies: JNA, kotlinx.coroutines, security-crypto
│   ├── src/main/java/
│   │   ├── uniffi/gist/            # Gitignored: generated Kotlin bindings (tools/gen-bindings-kt.sh)
│   │   └── app/gist/core/
│   │       ├── CoreClient.kt       # Singleton manager, Coroutine dispatchers, FFI wrapper
│   │       ├── models/             # FlowDocumentVM, LibraryItemVM, CollectionVM, SortOrder
│   │       ├── keys/               # AndroidKeystoreKeyProvider (ADR-023)
│   │       └── storage/            # AndroidStoragePaths, SAF stream helpers
│   └── src/test/java/              # JUnit / Robolectric tests against real GistCore
├── app/                            # Android application module (Jetpack Compose UI)
│   ├── build.gradle.kts            # Compose BOM, Material 3, CameraX, ML Kit
│   ├── src/main/
│   │   ├── AndroidManifest.xml     # Package definition, INTERNET permission, no broad storage
│   │   ├── jniLibs/                # Staged native binaries: arm64-v8a, armeabi-v7a, x86_64
│   │   │   ├── arm64-v8a/          # libgist_ffi.so, libpdfium.so
│   │   │   ├── armeabi-v7a/        # libgist_ffi.so, libpdfium.so
│   │   │   └── x86_64/             # libgist_ffi.so, libpdfium.so
│   │   ├── res/                    # Adaptive icon XMLs (docs/iconspecification.md), strings
│   │   └── java/app/gist/ui/
│   │       ├── MainActivity.kt     # Single-activity container, Navigation Compose
│   │       ├── theme/              # Material 3 Dynamic Color, Sepia, Dark, Light, OLED True Black
│   │       ├── library/            # LibraryScreen, SearchBar, CollectionDrawer, TagDialog
│   │       ├── reader/
│   │       │   ├── flow/           # FlowReaderScreen (LazyColumn, typography, nested TOC)
│   │       │   └── rsvp/           # RsvpReaderScreen (ORP display, Rotary/gesture speed dial)
│   │       ├── ocr/                # CameraCaptureScreen (CameraX), OcrReviewScreen
│   │       └── ocr/engine/         # AndroidMlKitOcrEngine (ADR-025)
tools/
├── build-core-android.sh           # cargo-ndk build for arm64-v8a, armeabi-v7a, x86_64
├── fetch-pdfium-android.sh         # Download & verify SHA-256 pinned libpdfium.so
└── gen-bindings-kt.sh              # uniffi-bindgen Kotlin generation into apps/android/core/
```

### Layering Parity

| Layer | Apple (macOS / iOS) | Windows (WinUI 3) | Android (Jetpack Compose) |
|---|---|---|---|
| **FFI Scaffold** | `gist-ffi` (UniFFI 0.32) | `gist-ffi` (UniFFI C# PR #176) | `gist-ffi` (UniFFI 0.32 native Kotlin) |
| **Native Binary** | `.xcframework` (staticlib) | `gist_ffi.dll` (cdylib, x64/arm64) | `libgist_ffi.so` (cdylib in `jniLibs/<abi>`) |
| **Client Layer** | `CoreClient` (`@MainActor`) | `CoreClient` (`ObservableObject`) | `CoreClient` (`StateFlow` + Coroutines) |
| **Threading Model** | Swift async/await + actors | `Task.Run` + `DispatcherQueue` | `withContext(Dispatchers.IO)` |
| **UI Framework** | SwiftUI | WinUI 3 (XAML) | Jetpack Compose (Material Design 3) |
| **Document Flow** | `LazyVStack` | `ItemsRepeater` | `LazyColumn` (virtualized blocks) |
| **RSVP Pacing** | `CVDisplayLink` / `Task.sleep` | `DispatcherQueueTimer` monotonic | `Choreographer` / monotonic nano-clock |
| **Key Custody** | Apple Keychain (`SecItemAdd`) | Windows DPAPI (`content-key.dpapi`) | Android Keystore TEE/StrongBox (`GAK1`) |
| **OCR Recognition**| Vision framework | `Windows.Media.Ocr` | Bundled Google ML Kit (`com.google.mlkit`) |
| **PDF Backend** | Embedded `libpdfium.dylib` | Trait stub / future PDFium | Pinned `libpdfium.so` (`pdfium-render`) |

---

## 3. Key Decisions & ADRs

| ADR | Decision | Rationale |
|---|---|---|
| [**021 Android UI stack**](./adr/021-android-ui-stack.md) | **Kotlin 2.x + Jetpack Compose** | Modern Android Development (MAD) standard; declarative model matches SwiftUI and WinUI; seamless theming (Material 3 Dynamic Color + OLED True Black). |
| [**022 Android FFI binding**](./adr/022-android-ffi-binding.md) | **UniFFI Kotlin bindings + Android NDK** | UniFFI 0.32 has native Kotlin support; zero API drift across platforms; `cargo-ndk` cross-compiles clean `.so` libraries for `arm64-v8a`, `armeabi-v7a`, and `x86_64`. |
| [**023 Android key custody**](./adr/023-android-key-custody.md) | **Hardware-backed Android Keystore** | AES-256 master key generated in TEE/StrongBox hardware wrapping a random 32-byte content key stored in `content-key.keystore` (`GAK1` magic). Race-safe atomic creation; fail-closed corruption detection. |
| [**024 Android storage & sandbox**](./adr/024-android-storage-and-sandbox.md) | **Scoped Storage, SAF, and cloud backup exclusion** | All app data in private `context.filesDir`; copy-on-import (ADR-006) preserves external user files; `android:allowBackup="false"` prevents leaking personal reading data to Google Drive. Only `INTERNET` permission requested. |
| [**025 Android OCR engine**](./adr/025-android-ocr-engine.md) | **Bundled on-device Google ML Kit Text Recognition** | Zero network traffic, 100% on-device, privacy-preserving; works on de-Googled devices; CameraX batch document scanner; full confidence scores. |
| [**026 Android PDF text extraction**](./adr/026-android-pdf-engine.md) | **Pinned prebuilt `libpdfium.so` via `pdfium-render`** | Android framework cannot extract PDF text; `libpdfium.so` dynamically loaded across ABIs; SHA-256 fail-closed build verification; BSD-3 license compliance. |

---

## 4. Cross-Cutting Engineering Work

### 4.1 Development Prerequisites & Toolchain Setup

To develop or build the Android shell locally:
1. **JDK 21:** Configured via `JAVA_HOME`.
2. **Android SDK:** Command-line tools, platform SDKs 34/35/36, build-tools 35.x.
3. **Android NDK:** NDK r28+ (`ANDROID_NDK_HOME`).
4. **Rust Targets:**
   ```bash
   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
   cargo install cargo-ndk --locked
   ```
5. **UniFFI CLI:** Built from the workspace's own `gist-ffi` crate:
   ```bash
   cargo build -p gist-ffi --bin uniffi-bindgen --features uniffi-bindgen-bin
   ```

### 4.2 CI/CD and Supply Chain (`android-build.yml`)

Add `.github/workflows/android-build.yml` triggered on changes to `apps/android/**`, `crates/**`, and Android build tooling:
- **Runner:** `ubuntu-latest`.
- **JDK & NDK Setup:** Use `actions/setup-java@v4` (Java 21) and setup Android SDK/NDK r28.
- **Action Pinning:** All GitHub Actions pinned to immutable 40-character commit SHAs; `permissions: contents: read`.
- **Dependabot:** Add `gradle` ecosystem entry for `/apps/android` in `.github/dependabot.yml` in the same PR.
- **Vulnerability Audit:** Run Gradle dependency check in CI to detect vulnerable libraries.

### 4.3 Monotonic Clock & RSVP Frame Pacing

RSVP reading flashes words at up to 1000 WPM (60 ms per word). Hand-rolled `Thread.sleep` or unanchored coroutine delays suffer from timer drift and frame jitter:
1. Android displays run at variable refresh rates (60 Hz, 90 Hz, 120 Hz).
2. The Android RSVP player anchors to `android.os.SystemClock.elapsedRealtimeNanos()` and coordinates with `android.view.Choreographer` (or Compose's `withFrameNanos`).
3. Each frame computes elapsed session time and requests the current token from `gist-rsvp` via the FFI monotonic contract, eliminating timer drift.

### 4.4 Data Privacy & Backup Invariant

In accordance with [`docs/PRIVACY.md`](./PRIVACY.md):
- `gist.sqlite3`, `storage/`, `originals/`, and `keys/` are stored strictly in `context.filesDir`.
- `AndroidManifest.xml` explicitly sets `android:allowBackup="false"` or defines an XML `<data-extraction-rules>` configuration explicitly blocking `cloud-backup` and `device-transfer` for internal storage.
- Storage Access Framework ensures that no broad storage permission (`READ_EXTERNAL_STORAGE`) is ever requested.

---

## 5. Phased Milestone Plan

### Phase A0 — Android Feasibility, NDK Spike & Architecture Gate (1–2 weeks)

**Goal:** Prove the Rust core compiles for Android ABIs, verify UniFFI Kotlin bindings in an isolated test harness, and establish the Gradle project structure.

- Verify `cargo-ndk` build of `gist-ffi` for `aarch64-linux-android` and `x86_64-linux-android`.
- Generate Kotlin bindings via `tools/gen-bindings-kt.sh` into `apps/android/core`.
- Build a headless JVM/Robolectric test spike executing the 20 canonical core checks:
  - Database initialization, import plain text, list items, search via FTS5 (prefix and hostile query).
  - Document JSON retrieval, RSVP token inspection, collections, and tags.
  - Fail-closed DRM rejection (`GistException.DrmProtected`).
  - Panic containment across JNA: trigger synthetic panic, verify it surfaces as `GistException.InternalPanic` without crashing the JVM.
- Build Keystore prototype verifying hardware-backed wrapping, atomic creation, and fail-closed corrupt file detection.
- Add `.github/workflows/android-build.yml` (non-blocking initially).

**Exit Criteria:** A0 spike passes 20/20 headless checks; ADRs 021–026 formally adopted; `PLATFORM_VERIFICATION.md` updated with measured Android results.

---

### Phase A1 — App Shell, CoreClient & Storage Paths (2–3 weeks)

**Goal:** Establish the production Android app shell, lifecycle-aware `CoreClient`, and secure key custody.

- Scaffold `apps/android/` with `:core` and `:app` Gradle modules.
- Implement `CoreClient.kt` in `:core`:
  - Coroutine-based asynchronous dispatch (`Dispatchers.IO`).
  - Expose `StateFlow<List<LibraryItemVM>>`, `StateFlow<List<CollectionVM>>`, and `StateFlow<Theme>`.
  - Comprehensive exception mapping (`GistException` → user-facing localized errors).
- Implement `AndroidKeystoreKeyProvider` (ADR-023) in `:core`:
  - TEE/StrongBox master key generation.
  - Race-safe temp-file-to-rename publishing.
  - Unit tests covering concurrent initialization and corrupted-key recovery.
- Implement `AndroidStoragePaths`:
  - Resolve paths within `context.filesDir`.
  - Validate copy-on-import directory (`originals/`).
- Shell UI: Minimal Jetpack Compose activity displaying empty library state, navigation scaffold, and theme container.

**Exit Criteria:** App launches on Android emulator and physical device; `CoreClient` initializes store with `newWithReadKey`; temp-dir unit tests verify key custody and store CRUD.

---

### Phase A2 — Library Management, SAF Ingestion & Per-Item Encryption (2–3 weeks)

**Goal:** Deliver full library management, file import via Storage Access Framework, and per-item encryption.

- **Storage Access Framework (SAF) Ingestion:**
  - File picker via `ActivityResultContracts.OpenDocument()`.
  - Support MIME types: `text/plain`, `application/epub+zip`, `application/vnd.openxmlformats-officedocument.wordprocessingml.document`, `application/pdf`.
  - Stream input bytes into temporary staging, run `gist-core` import, and verify content-addressed copy in `originals/` (ADR-006).
  - Web URL import dialogue: URL input sheet, call `CoreClient.importUrl()`, handle SSRF and redirect rejections.
- **Library UI (Jetpack Compose):**
  - Grid view (thumbnails/cards) and list view with selection mode.
  - Persistent sort controls: Name (A-Z/Z-A), Source Type, Date Added, Date Last Read, Reading Progress.
  - Top search bar with 300 ms debounce for FTS5 full-text queries.
  - Sidebar / Navigation Drawer for user collections and smart collections.
  - Tag editor dialog for single and multiple items.
- **Removal & Encryption:**
  - Single and bulk removal dialog with clear confirmation: "Remove from Library" vs "Also Delete Internal Copy". Assert external file is never touched.
  - Opt-in "Encrypt" action (ADR-014): encrypt item blobs via Keystore key, display lock icon indicator.

**Exit Criteria:** Automated tests verify SAF import, FTS search, collection filtering, non-deletion of external source files, and encrypted item read-back. Manual click-through on physical Android device verified.

---

### Phase A3 — Flow Reader & RSVP Speed-Reading Views (2–3 weeks)

**Goal:** Deliver both core reading experiences with full typography, progress tracking, and high-performance pacing.

- **Flow Reader (`FlowReaderScreen`):**
  - Virtualized rendering via `LazyColumn` mapping document IR blocks (`Heading`, `Paragraph`, `List`, `Table`, `Image`).
  - Typography bottom sheet: font size slider, line spacing options (compact, regular, relaxed), font design (system default, serif, rounded, monospace for code).
  - Nested Table of Contents (TOC) bottom sheet navigating smoothly to section block IDs.
  - In-document search: find bar, match count indicator, jump next/previous chevron, match highlighting via `AnnotatedString`.
  - Reading progress: compute fraction from visible block indices; persist progress to `FlowScrollPositionStore` on scroll.
- **RSVP Reader (`RsvpReaderScreen`):**
  - Optimal Recognition Point (ORP) rendering: center word at the focal character in high-contrast typography.
  - Controls: prominent play/pause button (tap anywhere or spacebar on Bluetooth keyboards); seek backward/forward 10 tokens.
  - Speed Dial: custom touch-rotary dial / slider supporting 100–1000 WPM in real-time with haptic feedback (`HapticFeedbackConstants.CLOCK_TICK`).
  - Monotonic frame-synced loop via `Choreographer` preventing timer drift.
  - Save progress token index to `reading_progress` table on pause or exit.

**Exit Criteria:** Flow reader smoothly scrolls long fixtures (e.g. 20-page benchmark text); RSVP reader operates at 1000 WPM without dropped frames; reading position restores accurately on reopening.

---

### Phase A4 — Format Parity: PDF, OCR, Web Share & TTS (2–3 weeks)

**Goal:** Complete format ingestion parity on Android (PDF text layer, on-device OCR, system share receiver, and text-to-speech).

- **PDF Ingestion (ADR-026):**
  - Integrate `libpdfium.so` into `jniLibs/`.
  - Pass library path to `gist-parse-pdf` on initialization.
  - Test PDF imports against test fixtures: extract text layer, preserve reading order, detect scanned PDFs without text layers and prompt OCR.
- **OCR Engine & Camera Scanner (ADR-025):**
  - Implement `AndroidMlKitOcrEngine` implementing UniFFI `OcrEngine`.
  - CameraX multi-page document scanner: batch take photos of paper document, display thumbnail filmstrip, deskew/normalize in `gist-imageprep`.
  - OCR Review Screen: display recognized text with confidence-based highlights for low-scoring words, allow inline corrections before committing to library.
- **Android Share Target:**
  - Register `ACTION_SEND` intent filter for `text/plain` and URL links in `AndroidManifest.xml`.
  - Browsers (Chrome, Firefox, Brave) sharing a web page to GIST immediately launch the URL import pipeline.
- **Android Text-to-Speech (TTS):**
  - Integrate `android.speech.tts.TextToSpeech` using local on-device TTS engine.
  - Read aloud synchronized with token stream highlighting.

**Exit Criteria:** Text-layer PDF and scanned camera documents import successfully; URL shared from Chrome imports cleanly; 100% offline OCR verified with zero network calls.

---

### Phase A5 — Accessibility, Material 3 Theming, Packaging & Independent Review (2–3 weeks)

**Goal:** Polish accessibility, implement system themes, prepare store packaging, and complete independent security review.

- **Accessibility Pass:**
  - TalkBack screen reader labels and semantics for all custom Compose components (RSVP controls, speed dial, TOC list, library items).
  - Dynamic font scaling (Android system font scale 100%–200%).
  - Minimum touch targets (48 × 48 dp) for all interactive icons and buttons.
- **Theming & Adaptive Icons:**
  - Material 3 Dynamic Color integration (adapting to user wallpaper palette on Android 12+).
  - Explicit theme picker: Light, Dark, Sepia, and OLED True Black (`#000000`).
  - Adaptive App Icon using dual XML vector source layers conforming to [`docs/iconspecification.md`](./iconspecification.md) §3.
- **Packaging & Signing:**
  - Configure Gradle release signing with production keystore.
  - Build Android App Bundle (`.aab`) for Google Play and standalone signed `.apk` for GitHub Releases and F-Droid.
  - Validate package size and ABI splits.
- **Independent Security & Quality Review:**
  - Perform independent review matching the F30 protocol.
  - Audit Keystore lifecycle, SAF sandbox boundaries, SSRF resolver, log outputs, and dependency licenses (`docs/THIRD-PARTY.md`).

**Exit Criteria:** TalkBack pass complete; zero High/Medium security findings; signed AAB/APK builds reproducibly in CI; `PLATFORM_VERIFICATION.md` updated with end-to-end device confirmation.

---

## 6. Test & CI Matrix

| Test Layer | Framework / Tool | Verification Scope |
|---|---|---|
| **Rust Workspace** | `cargo test --workspace` | All existing Rust unit and integration tests remain green on Linux host. |
| **NDK Cross-compilation** | `cargo-ndk build` | Compiles clean `.so` libraries for `arm64-v8a`, `armeabi-v7a`, `x86_64` without warnings. |
| **Headless Core Tests** | JUnit 5 + Robolectric in `:core` | Port all 49 Apple/Windows behavioral tests: import, search, collections, tags, removal never touching originals, encryption idempotency, sort orders, theme resolution. |
| **Key Custody** | Android Keystore Tests | Hardware key generation, race condition convergence across 20 parallel threads, corrupted file handling (`KeyStoreCorruptException`). |
| **UI Snapshot & State** | Compose Testing / Roborazzi | Compose UI tests for Library, RSVP speed dial gestures, Flow typography changes, and Theme switching. |
| **Physical Device QA** | Manual & automated instrumentation | Device verification on physical phone/tablet: SAF import, camera scan, TalkBack screen reader navigation, OLED true black visual inspection. |

---

## 7. Out of Scope for the Initial Android Release

- Cloud synchronization across devices (consistent with v1.0 non-goals).
- Google Drive or third-party cloud drive automatic backup.
- Social sharing, user accounts, or store purchases.
- Wear OS or Android Auto companion applications.
- Proprietary cloud OCR or remote transcription services (GIST remains 100% on-device).

---

## 8. Integration Rules & Roadmap Sequencing

1. **Non-blocking Platform Schedule:** The Android project runs concurrently with ongoing Apple M7 and Windows W6 milestones. Neither blocks the other.
2. **Shared Rust Invariance:** Android consumes `gist-core`, `gist-store`, and `gist-rsvp` without forking persistence schemas or timing arithmetic. Any core improvements benefit all platforms.
3. **Cross-Platform Handoff Discipline:** If Android development discovers an enhancement needed in shared Rust crates, changes must pass the full workspace test suite. Apple- or Windows-specific code must not be modified from non-matching host environments; log required items in `PENDING_APPLE_CHANGES.md` or `PENDING_WINDOWS_CHANGES.md`.
4. **Platform Verification Logging:** Update [`PLATFORM_VERIFICATION.md`](./PLATFORM_VERIFICATION.md) only with actual executed commands and measured test runs on verified hardware.
