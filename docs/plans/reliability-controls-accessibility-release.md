# Implementation Plan: Reliability, Daily Controls, Accessibility, and Release Gates

**Spec:** `docs/specs/reliability-controls-accessibility-release.md`
**Status:** Implemented; local Rust execution is toolchain-blocked

## Phase 1 — Typed settings and validation

1. Add Rust enum and validation tests before replacing free-form mode/action strings.
2. Preserve the existing lowercase JSON contract with serde.
3. Validate complete configurations transactionally before disk or live-state mutation.
4. Return field-addressable validation issues and render them beside the responsible controls.

## Phase 2 — Confirmed enforcement outcomes

1. Add pure confirmation tests for missing, reused, changed, still-matching, and unreadable targets.
2. Keep the consumed ticket's original rule and target identity in Rust.
3. Poll the original target for at most two seconds without holding shared state.
4. Award close XP only after a confirmed result.

## Phase 3 — Daily allowance and schedule

1. Add schedule-boundary, date-reset, monotonic-accounting, privacy, and recovery tests.
2. Store only local date and aggregate used seconds through recoverable persistence; flush on intentional exit.
3. Gate usage and enforcement in the monitor while preserving disabled-feature behavior.
4. Add opt-in settings and live status text.

## Phase 4 — Accessibility and keyboard operation

1. Add semantic navigation, panels, headings, groups, names, and live regions.
2. Add deterministic focus behavior for dynamic rules and validation failures.
3. Expose Snooze and Pat through ordinary buttons.
4. Honor reduced-motion preferences and add visible focus/error states.
5. Run automated axe checks for every settings panel and manually verify static color-token contrast.

## Phase 5 — CI and verified releases

1. Add Windows pull-request/default-branch gates for JavaScript, Rust format/tests/lint, and production build.
2. Pin every third-party action to an immutable commit.
3. Check package, Cargo, Tauri, and tag versions before release.
4. Create releases as drafts, verify installers/updater metadata/signatures, then publish.

## Verification Record

- `npm test` passes: JavaScript syntax, axe critical/serious checks on every rendered panel (including generated rule controls), and release-verifier fixtures.
- `node scripts/check-version.mjs` passes for repository versions; a deliberately mismatched tag is rejected.
- `cargo fmt --all -- --check` passes.
- `cargo metadata --locked --no-deps` passes.
- Both workflow YAML files parse successfully.
- Static contrast checks pass for normal text, muted text, focus, and error colors.
- Regression tests cover unmatched/schedule/date-boundary intervals so they cannot consume a later day's allowance.
- `cargo test --locked` cannot reach project compilation on this machine because the MSVC linker `link.exe` is not installed. The new Windows CI runs the required Rust test, Clippy, and production-build gates on a complete toolchain.
