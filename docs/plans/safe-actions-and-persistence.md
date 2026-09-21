# Implementation Plan: Safe Enforcement and Crash-Safe Persistence

**Spec:** `docs/specs/safe-actions-and-persistence.md`
**Status:** Implemented (Rust test execution requires MSVC Build Tools)

## Phase 1 — Action authorization module

1. Add behavior-first Rust tests for valid, mismatched, expired, reused, and bounded tickets.
2. Add a small in-memory `ActionTickets` module that owns issuance and authorization.
3. Keep time injectable at the module interface and ticket IDs opaque.

## Phase 2 — Runtime integration

1. Store `ActionTickets` inside shared Rust state.
2. Issue a ticket only for real close-mode interceptions.
3. Remove native target data from the renderer payload.
4. Change `act` to accept an action ID, capture a fresh foreground snapshot, authorize it, then dispatch the stored action.
5. Update the pet renderer to return only the action ID.

## Phase 3 — Recoverable persistence

1. Add real-filesystem tests for successful replacement, backup recovery, first launch, and invalid copies.
2. Add one shared recoverable JSON writer/reader helper.
3. Route configuration and progress persistence through the helper without changing their JSON schemas.
4. Surface recovery notes through the existing status response.

## Phase 4 — Verification

1. Run the full Rust test suite and fix regressions.
2. Run formatting and static checks available in the environment.
3. Review the diff against every acceptance criterion and edge case.
4. Record any verification limitation caused by missing local tooling.

## Verification Record

- JavaScript syntax checks passed for `src/pet.js` and `src/settings.js`.
- New Rust modules pass `rustfmt --check`; all edited Rust files parse successfully through rustfmt.
- `cargo metadata --locked --no-deps` passed.
- `git diff --check` passed (Git reports only the repository's CRLF conversion warnings).
- `cargo test --locked` reached dependency compilation but could not run because `link.exe` and the Visual C++ Build Tools are not installed on this machine.
