# Spec: Reliability, Daily Controls, Accessibility, and Release Gates

**Author:** Codex
**Date:** 2026-09-20
**Status:** Approved
**Reviewer:** User
**Repository:** `X-DIABLO-X/badkat`

## Context

BadKat currently represents enforcement modes and rule actions as free-form strings. The settings UI constrains normal input, but Rust accepts a complete renderer-supplied configuration without validating enum values, numeric ranges, duplicate rule IDs, or malformed patterns. The destructive action path now authorizes a server-owned ticket, but it awards XP after Windows accepts an input or close message rather than after the offending target is observed to have gone away.

The product also has no daily allowance or patrol schedule, so users can only keep patrol continuously enabled or manually snooze it. Settings are visually structured but dynamic rule controls lack programmatic labels and focus management, status changes are not announced, reduced-motion preferences are ignored, and cat-only interactions have no keyboard-accessible equivalent. The release workflow publishes signed updater artifacts on tags, but pull requests do not run a dedicated quality gate and a release is not verified before becoming public.

This epic strengthens those five areas while preserving existing configuration files and default behavior. Daily controls remain opt-in, no global keyboard hooks are registered, no browsing history is stored, and release publication remains GitHub Actions based.

## Functional Requirements

### Typed and Validated Settings

- FR-1: Rust MUST represent enforcement mode as `EnforcementMode::{Close, Nag}` and rule action as `RuleAction::{Tab, Close}` while serializing them as the existing lowercase JSON strings.
- FR-2: Rust MUST validate the complete configuration before saving it or replacing live state.
- FR-3: Validation MUST reject an invalid mode/action, duplicate or blank rule ID, blank label, empty effective pattern set, blank pattern entry, non-finite cat value, or numeric value outside the bounds in the Data Models section.
- FR-4: A rejected save MUST return field-addressable validation errors and MUST leave both persisted and live configuration unchanged.
- FR-5: Loading a valid version 0.1.1 configuration MUST preserve its behavior without migration or user action.
- FR-6: An invalid loaded configuration MUST use the existing backup/default recovery path and report the reason through existing configuration notes.
- FR-7: Frontend code MUST use the typed serialized values and MUST display server validation errors without inventing a second authoritative validation policy.

### Confirmed Enforcement Outcome

- FR-8: A successfully dispatched close action MUST enter a bounded confirmation phase before XP or close statistics are awarded.
- FR-9: A tab action MUST be confirmed only when the original HWND/process is gone or a fresh snapshot of that window no longer matches the ticket's original rule.
- FR-10: A window-close action MUST be confirmed only when the original HWND is invalid or belongs to a different PID.
- FR-11: Confirmation MUST poll for at most 2 seconds at intervals no shorter than 100 ms.
- FR-12: Timeout, unreadable state, or continued rule match MUST return `acted: false` with a non-empty reason and MUST NOT award XP or increment close statistics.
- FR-13: Confirmation MUST remain on the Rust side and MUST NOT trust renderer-reported closure.

### Daily Allowance and Schedule

- FR-14: Configuration MUST support an opt-in daily allowance measured in foreground minutes of matched content.
- FR-15: Configuration MUST support an opt-in weekly patrol schedule consisting of selected local weekdays, a local start time, and a local end time; an end time earlier than or equal to the start time denotes an overnight window.
- FR-16: Both features MUST default to disabled so an existing installation behaves exactly as before.
- FR-17: When the schedule is enabled and the current local time is outside its active window, BadKat MUST neither accumulate allowance usage nor start/continue an enforcement grace period.
- FR-18: While the schedule permits patrol and a rule remains matched in the foreground, BadKat MUST accumulate elapsed foreground time against the current local date using monotonic elapsed durations.
- FR-19: Before the daily allowance is exhausted, matched content MUST be observed and counted but MUST NOT trigger a bust.
- FR-20: When the allowance becomes exhausted, the matched rule's normal grace period MUST start from zero; after that, existing countdown and enforcement behavior applies.
- FR-21: With daily allowance disabled, existing rule grace behavior MUST remain unchanged.
- FR-22: Usage MUST be stored locally in a recoverable `usage.json` file containing only local date and aggregate used seconds, retaining no URL, title, process, or rule history.
- FR-23: Usage MUST reset on local-date change and MUST tolerate DST/timezone changes without producing negative elapsed time or carrying a prior date's usage into the new local date.
- FR-24: Settings and status surfaces MUST show whether patrol is outside schedule, allowance remaining, or allowance exhausted.

### Accessibility and Keyboard Controls

- FR-25: Settings navigation MUST expose appropriate navigation, current-page, heading, group, label, description, and live-region semantics to accessibility APIs.
- FR-26: Every generated rule input and delete control MUST have a unique accessible name that includes the rule label or row position.
- FR-27: Adding a rule MUST move keyboard focus to its name field; deleting a rule MUST move focus to the nearest surviving rule or the Add button.
- FR-28: Save success, validation failure, update status, allowance status, and recovery notes MUST be announced through polite or assertive live regions as appropriate.
- FR-29: All settings actions MUST be operable by keyboard with visible focus and without requiring pointer drag operations.
- FR-30: Settings MUST provide keyboard-accessible buttons for “Snooze now” and “Pat the cat,” equivalent to the pet click actions.
- FR-31: When `prefers-reduced-motion: reduce` is active, nonessential settings and pet animations MUST be removed or reduced to a transition no longer than 100 ms; enforcement information MUST remain visible.
- FR-32: Text and interactive-control contrast MUST meet WCAG 2.2 AA: 4.5:1 for normal text and 3:1 for large text, focus indicators, and component boundaries.
- FR-33: No system-wide/global shortcut MUST be registered in this epic.

### CI and Release Verification

- FR-34: Pull requests and pushes to the default branch MUST run a Windows CI workflow using the repository lockfiles.
- FR-35: CI MUST run JavaScript syntax checks, `cargo fmt --check`, `cargo test --locked`, `cargo clippy --locked -- -D warnings`, and a Tauri production build.
- FR-36: CI MUST cache Rust artifacts without allowing a cache hit to skip any quality gate.
- FR-37: Tag releases MUST verify that the `vX.Y.Z` tag, `package.json`, `Cargo.toml`, and `tauri.conf.json` versions match before building.
- FR-38: The release workflow MUST run the same test, format, and lint gates before publishing artifacts.
- FR-39: Releases MUST initially be created as drafts, then verify that `latest.json`, updater signatures, NSIS installer, and MSI installer exist and reference the release version before becoming public.
- FR-40: A verification failure MUST leave the release as a draft and MUST fail the workflow.
- FR-41: CI and release jobs MUST use least-privilege GitHub token permissions; pull-request CI MUST have read-only repository access.

## Non-Functional Requirements

- **NFR-S1:** Configuration validation and enforcement confirmation MUST be authoritative in Rust; renderer checks are usability aids only.
- **NFR-S2:** Daily usage data MUST never include URLs, titles, process names, rule IDs, or event timestamps.
- **NFR-R1:** Existing `config.json` and `progress.json` formats MUST remain readable; new configuration fields MUST have serde defaults.
- **NFR-R2:** Usage persistence MUST use the existing atomic temporary/backup storage helper.
- **NFR-R3:** An app restart, clock rollback, DST change, corrupt usage file, or confirmation timeout MUST fail without awarding unearned XP or triggering an unintended close.
- **NFR-P1:** Confirmation polling MUST stop within 2.25 seconds of dispatch and MUST not hold the shared state mutex while sleeping or querying Windows.
- **NFR-P2:** Allowance accounting MUST add no more than one filesystem write per monitor polling interval and SHOULD coalesce writes to at most one every 30 seconds plus lifecycle/date transitions.
- **NFR-A1:** The settings window MUST pass automated axe-core checks with zero critical or serious violations for every panel.
- **NFR-A2:** Every settings function MUST be completable using only Tab, Shift+Tab, arrow keys where native controls require them, Enter, Space, and Escape.
- **NFR-C1:** Workflows MUST pin third-party GitHub Actions to immutable full commit SHAs before merge.
- **NFR-C2:** CI MUST complete with no network credentials other than GitHub's read-only token; signing secrets MUST only be exposed to the release build step.

## Acceptance Criteria

### AC-1: Typed configuration compatibility (FR-1, FR-5, FR-6, FR-7)

Given an existing valid 0.1.1 configuration, when it is loaded, edited with valid values, saved, and reloaded, then typed modes/actions retain the same lowercase JSON and behavior; invalid loaded values use recovery and expose a note.

### AC-2: Transactional validation (FR-2, FR-3, FR-4)

Given a configuration containing each invalid category, when save is requested, then field-addressable errors are returned and live/disk configuration remains byte-for-byte at the last committed value.

### AC-3: Confirmed tab outcome (FR-8, FR-9, FR-11, FR-13)

Given a dispatched tab close whose original window no longer matches the original rule, when confirmation observes that change within 2 seconds, then the action succeeds and exactly one close award is recorded without renderer input.

### AC-4: Unconfirmed close earns nothing (FR-8, FR-10, FR-11, FR-12, FR-13)

Given a dispatched close whose target remains present and matched until timeout, when confirmation ends, then `acted` is false with a reason and XP/statistics are unchanged.

### AC-5: Allowance transition (FR-14, FR-18, FR-19, FR-20)

Given a 10-minute allowance with 9 minutes 59 seconds used, when matched content remains foreground for one more second during schedule hours, then allowance becomes exhausted and rule grace starts at zero.

### AC-6: Schedule and civil-time boundaries (FR-15, FR-17, FR-23)

Given weekday, overnight, date-boundary, DST, and timezone-change cases, when schedule state is evaluated, then only configured local windows are active and elapsed usage is never negative or assigned to the wrong local date.

### AC-7: Opt-in compatibility (FR-16, FR-21)

Given an upgraded user with no new fields, when BadKat starts, then monitoring and enforcement match version 0.1.1 behavior.

### AC-8: Private recoverable usage status (FR-22, FR-23, FR-24)

Given recorded usage, when `usage.json` and the status response are inspected, then storage contains only schema version, local date, and aggregate seconds, recovery works, and the correct remaining state is reported.

### AC-9: Accessible settings operation (FR-25, FR-26, FR-27, FR-28, FR-29, FR-30, FR-32, FR-33)

Given keyboard-only navigation and a screen-reader accessibility tree, when every panel and dynamic rule operation is exercised, then all controls are named, focused in a logical order, operable, statuses announced, contrast passes, and no global shortcut exists.

### AC-10: Reduced motion and automated accessibility (FR-31)

Given reduced motion and each settings panel, when animations/status transitions occur and axe checks run, then nonessential motion is at most 100 ms and there are zero critical/serious violations.

### AC-11: Pull-request quality gate (FR-34, FR-35, FR-36, FR-41)

Given a pull request, when CI runs, then all five quality gates execute on Windows under read-only permissions and any failure blocks success.

### AC-12: Draft-first verified release (FR-37, FR-38, FR-39, FR-40, FR-41)

Given a matching version tag and signing secrets, when release runs, then the quality gates run and the draft is published only after version/artifact/signature verification; any mismatch leaves a failed draft.

## Edge Cases and Error Scenarios

- EC-1: Unknown enum string in a loaded config → recover via valid backup or defaults with a note; never reinterpret it as a destructive mode/action.
- EC-2: Renderer submits `NaN`, infinity, fractional integer fields, duplicate IDs, or out-of-range values → reject the full save transaction.
- EC-3: Config changes during close confirmation → confirmation uses the consumed ticket's original rule identity but still awards only if the target is gone/no longer matches.
- EC-4: HWND is reused by another PID during confirmation → treat the original window as closed.
- EC-5: URL becomes unreadable for a URL-dependent tab during confirmation → do not claim confirmed closure until the HWND disappears or timeout occurs.
- EC-6: App exits during confirmation → no later award is possible because awards are not persisted before confirmation.
- EC-7: Daily allowance is zero → normal rule grace begins immediately when a scheduled match starts.
- EC-8: No weekdays selected for an enabled schedule → validation error; do not silently create an always-off schedule.
- EC-9: Start equals end → interpret as a 24-hour active window on selected start weekdays.
- EC-10: Overnight window crosses local midnight → post-midnight time belongs to the prior selected weekday's window, while usage belongs to the new local date.
- EC-11: Corrupt primary usage file with valid backup → recover backup and surface a note; both invalid → start zero usage for today and report recovery failure.
- EC-12: Settings validation fails during autosave → keep edits visible, associate errors with controls, announce failure, and retry only after another edit.
- EC-13: Deleting the only rule row → focus Add Rule and allow server validation to prevent saving an empty ruleset.
- EC-14: Release tag mismatch or missing artifact/signature → fail before public publication and retain draft evidence.
- EC-15: Forked pull request → CI runs without signing/update secrets and never executes release publication.

## API Contracts

```typescript
type EnforcementMode = "close" | "nag";
type RuleAction = "tab" | "close";

interface ValidationIssue {
  path: string;       // e.g. "rules[2].id"
  code: string;       // stable machine-readable code
  message: string;    // user-facing English message
}

interface SaveConfigResult {
  saved: boolean;
  issues: ValidationIssue[];
}

interface DailyAllowanceConfig {
  enabled: boolean;
  minutes: number;
}

interface PatrolScheduleConfig {
  enabled: boolean;
  weekdays: Array<"mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun">;
  start: string; // strict HH:MM, local time
  end: string;   // strict HH:MM, local time
}

interface UsageStatus {
  state: "disabled" | "outside-schedule" | "available" | "exhausted";
  usedSeconds: number;
  allowanceSeconds?: number;
  remainingSeconds?: number;
  localDate: string;
}

interface ActResult {
  acted: boolean; // true only after confirmation
  reason: string;
}
```

Existing Tauri command names remain stable. `save_config` changes from a void success result to `SaveConfigResult`; rejected saves are domain results rather than transport errors. `status` adds `usage: UsageStatus`. Two keyboard-accessible commands reuse existing behavior: `snooze` and `award_pat`.

## Data Models

### Validated Configuration Bounds

| Field | Type | Constraint |
|---|---|---|
| `mode` | `EnforcementMode` | `close` or `nag` |
| `countdownSeconds` | integer seconds | 0–30 |
| `snoozeMinutes` | integer minutes | 1–240 |
| `pollMs` | integer ms | 250–10,000 |
| `urlPollMs` | integer ms | 500–10,000 |
| `cat.scale` | finite number | 0.7–2.2 |
| `cat.speed` | finite number | 0.5–2.0 |
| `rule.id` | trimmed string | non-empty, unique, maximum 64 characters |
| `rule.label` | trimmed string | non-empty, maximum 120 characters |
| `rule.all[]`, `rule.any[]` | trimmed strings | non-empty, maximum 256 characters each |
| `rule.grace` | integer seconds | 0–86,400 |
| `rule.action` | `RuleAction` | `tab` or `close` |
| `dailyAllowance.minutes` | integer minutes | 0–1,440 |
| schedule time | string | strict local `HH:MM` |

### Usage File

| Field | Type | Constraint |
|---|---|---|
| `version` | integer | `1` |
| `localDate` | `YYYY-MM-DD` | current accounting date |
| `usedSeconds` | integer | non-negative, saturating |

No per-rule or per-event records are permitted.

## Implementation Order

1. **Typed configuration and transactional validation** — foundation for every later setting.
2. **Confirmed close outcomes** — correctness of rewards and statistics.
3. **Daily allowance and weekly schedule** — new domain state and UI.
4. **Accessibility and keyboard controls** — stabilize semantics after the settings layout is final.
5. **CI and draft-first release verification** — enforce the completed quality contract.

Each stage receives its own red/green test slice and must leave the full suite green before the next begins.

## Out of Scope

- OS-1: Cloud sync, accounts, telemetry, exported reports, browsing history, or productivity scoring.
- OS-2: Multiple daily allowance buckets, per-rule budgets, holiday calendars, date exceptions, or multiple schedule windows per day.
- OS-3: Global/system-wide keyboard shortcuts or making the click-through pet window keyboard-focusable.
- OS-4: Proving that Windows destroyed a process; confirmation observes window identity/rule state only.
- OS-5: macOS/Linux builds; the current application and enforcement integration are Windows-specific.
- OS-6: Authenticode certificate acquisition. Existing Tauri updater signatures remain required; Windows publisher signing can be a separate release-security project.
- OS-7: Visual redesign, new cat art, localization, or changes to XP amounts.

## Approval Decisions

Approval of this draft confirms these product choices:

1. “Daily limit” means an allowed number of foreground minutes of matched content before normal enforcement begins—not a cap on closes.
2. One optional weekly local-time window is sufficient; multiple windows and exceptions are out of scope.
3. Accessibility keyboard controls live inside Settings; no global hotkeys are installed.
4. Existing users see no behavioral change until they explicitly enable allowance or schedule controls.
