# ADR 018 — Windows UI stack: WinUI 3, Windows App SDK, C# on .NET 10, CommunityToolkit.Mvvm

**Date:** 2026-09-20
**Status:** Proposed

## Context
The Rust core owns parsing, persistence and pacing; the platform shell must present the same product as the SwiftUI app (library, collections/tags, RSVP, flow view with virtualised text, theming incl. OLED/Sepia, keyboard-driven use) per `docs/product-spec-reader-app-v3.md` and `docs/windows-ui-spec.md`. The binding to the core is uniffi-generated C# (ADR-015), so the shell language should be a .NET language. `docs/windows-development-plan.md` originally named `net8.0`; .NET 8 is not the newest LTS.

Verified 2026-09-20 on the dev machine:
- .NET SDK 10.0.401 installed. `Microsoft.WindowsAppSDK` stable versions on nuget.org end at 2.5.1 (previous stable: 2.4.0, 2.3.1, 2.2.0); this is the 2.x line, not the 1.x line the plan assumed. A hand-written project targeting `net10.0-windows10.0.19041.0` with 2.5.1 builds cleanly with `dotnet build` and launches (window title present, process alive >5 s), a NavigationView shell included, with no Visual Studio workload installed (see ADR-017 for the full record).
- The uniffi C# bindings driven from a .NET 10 console app passed 20 checks (ADR-015).
- Not verified: `CommunityToolkit.Mvvm` under this exact SDK (not yet referenced in the spike); WinUI 3 `ListView`/`ItemsRepeater` virtualisation on long documents; accessibility (Narrator/UIA) behaviour; text selection and find in a virtualised flow view; ARM64; trimming/AOT.

## Decision
- **UI framework:** WinUI 3 via the Windows App SDK (2.x, pinned exactly, currently 2.5.1 as verified).
- **Language/runtime:** C# on **.NET 10** (I believe it is an LTS release; unverified here, confirm the support end date before W1 closes), `net10.0-windows10.0.19041.0`, min platform 10.0.19041.
- **MVVM:** `CommunityToolkit.Mvvm` (source-generated observable properties/commands, no runtime reflection-heavy container). View models live in a UI-free `GIST.Core`-side project so they are unit-testable with plain `dotnet test`.
- **Shell:** `NavigationView`, standard Fluent controls only in W1 (plan risk WR4: team is new to WinUI). Theming through resource dictionaries mapped from the existing `ThemeSelection` semantics.

### Alternatives considered
| Option | Why not (given: match SwiftUI app, spec, uniffi C# bindings) |
|---|---|
| **WPF** | Mature and well tooled, and would work with the same C# bindings. Rejected as primary because it is the older stack with no Fluent/Mica/modern controls out of the box and weaker high-DPI/per-monitor and touch behaviour; the spec targets a native Windows 11 look. Remains the credible fallback if WinUI blocks (the view models and `CoreClient` would carry over). |
| **WinForms** | GDI-era rendering, poor fit for custom typography, theming (OLED/Sepia) and virtualised rich text. |
| **.NET MAUI** | Its Windows target is WinUI 3 underneath, so it adds an abstraction layer without giving us macOS/iOS, which we do not need (Apple has its own SwiftUI shell). Extra layer, weaker fidelity for custom text views. |
| **Avalonia** | Real cross-platform and would run on Windows/Linux/macOS from one codebase, and the C# bindings would work. Rejected because the strategy is deliberately native shells over a shared Rust core, and it would draw its own controls rather than use platform ones (accessibility/IME fidelity is a per-control question we would have to verify). Reconsider if a Linux shell is ever requested. |
| **Flutter** | Would add Dart plus a second FFI path (dart:ffi or a C ABI shim), abandoning the uniffi single source of truth from ADR-015. Contradicts this repo's architecture (Rust core + native shells). |
| **WebView2 hybrid (web UI)** | A web UI plus a bridge to the core; reading text rendering would be good, but it duplicates the SwiftUI design in a third technology, complicates the sandbox/CSP story, and weakens native accessibility/keyboard integration. Reasonable only for a single rich-text surface later (flow view), not the app. |

### Roadmap risk (honest)
I have seen public discussion that WinUI 3 / Windows App SDK receive less investment than in the past, that some control gaps are long-standing (for example rich virtualised text selection, and a perceived slow issue turnaround), and that Microsoft's own apps are mixed between WinUI, WPF and web tech. **I did not verify any of this in this session**, and I have no source to cite. What is verified: the Windows App SDK still shipped stable 2.x releases (2.0.1 through 2.5.1 on nuget.org) and the 2.5.1 runtime is present on this machine. Mitigations regardless: keep view models and `CoreClient` free of WinUI types so a WPF or other XAML shell remains a bounded rewrite; avoid experimental Windows App SDK packages; pin versions; treat the flow view text surface as the W-phase risk item and spike it early.

## Consequences
- Plan update needed (team lead): `net8.0` becomes `net10.0-windows10.0.19041.0`, Windows App SDK 2.x rather than 1.x, and the "install the WinUI VS workload" prerequisite becomes optional.
- Windows App Runtime must be present for unpackaged framework-dependent runs (it was here); the MSIX carries a framework dependency, and a self-contained option exists but is unverified.
- Generated uniffi types stay `internal` in `GIST.Core`; the UI sees only `CoreClient` and view models (ADR-015).
- Unverified items above (Toolkit reference, virtualised text, accessibility, ARM64, AOT) become explicit W1/W3 exit checks.
