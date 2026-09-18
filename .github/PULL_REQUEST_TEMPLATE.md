## Summary

<!-- What does this PR change, and why? -->

## Checklist

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo deny check bans licenses sources` passes
- [ ] If this touches `apps/apple/**`, `crates/gist-ffi/**`, or the bindings/xcframework tooling: `xcodegen generate` + `xcodebuild build test` (scheme `GISTmacOS`) passes
- [ ] If this makes or revisits an architectural decision, it's recorded in `docs/adr/`
- [ ] If this is a security fix, the relevant finding in `CLAUDE.md`'s security register / `docs/security-review-v2.md` is updated

## Test plan

<!-- How did you verify this works? -->
