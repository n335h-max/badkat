# Spec: Safe Enforcement and Crash-Safe Persistence

**Author:** Codex
**Date:** 2026-09-20
**Status:** Approved
**Reviewer:** User
**Related code:** `badkat/src-tauri/src/lib.rs`, `win.rs`, `config.rs`, `progress.rs`

## Context

BadKat currently sends the full detected window snapshot to the renderer and accepts that snapshot back through the `act` command. Rust rechecks the foreground window handle and part of its title before sending `Ctrl+W` or `WM_CLOSE`, but the renderer still supplies the target and an unmatched target falls back to the `tab` action. This spreads authority across the monitor, renderer, and command handler instead of keeping the destructive decision in Rust.

Configuration and earned progress are serialized directly to their final files. A process interruption, disk error, or partial write can leave either file unreadable. Configuration then falls back to defaults and progress silently resets, even when the previous valid data could have been retained as a recovery copy.

This milestone keeps the current product behavior and JSON formats while making destructive actions server-owned and saved state recoverable.

## Functional Requirements

- FR-1: Rust MUST issue an opaque, single-use action ticket when a matched rule reaches its grace limit.
- FR-2: Rust MUST retain the ticket's target snapshot, matched rule ID, resolved action, issue time, and expiry internally; the renderer MUST receive only the opaque ticket ID plus non-authoritative display data.
- FR-3: The `act` command MUST accept only an action ticket ID and MUST NOT accept a renderer-supplied window snapshot or action.
- FR-4: Before acting, Rust MUST capture a fresh foreground snapshot and require the same window handle, process ID, process name, and matched rule ID as the issued ticket.
- FR-5: If the original rule depended on a URL, revalidation MUST include a fresh URL. Failure to read the URL MUST reject the action rather than fall back to title-only matching.
- FR-6: A ticket MUST expire 15 seconds after its configured countdown ends and MUST become unusable after its first consumption attempt, whether that attempt succeeds or fails.
- FR-7: Missing, expired, consumed, mismatched, disabled, or no-longer-matching tickets MUST produce `acted: false`, MUST NOT send input or a close message, and MUST NOT award XP.
- FR-8: The operating-system action result MUST be checked. XP MUST be awarded only when the OS reports that the requested input or close message was successfully dispatched.
- FR-9: Configuration and progress saves MUST write a complete temporary file before replacing the primary file.
- FR-10: A successful replacement MUST retain the previous valid primary file as a backup.
- FR-11: Loading MUST prefer a valid primary file, recover from a valid backup when the primary is missing or invalid, and use defaults only when neither copy is valid.
- FR-12: Backup recovery MUST be reported in configuration notes and in a progress recovery note visible through the existing status surface.
- FR-13: Existing `config.json` and `progress.json` data MUST remain readable without migration.

## Non-Functional Requirements

- **NFR-S1:** Renderer code MUST have no interface that can select an arbitrary native window or action for enforcement.
- **NFR-S2:** Every uncertain revalidation outcome MUST fail closed without acting.
- **NFR-R1:** After any save that returns success, at least one complete, parseable copy of the newly saved value MUST exist at the primary path.
- **NFR-R2:** Replacement failure MUST preserve or restore the last valid primary or backup copy.
- **NFR-R3:** Ticket storage MUST be bounded to at most 32 entries and MUST prune expired entries whenever tickets are issued or consumed.
- **NFR-C1:** The persisted JSON field names and meanings MUST remain backward compatible with version 0.1.1.
- **NFR-T1:** Tests MUST use the action-authorization interface and the existing persistence interfaces, not private implementation details.

## Acceptance Criteria

### AC-1: Valid ticket authorizes the original action (FR-1, FR-2, FR-3, FR-4, FR-5)
Given Rust issued a live ticket for a rule-matched foreground browser tab
When `act` consumes the ticket while a fresh snapshot identifies the same window, process, URL, and rule
Then the resolved action from the ticket is authorized
And no target or action supplied by the renderer influences the decision

### AC-2: Window change rejects the ticket (FR-4, FR-7, FR-8, NFR-S2)
Given Rust issued a ticket for window A
When window B is foreground at consumption time
Then the result is `acted: false`
And no operating-system action is attempted
And no XP is awarded

### AC-3: Tab change rejects a URL-dependent ticket (FR-4, FR-5, FR-7)
Given Rust issued a ticket for a URL-dependent rule in a browser window
When the same browser window and process remain foreground but the fresh URL no longer matches that rule
Then the result is `acted: false`
And no operating-system action is attempted

### AC-4: Unreadable URL fails closed (FR-5, FR-7, NFR-S2)
Given Rust issued a ticket whose rule matched through its URL
When the URL cannot be read during fresh revalidation
Then the ticket is rejected
And title text alone does not authorize the action

### AC-5: Ticket is single-use (FR-6, FR-7)
Given a live ticket has already been consumed once
When `act` receives the same ticket ID again
Then the result is `acted: false`
And no operating-system action is attempted

### AC-6: Ticket expiry and bounded storage (FR-6, NFR-R3)
Given issued tickets whose expiry times have passed and more tickets are issued
When the ticket store is inspected through its public interface
Then expired tickets cannot be consumed
And the store contains no more than 32 entries

### AC-7: Successful save preserves current and previous values (FR-9, FR-10, NFR-R1)
Given a valid primary file containing value A
When value B is saved successfully
Then the primary file contains complete value B
And the backup contains complete value A

### AC-8: Failed replacement preserves recoverable data (FR-9, NFR-R2)
Given a valid primary file
When replacement fails after the temporary file is written
Then save returns an error
And a subsequent load returns the prior valid value

### AC-9: Invalid primary recovers from backup (FR-11, FR-12)
Given an invalid primary file and a valid backup file
When the application loads saved state
Then it returns the backup value
And reports that backup recovery occurred

### AC-10: Existing files remain compatible (FR-13, NFR-C1)
Given configuration and progress JSON written by version 0.1.1
When the new version loads those files
Then all existing fields retain their prior values
And no migration is required

## Edge Cases and Error Scenarios

- EC-1: Unknown or empty ticket ID → reject without acting.
- EC-2: Rule disabled or edited during the countdown → reject without acting.
- EC-3: Same HWND reused for a different PID → reject without acting.
- EC-4: Ticket action is neither `tab` nor `close` due to corrupted internal state → reject without acting.
- EC-5: Temporary-file creation, write, flush, backup, or replacement fails → return an error and preserve recoverable prior data.
- EC-6: Primary and backup are both invalid → load defaults and report data recovery failure without deleting either file.
- EC-7: No primary or backup exists on first launch → create/use defaults without reporting corruption.
- EC-8: Preview interception in nag mode → display normally without issuing an actionable ticket.

## API Contracts

```typescript
interface BustDisplayPayload {
  actionId?: string;
  ruleId: string;
  label: string;
  mode: "close" | "nag";
  countdown: number;
  windowCenterX?: number;
}

interface ActRequest {
  actionId: string;
}

interface ActResult {
  acted: boolean;
  reason: string;
}
```

Test seams to approve:

```text
ActionTickets.issue(matched target, rule, action, countdown, now) -> action ID
ActionTickets.authorize(action ID, fresh target, current config, now) -> authorized action | rejection

config::save / config::load against a real temporary directory
progress::save / progress::load against a real temporary directory
```

Time is supplied to the in-process action-ticket interface in tests. Persistence tests use the real filesystem in isolated temporary directories; they do not mock internal functions.

## Data Models

### Pending Action Ticket

| Field | Type | Constraints |
|---|---|---|
| id | opaque random string | Unique, non-empty, never chosen by renderer |
| target | Snapshot | Internal only |
| ruleId | string | Must identify the rule matched at issue time |
| action | `tab` or `close` | Resolved at issue time |
| urlRequired | boolean | True when URL contributed to the original match |
| issuedAt | monotonic time | Internal only |
| expiresAt | monotonic time | Countdown end plus 15 seconds |

Tickets are memory-only and are not persisted across application restarts.

### Recoverable JSON File

| Path | Meaning |
|---|---|
| `<name>.json` | Current committed value |
| `<name>.json.tmp` | In-progress replacement; never loaded as committed data |
| `<name>.json.bak` | Previous valid committed value used for recovery |

## Out of Scope

- OS-1: Daily budgets, schedules, profiles, skins, and unlocks — separate product milestones.
- OS-2: Browser extensions or native messaging — potentially stronger tab identity, but materially larger installation and maintenance scope.
- OS-3: Confirming that a tab or application actually terminated after successful OS dispatch — this milestone verifies authorization and dispatch only.
- OS-4: Changing rule syntax, default rules, XP values, or countdown UX.
- OS-5: Broad settings validation and typed configuration enums — valuable follow-up, but independent of action ownership and persistence recovery.
- OS-6: Cross-platform persistence guarantees beyond currently supported Windows behavior.
