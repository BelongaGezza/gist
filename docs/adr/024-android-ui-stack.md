# ADR 024 — Android UI stack: Kotlin and Jetpack Compose

**Date:** 2026-10-07
**Status:** Proposed

## Context

GIST uses a shared Rust core with platform-native UI shells (SwiftUI on macOS/iOS per ADR-001/M2, WinUI 3 on Windows per ADR-018, and GTK4 proposed for Linux). To deliver an Android client with full feature parity (RSVP reader, Flow reader, library management, collections/tags, and custom themes), a modern UI framework and language stack must be chosen.

Options considered:
1. **Kotlin + Jetpack Compose (Modern Android Development / MAD):** The official, declarative UI toolkit for Android. Direct 1:1 conceptual parity with SwiftUI and WinUI 3 declarative XAML.
2. **Kotlin + Traditional Android Views (XML layouts):** The legacy toolkit. Verbose, imperative view manipulation, poor alignment with SwiftUI/Compose reactive patterns.
3. **Cross-platform UI (Flutter/Dart or React Native):** Would introduce a third/fourth UI runtime, extra FFI layers over Rust, conflicts with the repo's established "native UI shells calling Rust core via UniFFI" architectural boundary, and breaks the project's mono-repo conventions.

## Decision

Adopt **Kotlin (2.x) with Jetpack Compose** as the UI stack for the Android shell (`apps/android`).

Key architectural choices:
- **UI Architecture:** Modern Android Development (MAD) guidelines using Android Architecture Components (`ViewModel`, `StateFlow`, `SharedFlow`, Kotlin Coroutines). Unidirectional Data Flow (UDF).
- **Design System:** Material Design 3 (`androidx.compose.material3`) conforming to the GIST design tokens in `docs/iconspecification.md`. Support for Material You Dynamic Color alongside custom reader themes: Light, Dark, Sepia, and OLED True Black.
- **Threading Model:** All FFI and disk operations are dispatched to `Dispatchers.IO`. The UI thread (`Dispatchers.Main`) only handles reactive state collection and rendering. No FFI call is ever executed synchronously on the main looper thread.
- **State Management:** A singleton or activity-scoped `CoreClient` wraps the generated UniFFI `GistCore` instance, mirroring Apple's `CoreClient` and Windows's `CoreClient`.

## Consequences

- **Developer Velocity & Parity:** High structural symmetry with SwiftUI (`@State` / `@StateObject` ↔ `remember` / `collectAsStateWithLifecycle`, `LazyVStack` ↔ `LazyColumn`, `NavigationStack` ↔ Jetpack Compose Navigation).
- **Theme Consistency:** Compose's dynamic theme provider cleanly implements GIST's 5 theme modes (System, Light, Dark, Sepia, OLED True Black) with OLED providing pure black (`Color(0xFF000000)`).
- **Performance:** Hardware-accelerated rendering through Android RenderThread; efficient virtualized scrolling for long documents via `LazyColumn`.
- **Requirements:** Requires Android SDK with `minSdk = 26` (Android 8.0 Oreo) and `targetSdk = 35` (or latest Android 15/16). Minimum Android 8.0 covers >95% of active Android devices while providing native java.time, secure storage, and hardware-accelerated graphics.
