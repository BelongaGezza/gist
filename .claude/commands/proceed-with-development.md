---
description: Act as Team Leader and assign agents to the roles defined in docs/m3-agent-roles.md to execute the next milestone's remaining work.
---

You have just been invoked as **Team Leader** for GIST's next development milestone. This command is deliberately thin — the actual plan, role definitions, dependency graph, and Team Leader responsibilities live in `docs/m3-agent-roles.md`. Read that file now in full before doing anything else; do not rely on memory of a prior conversation's summary of it, since it may have been refined since.

Follow `docs/m3-agent-roles.md` §0 (Team Leader role) exactly:

1. Preflight — check `PENDING_APPLE_CHANGES.md`/`PENDING_WINDOWS_CHANGES.md` for actionable entries, confirm `git status` is clean.
2. Compute the next ready batch of roles from §2's dependency table (recompute from the table, don't trust the doc's suggested batching if the table has since changed).
3. Spawn every ready role in that batch as parallel `Agent` tool calls in a single message, `isolation: "worktree"`, each briefed from that role's §3 entry — subagents have no context of their own, quote what they need into the prompt.
4. As each completes, integrate its branch onto `integration/m3-<today's date>` one at a time, resolving conflicts by hand.
5. Repeat 2–4 until every role in §2 is either done or blocked on something outside this plan's scope.
6. Run the full verification suite on the fully-integrated tree for real (`cargo test --workspace`, `clippy -D warnings`, `fmt --check`, `cargo deny check bans licenses sources`, `xcodegen generate` + `xcodebuild build`/`test` scheme `GISTmacOS`) — this step is not optional and not satisfied by a role's own isolated test run.
7. Update `CLAUDE.md`, `docs/development-plan-v2.md`, relevant ADRs, `PLATFORM_VERIFICATION.md`, and `docs/m3-agent-roles.md`'s own status notes — dated, in-place notes, matching this repo's established documentation convention. Do not silently rewrite prior claims; correct them explicitly with a dated note the way every other pass in these documents does.
8. Report to the user what shipped, how it was verified, and what's still outstanding. **Stop there.** Do not push the integration branch or open a pull request — that is a separate, explicit decision the user makes after seeing your report, not something this trigger authorizes on its own.

If the current session cannot build the Apple side (`apple(macOS/iOS)=NO` in the `[GIST ENV]` banner), execute only the Rust-only roles (R1, R2a, R2b) and clearly flag the Swift roles as pending for a macOS-capable session, per the platform-scope note at the top of `docs/m3-agent-roles.md`.

If `docs/m3-agent-roles.md` reports that every role in §2 is already done, say so plainly and ask the user whether to refine the plan for the next milestone rather than inventing new work.
