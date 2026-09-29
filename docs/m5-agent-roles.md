# M5 Execution Plan — Agent Roles & Team Structure

**Status:** in progress, started 2026-09-29. Authoritative for *how M5 gets built* — scope/exit-criteria authority remains `docs/development-plan-v2.md` §5 M5 and `CLAUDE.md`'s milestone register; this document does not restate or override those, it refines them into assignable work. Modeled directly on `docs/m4-agent-roles.md`'s structure (now fully done — every role in its §2 landed and was merged/verified 2026-09-29).

**Trigger:** the phrase **"proceed with development"** (or `/proceed-with-development`) now invokes this plan — `CLAUDE.md`'s Working Conventions pointer should be repointed from `docs/m4-agent-roles.md` to this file once this plan lands (see §4).

**Platform scope:** macOS shell only, same as M3/M4. Requires `apple(macOS/iOS)=yes` per the `[GIST ENV]` banner for the Swift-touching role, if any is added later; as scoped below, R1 is Rust-only and R2 is docs-only, so neither strictly needs Apple tooling — still run from a macOS session for consistency with the rest of this repo's session history.

**Why M5's own plan text (`docs/development-plan-v2.md` §5 M5: "Beta feedback triaged; release notes; landing/README; GitHub issue templates; v1.0 tagged and published") isn't the whole story here:** two of those five items are not agent-executable at all in this environment, for the same reason M4's `F10`/`N8` weren't — see the hard environment constraint below. A third, real gap was found while scoping this plan and folded in as R1: **Q10** ("Schema/IR versioning and forward compatibility policy") was flagged in `docs/development-plan-v2.md` §7 as **"Required before first public beta," due "M4 start"** — it was never assigned to a role in `docs/m4-agent-roles.md` and is still genuinely unimplemented (confirmed 2026-09-29 by reading `gist_model::Document`: it has no version field of any kind, and `gist-store` deserializes IR blobs via a bare `serde_json::from_str` with no forward-compatibility check — unlike the SQLite schema, which already has `SCHEMA_VERSION`/`SchemaTooNew`). Since it gates the public beta that M5 itself is supposed to run, it has to close before M5's remaining, human-gated items make sense to pursue — it is R1 here, not deferred again.

**Hard environment constraint, confirmed 2026-09-29 (same constraint as M4's header, restated because it directly determines this plan's scope):** this dev environment has **no Apple code-signing identity** and **no notarization credentials**. `docs/m4-agent-roles.md`'s R7 already built the unsigned half of the release pipeline (`release-macos.yml`, `tools/build-dmg.sh`) — that work is not repeated here. What M5 adds on top of it (a real signed/notarized tag-triggered release, actually running a public beta, triaging real user feedback) needs a human with real credentials and real users, neither of which exist in this environment. That work is written up as an explicit go/no-go checklist (see R3) so a human can pick it up, not silently skipped.

---

## 0. Team Leader role

Same protocol as `docs/m4-agent-roles.md` §0, unchanged, restated briefly:

1. **Preflight.** Check `PENDING_APPLE_CHANGES.md`/`PENDING_WINDOWS_CHANGES.md` for actionable entries. Confirm `git status` is clean before spawning (stash/ask if not).
2. **Compute the ready batch** from §2's dependency table.
3. **Spawn the batch in parallel** — one `Agent` call per ready role, all in a single message, `isolation: "worktree"`, self-contained prompts quoting the relevant role spec (agents don't have this session's context).
4. **Integrate one at a time** onto a fresh `integration/m5-2026-09-29` branch off `integration/m4-2026-09-28` (M4's integration branch is the current tip of real, verified work — not `main`, which is behind it and has not been fast-forwarded/merged/PR'd yet).
5. **Advance to the next batch** once dependencies are satisfied on the integration branch.
6. **Full verification on the fully-integrated tree**: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources` (+ `cargo deny check advisories` if the local binary supports it), `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`) if any role touched Swift.
7. **Update the record**: `CLAUDE.md` (M5 narrative + milestone register + open-questions table for Q10 + security register if relevant), `docs/development-plan-v2.md` §5 M5/§7/§8, this file's §2 status column.
8. **Report to the user, stop before publishing.** Do not push the integration branch or open a PR without a separate explicit go-ahead.

---

## 1. What's already done (do not re-assign)

Confirmed against current source, 2026-09-29:

- All of M4's agent-executable backlog (`docs/m4-agent-roles.md` R1–R7) — crash/fuzz sweep, performance benchmarks + pagination, checksum-mismatch surfacing (Rust+Swift), storage/integrity UI, `docs/PRIVACY.md`, license attribution screen, and the unsigned half of the release pipeline (`release-macos.yml` + `tools/build-dmg.sh`). No M5 work should re-touch any of that surface.
- `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md` already exist — R2 below only needs to verify they're current/adequate for a public launch, not create them from scratch. No `.github/ISSUE_TEMPLATE/config.yml` chooser exists — R2 adds one if it's warranted.
- `CONTRIBUTING.md` and `docs/BUILDING-macos.md` already exist (confirmed by M4's header) — R2 verifies currency, does not recreate.
- No `CHANGELOG.md`/`RELEASE_NOTES.md` exists anywhere in the repo (confirmed via `ls`) — this is genuinely new, part of R2.
- `README.md` exists (70 lines) with an honest "pre-1.0, under active development" status line — R2 evaluates whether it needs updating and how, without prematurely claiming a public release that hasn't happened.
- `gist-store`'s SQLite schema already has `SCHEMA_VERSION`/`SchemaTooNew` version-ceiling protection (ADR/`CLAUDE.md` security policies) — this is the *database* schema, a different thing from the *IR blob* (`<id>.json`/`<id>.tokens.json`) format versioning Q10 is actually about. Do not conflate the two or treat Q10 as already solved because of this.

---

## 2. M5 backlog → roles

| # | Role | Scope | Primary files/crates | Depends on | Status |
|---|---|---|---|---|---|
| R1 | Rust — Q10: IR/schema versioning + forward-compatibility policy | `crates/gist-model/`, `crates/gist-store/`, new `docs/adr/019-ir-versioning.md` | — | ✅ Done 2026-09-29 |
| R2 | Docs — release readiness (CHANGELOG, README, issue templates, CONTRIBUTING/BUILDING currency) | `CHANGELOG.md`, `README.md`, `.github/ISSUE_TEMPLATE/`, `CONTRIBUTING.md`, `docs/BUILDING-macos.md` | — | ✅ Done 2026-09-29 (`docs/BUILDING-macos.md` was found empty since the initial scaffold, not merely stale — populated for real) |
| — | Public beta (~20–50 users) + triage | n/a | n/a | **Out of scope for agent execution — needs real users. Not assigned to an agent.** See R3's checklist. |
| — | Signed/notarized tag-triggered release, `v1.0` tag + publish | n/a | n/a | **Blocked — needs a human with a real Developer ID cert + notarization credentials, same constraint as M4's `F10`/`N8`.** See R3's checklist. |
| R3 | Docs — human go/no-go checklist for the two blocked items above | `docs/v1.0-release-checklist.md` (new) | — | ✅ Done 2026-09-29 |

R1 and R2 have no dependency on each other and can run in the same batch. R3 is a short, mechanical writeup — it can run in the same batch too, or be done directly by the Team Leader rather than spawned, at the Team Leader's discretion, since it's pure transcription of already-known blockers (M4's `F10`/`N8` register rows, this file's own header) into a human-facing checklist rather than new investigation.

### R1 — Rust: Q10 — IR/schema versioning + forward-compatibility policy

- Read `docs/development-plan-v2.md` §7's Q10 entry and `CLAUDE.md`'s open-questions table for the exact framing: "Schema/IR versioning and forward compatibility policy," due "M4 start," "required before first public beta."
- Confirm the current gap directly from source before designing anything: `gist_model::Document` (`crates/gist-model/src/lib.rs`) has no version field; `gist-store`'s `insert_item`/`get_item` (`crates/gist-store/src/lib.rs`) serialize/deserialize the IR blobs via plain `serde_json::to_string`/`from_str` with no version check of any kind. This is distinct from `SCHEMA_VERSION`/`SchemaTooNew`, which only covers the SQLite schema.
- Write `docs/adr/019-ir-versioning.md` deciding and recording:
  - An explicit `ir_version: u32` (or similar) field on the persisted IR envelope — decide whether it belongs on `Document` itself (simplest, but changes the public model type every consumer sees) or as a thin wrapper struct only at the storage boundary (`gist-store` wraps/unwraps it, `gist-model::Document` stays version-agnostic) — this file deliberately does not pre-decide which, that's the ADR's job, but lean toward whichever keeps `gist-model`'s wasm32 compile target (see `CLAUDE.md`'s crate conventions: "`gist-model` must compile to `wasm32-unknown-unknown`, no I/O deps") unaffected, since I/O-boundary concerns like this arguably belong in `gist-store`, not the pure model crate.
  - Forward-compatibility policy: what happens when a future, newer-versioned IR blob is read by an older binary. Mirror `SchemaTooNew`'s shape — a typed, distinct error (not a generic JSON parse failure), refusing to guess at semantics of fields it doesn't understand, never silently truncating/misinterpreting data.
  - Backward-compatibility policy for *additive* changes: confirm (and write a test proving) that adding a new optional field to `Document`/`Section`/`Block`/`Token` and reading an *old* blob that lacks it deserializes cleanly via `#[serde(default)]`, and that reading a *new* blob with an extra unknown field from an *old* binary doesn't error (serde's default behavior already tolerates unknown fields unless `deny_unknown_fields` is set — confirm no type in the IR graph sets that attribute, and document this as the deliberate policy in the ADR rather than an accident).
  - A concrete migration story for the *first* breaking IR change post-v1.0 (doesn't need to be built now, since none exists yet — just specify the mechanism: e.g. "a versioned enum of decoders, `IrEnvelope::V1(Document)` / `IrEnvelope::V2(...)`, matched during load" or whatever the ADR decides) so this isn't re-litigated from scratch whenever the first real breaking change happens.
- Implement the decided version field + forward-compat rejection, with tests: a current-version blob round-trips; a blob with a fabricated future version number is rejected with the new typed error, not a panic or a generic parse error; a blob with an extra unknown JSON field (simulating a hypothetical newer-but-still-decodable version) still deserializes successfully.
- `cargo test --workspace`/`clippy -D warnings`/`fmt --check` must stay green. If the storage format on disk changes shape at all (e.g. wrapping the JSON in an envelope), add a migration/compat test reading a *pre-existing, unversioned* blob (i.e. today's on-disk format, no wrapper) and confirm it's still readable — existing users' libraries must not break. This is the most important test in this role; don't skip it.
- Update `CLAUDE.md`'s Q10 row and `docs/development-plan-v2.md` §7 to record the decision as closed, dated 2026-09-29, in the same style as Q11's closure note.

### R2 — Docs: release readiness

- **`CHANGELOG.md`** (new, repo root): start it now rather than at tag time, per standard practice (e.g. Keep a Changelog format is a reasonable default, but not mandated — your call on exact format). Populate an "Unreleased" section summarizing what's shipped so far at a user-facing level (import formats, RSVP + flow reading views, search/collections/tags, theming, annotations, accessibility, encryption-at-rest, at-rest integrity) — derive this from `CLAUDE.md`'s milestone register and ADRs, not by re-deriving from raw git log, since that register already exists as the source of truth. Do not write a "v1.0" dated entry yet — there is no tag, and doing so would misrepresent an unreleased state as shipped.
- **`README.md`**: evaluate whether the "Status" section (pre-1.0, active development) still accurately reflects where the project actually is (M1–M4 done modulo credential-gated items, M5 in progress) — update the wording if it's grown stale, but do **not** claim "v1.0 released" or imply a public download is available; that would be false until a human completes the credential-gated release. Consider whether a brief "Features" bullet list (derived from the same milestone-register source as the changelog) would help a first-time reader, but keep changes proportionate — this is a light pass, not a rewrite.
- **`.github/ISSUE_TEMPLATE/`**: review the two existing templates (`bug_report.md`, `feature_request.md`) for adequacy ahead of a public beta with unfamiliar users (e.g. do they ask for platform/OS version, reproduction steps, expected vs actual behavior?) and improve them if genuinely lacking — don't rewrite them wholesale if they're already adequate. Add `.github/ISSUE_TEMPLATE/config.yml` (a template chooser) only if you judge it adds real value (e.g. a link to `docs/PRIVACY.md` or the repo's security-reporting channel for anyone who'd otherwise file a vulnerability as a public issue). **Correction, 2026-09-29 (found by R2, not pre-checked when this plan was drafted): a `SECURITY.md` already exists at the repo root and is already linked from `README.md`'s Security section — don't claim it's missing or invent a new contact address; a config.yml contact link should point at the existing one.**
- **`CONTRIBUTING.md`** / **`docs/BUILDING-macos.md`**: read both in full and cross-check their build instructions against the actual current toolchain (`rust-toolchain.toml`'s pinned version, `tools/build-core-xcframework.sh`, `xcodegen`) — fix anything stale (e.g. an old Rust version number, a removed script name), but this is a currency check, not a rewrite.
- Report what you changed and, importantly, what you *chose not to change* and why (e.g. "README's Status section already reads accurately, left as-is").

### R3 — Docs: human go/no-go checklist for credential-gated items

- Write `docs/v1.0-release-checklist.md`: a short, concrete, checkbox-style document a human (not an agent) works through when they're ready to actually ship v1.0. Pull the exact blocking facts from `CLAUDE.md`'s `F10` and `N8` security-register rows and this file's header — do not re-investigate, this is a transcription/organization task. Cover, at minimum:
  - Apple Developer Program enrollment + a real Developer ID Application certificate; configuring the five `release-macos.yml` secrets (`APPLE_DEVELOPER_ID_CERT_P12`/`APPLE_DEVELOPER_ID_CERT_PASSWORD`/`APPLE_TEAM_ID`/`APPLE_ID`/`APPLE_APP_SPECIFIC_PASSWORD`).
  - Running `N8`'s real signed-Release launch check (confirm no `Library not loaded`/library-validation errors) before ever pushing a real `v*` tag.
  - Recruiting the ~20–50 beta users the plan calls for, and where/how their feedback gets triaged (this doc should ask the question, not answer it — that's a product decision for the human, not something to invent).
  - Confirming M3's and M2's still-outstanding manual click-through/device-verification exit gates (VoiceOver navigation, hearing read-aloud, Dynamic Type/contrast on a real display, the standard UI click-through) are done, since those predate M5 and were never closed by any agent session — cross-reference `CLAUDE.md`'s M2/M3 milestone-register rows rather than re-describing them here.
  - Actually pushing a `v*` tag and confirming `release-macos.yml` runs green end-to-end for real (not the unsigned smoke-test `xcodebuild build` R7 already did).
- This is intentionally a short, mechanical document — do not pad it or turn it into a general release-engineering essay.

---

## 3. Exit criteria for this plan (not the same as M5's full exit criteria)

This plan covers R1–R3 only. It explicitly does **not** close:
- Running an actual public beta or triaging real feedback (needs real users — a human/product task, not code).
- Tagging and publishing a real, signed, notarized v1.0 (needs a human with real credentials — see R3's checklist).
- M2's and M3's still-open manual click-through / device-verification exit gates (pre-existing, not created by this plan, but a real precondition for a responsible public release — flagged in R3, not re-litigated here).

Once R1–R3 land and are verified, report to the user what's genuinely done, what's still blocked on a human, and hand them `docs/v1.0-release-checklist.md` as the next concrete step.
