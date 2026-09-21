//! BadKat — a desktop cat that closes your doomscrolling.
//!
//! Three moving parts:
//!   * a transparent, click-through overlay strip along the bottom of
//!     the screen, where the cat lives
//!   * a settings window
//!   * a background thread that watches the foreground window and
//!     decides when the cat should get up
//!
//! The overlay never gets a window handle and never closes anything by
//! itself. It asks, and the Rust side re-verifies before acting.

mod action_tickets;
mod config;
mod progress;
mod rules;
mod storage;
mod usage;
mod win;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

use action_tickets::ActionTickets;
use config::{Config, EnforcementMode};
use progress::Progress;
use win::Snapshot;

const COOLDOWN: Duration = Duration::from_secs(12);
const TRAIL_MAX: usize = 40;
/// A pat is worth XP, but clicking the cat is free and fast. Without a
/// floor between awards the level system is just a clicker game.
const PAT_COOLDOWN: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailEntry {
    pub event: String,
    pub rule: String,
    pub detail: String,
    pub at: String,
}

pub struct Shared {
    cfg: Config,
    dir: PathBuf,
    notes: Vec<String>,
    snooze_until: Option<Instant>,
    cooldown_until: Option<Instant>,
    last: Option<Snapshot>,
    matched: Option<String>,
    remaining: f64,
    trail: Vec<TrailEntry>,
    started: Instant,
    progress: Progress,
    last_pat: Option<Instant>,
    action_tickets: ActionTickets,
    usage: usage::UsageTracker,
}

impl Shared {
    fn snoozing(&self) -> bool {
        self.snooze_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    fn note(&mut self, event: &str, rule: &str, detail: &str) {
        let secs = self.started.elapsed().as_secs();
        // debug builds narrate to stdout; `cargo run` is then a live log
        #[cfg(debug_assertions)]
        eprintln!("[badkat] {event:<8} {rule:<18} {detail}");
        self.trail.push(TrailEntry {
            event: event.into(),
            rule: rule.into(),
            detail: detail.into(),
            at: format!("+{}:{:02}", secs / 60, secs % 60),
        });
        if self.trail.len() > TRAIL_MAX {
            self.trail.remove(0);
        }
    }
}

pub type State = Arc<Mutex<Shared>>;

/* ------------------------------------------------------------------
   payloads
------------------------------------------------------------------ */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BustPayload {
    rule_id: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_id: Option<String>,
    mode: EnforcementMode,
    countdown: u64,
    /// The offending window's horizontal centre, in the overlay's own
    /// local coordinate space (physical px, already offset by the work
    /// area's origin) — so the cat can walk to beneath the actual
    /// window instead of an arbitrary spot on screen. Absent if the
    /// window rect couldn't be read.
    window_center_x: Option<f64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    watching: bool,
    label: String,
    remaining: f64,
}

/// What every surface needs to draw the affection meter. `needed` is
/// derived rather than stored, so the level curve can be retuned without
/// migrating anyone's saved progress. `gained` is non-zero only on the
/// award that crossed a threshold — that is the pet window's cue to play
/// the level-up animation — and `awarded` is what this particular event
/// paid out, which is what the floating "+25 XP" reads.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressView {
    level: u32,
    xp: u32,
    needed: u32,
    total_xp: u64,
    closes: u32,
    pats: u32,
    gained: u32,
    awarded: u32,
}

impl ProgressView {
    fn of(p: &Progress, gained: u32, awarded: u32) -> Self {
        Self {
            level: p.level,
            xp: p.xp,
            needed: p.needed(),
            total_xp: p.total_xp,
            closes: p.closes,
            pats: p.pats,
            gained,
            awarded,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusInfo {
    enabled: bool,
    mode: EnforcementMode,
    snoozing: bool,
    snooze_seconds_left: u64,
    foreground: Option<Snapshot>,
    haystack: String,
    matched: Option<String>,
    remaining: f64,
    notes: Vec<String>,
    trail: Vec<TrailEntry>,
    usage: usage::UsageStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveConfigResult {
    saved: bool,
    issues: Vec<config::ValidationIssue>,
}

/* ------------------------------------------------------------------
   commands
------------------------------------------------------------------ */

#[tauri::command]
fn get_config(state: tauri::State<State>) -> Config {
    state.lock().unwrap().cfg.clone()
}

#[tauri::command]
fn save_config(
    app: AppHandle,
    state: tauri::State<State>,
    cfg: Config,
) -> Result<SaveConfigResult, String> {
    let issues = cfg.validate();
    if !issues.is_empty() {
        return Ok(SaveConfigResult {
            saved: false,
            issues,
        });
    }
    let mut guard = state.lock().unwrap();
    let dir = guard.dir.clone();
    config::save(&dir, &cfg).map_err(|e| e.to_string())?;
    guard.cfg = cfg.clone();
    guard.action_tickets.clear();
    guard.note("settings", "", "saved");
    drop(guard);

    // the overlay reads cat size/speed straight from the config
    let _ = app.emit("config-changed", cfg);
    Ok(SaveConfigResult {
        saved: true,
        issues: Vec::new(),
    })
}

#[tauri::command]
fn reset_rules(state: tauri::State<State>) -> Vec<config::Rule> {
    let mut guard = state.lock().unwrap();
    guard.cfg.rules = config::default_rules();
    guard.action_tickets.clear();
    let dir = guard.dir.clone();
    let cfg = guard.cfg.clone();
    let _ = config::save(&dir, &cfg);
    guard.cfg.rules.clone()
}

#[tauri::command]
fn status(state: tauri::State<State>) -> StatusInfo {
    let guard = state.lock().unwrap();
    let snoozing = guard.snoozing();
    StatusInfo {
        enabled: guard.cfg.enabled,
        mode: guard.cfg.mode,
        snoozing,
        snooze_seconds_left: guard
            .snooze_until
            .map(|t| t.saturating_duration_since(Instant::now()).as_secs())
            .unwrap_or(0),
        haystack: guard.last.as_ref().map(rules::haystack).unwrap_or_default(),
        foreground: guard.last.clone(),
        matched: guard.matched.clone(),
        remaining: guard.remaining,
        notes: guard.notes.clone(),
        trail: guard.trail.clone(),
        usage: guard.usage.status(&guard.cfg, usage::local_now()),
    }
}

/* ------------------------------------------------------------------
   updates
   ------------------------------------------------------------------
   The whole flow lives on this side rather than in the frontend. The
   settings page is plain script tags with no bundler, so it cannot
   import the updater's JS package; exposing two commands instead keeps
   the dashboard doing what it already does everywhere else — invoke()
   and listen() — and keeps the download loop where the errors are
   actually legible.
------------------------------------------------------------------ */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    available: bool,
    current: String,
    version: String,
    notes: String,
    date: String,
    /// set when the check itself failed — offline, DNS, no release yet —
    /// so the dashboard can say why rather than just "no updates"
    error: String,
}

impl UpdateInfo {
    fn none(current: &str, error: String) -> Self {
        Self {
            available: false,
            current: current.into(),
            version: String::new(),
            notes: String::new(),
            date: String::new(),
            error,
        }
    }
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn check_update(app: AppHandle) -> UpdateInfo {
    use tauri_plugin_updater::UpdaterExt;
    let current = current_version(&app);

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => return UpdateInfo::none(&current, e.to_string()),
    };

    match updater.check().await {
        Ok(Some(update)) => UpdateInfo {
            available: true,
            current,
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
            date: update.date.map(|d| d.to_string()).unwrap_or_default(),
            error: String::new(),
        },
        Ok(None) => UpdateInfo::none(&current, String::new()),
        Err(e) => UpdateInfo::none(&current, e.to_string()),
    }
}

/// Downloads and installs, reporting progress as it goes. On Windows
/// the NSIS installer takes over at the end and restarts the app, so
/// anything after `install` may never run — the caller should treat the
/// "done" event as the last thing it will hear.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;

    let mut downloaded: u64 = 0;
    let emitter = app.clone();
    let progress = app.clone();

    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let pct = total
                    .map(|t| {
                        if t > 0 {
                            (downloaded as f64 / t as f64) * 100.0
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0);
                let _ = progress.emit(
                    "update-progress",
                    serde_json::json!({
                        "downloaded": downloaded,
                        "total": total,
                        "percent": pct
                    }),
                );
            },
            move || {
                let _ = emitter.emit("update-progress", serde_json::json!({ "installing": true }));
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub const SITE_URL: &str = "https://badkat.cypherion.tech";
pub const REPO_URL: &str = "https://github.com/X-DIABLO-X/badkat";

/// Opens one of the project's own links in the default browser.
///
/// Takes a KEY, not a URL. A command that opened whatever string the
/// frontend handed it would be a way to launch arbitrary things through
/// the shell; there are exactly two links worth opening, so they are
/// named here and the frontend can only pick between them.
#[tauri::command]
fn open_link(which: String) {
    let url = match which.as_str() {
        "site" => SITE_URL,
        "repo" => REPO_URL,
        _ => return,
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // ShellExecute via rundll32 rather than `cmd /C start`: no shell
        // parsing involved, so the argument cannot be read as a command
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

/* ------------------------------------------------------------------
   affection
------------------------------------------------------------------ */

/// Banks XP, persists it, and tells every window. The single funnel for
/// earning, so the saved file and the two surfaces can never disagree
/// about the level. Awarding is deliberately server-side: the overlay
/// asks the cat to be fed, it does not get to decide how much.
fn bank_xp(app: &AppHandle, state: &State, amount: u32, kind: &str) -> ProgressView {
    let (snap, gained, dir) = {
        let mut guard = state.lock().unwrap();
        match kind {
            "close" => guard.progress.closes += 1,
            "pat" => guard.progress.pats += 1,
            _ => {}
        }
        let gained = guard.progress.award(amount);
        if gained > 0 {
            let level = guard.progress.level;
            guard.note("level up", "", &format!("now level {level}"));
        }
        (guard.progress.clone(), gained, guard.dir.clone())
    };

    let _ = progress::save(&dir, &snap);
    let view = ProgressView::of(&snap, gained, amount);
    let _ = app.emit("progress", view.clone());
    view
}

#[tauri::command]
fn get_progress(state: tauri::State<State>) -> ProgressView {
    ProgressView::of(&state.lock().unwrap().progress, 0, 0)
}

/// A pat from the overlay. Rate-limited here rather than in the
/// overlay, because the overlay is the thing that would be spammed.
#[tauri::command]
fn award_pat(app: AppHandle, state: tauri::State<State>) -> ProgressView {
    {
        let mut guard = state.lock().unwrap();
        let now = Instant::now();
        if let Some(last) = guard.last_pat {
            if now.duration_since(last) < PAT_COOLDOWN {
                return ProgressView::of(&guard.progress, 0, 0);
            }
        }
        guard.last_pat = Some(now);
    }
    let inner: State = state.inner().clone();
    bank_xp(&app, &inner, progress::XP_PAT, "pat")
}

/// The overlay asks for this when its countdown reaches zero.
#[tauri::command]
async fn act(app: AppHandle, state: tauri::State<'_, State>, action_id: String) -> win::ActResult {
    let inner: State = state.inner().clone();
    let now = Instant::now();
    let fresh = win::fresh_foreground().unwrap_or_default();
    let authorized = {
        let mut guard = inner.lock().unwrap();
        let mut cfg = guard.cfg.clone();
        let usage_state = guard.usage.status(&cfg, usage::local_now()).state;
        if guard.snoozing()
            || matches!(
                usage_state,
                usage::UsageState::OutsideSchedule | usage::UsageState::Available
            )
        {
            cfg.mode = EnforcementMode::Nag;
        }
        guard
            .action_tickets
            .authorize(&action_id, &fresh, &cfg, now)
    };
    let authorized = match authorized {
        Ok(action) => action,
        Err(reason) => {
            let mut guard = inner.lock().unwrap();
            guard.note("skipped", "", reason);
            return win::ActResult {
                acted: false,
                reason: reason.into(),
            };
        }
    };

    let dispatched = win::close_target(&authorized.target, authorized.action);
    let result = if dispatched.acted {
        let target = authorized.target;
        let rule = authorized.rule;
        let url_required = authorized.url_required;
        let action = authorized.action;
        tauri::async_runtime::spawn_blocking(move || {
            win::confirm_closed(&target, &rule, url_required, action)
        })
        .await
        .unwrap_or_else(|_| win::ActResult {
            acted: false,
            reason: "close confirmation failed".into(),
        })
    } else {
        dispatched
    };

    {
        let mut guard = inner.lock().unwrap();
        guard.cooldown_until = Some(Instant::now() + COOLDOWN);
        guard.note(
            if result.acted { "closed" } else { "skipped" },
            "",
            &result.reason,
        );
    }

    // only a window that actually closed is worth anything — a skipped
    // or refused close would otherwise pay out for doing nothing
    if result.acted {
        bank_xp(&app, &inner, progress::XP_CLOSE, "close");
    }
    result
}

#[tauri::command]
fn snooze(state: tauri::State<State>, minutes: Option<u64>) -> u64 {
    let mut guard = state.lock().unwrap();
    let mins = minutes.unwrap_or(guard.cfg.snooze_minutes);
    guard.snooze_until = Some(Instant::now() + Duration::from_secs(mins * 60));
    guard.action_tickets.clear();
    guard.note("snooze", "", &format!("{mins} min"));
    mins
}

#[tauri::command]
fn cancel_snooze(state: tauri::State<State>) {
    let mut guard = state.lock().unwrap();
    guard.snooze_until = None;
}

/// Moves the small, always-interactive "hitbox" window to sit exactly
/// over the cat's current position (x, y, width, height in the "pet"
/// window's own local coordinates). Called on load, whenever the cat
/// moves, and whenever its size changes in settings.
///
/// Two things were tried before this and both failed on Windows:
///
///   - `set_ignore_cursor_events` is a blunt WS_EX_TRANSPARENT toggle:
///     once set, the window receives NO mouse input at all, including
///     mousemove, so there is no way to detect "the cursor just
///     reached the cat" to turn it back off.
///   - Subclassing the overlay's WndProc to answer WM_NCHITTEST
///     per-pixel *looked* right and is the standard Win32 technique for
///     this — but WebView2 does not consult it: a click was measurably
///     still delivered to the webview even on pixels the hit-test
///     handler had just answered HTTRANSPARENT for (confirmed by
///     logging both sides of that exact click).
///
/// So instead: the "pet" window is permanently click-through, and a
/// second, genuinely small window sits on top of it wherever the cat
/// currently is. A small ordinary window blocking clicks only within
/// its own bounds needs no tricks at all — that's just how windows work.
#[tauri::command]
fn set_hitbox(app: AppHandle, x: i32, y: i32, w: i32, h: i32) {
    let Some(hitbox) = app.get_webview_window("hitbox") else {
        return;
    };
    if w <= 0 || h <= 0 {
        // park it off-screen rather than trying to shrink to nothing,
        // which some window managers refuse or clamp oddly
        let _ = hitbox.set_position(PhysicalPosition::new(-10_000, -10_000));
        return;
    }
    let (ox, oy, _, _) = work_area();
    let _ = hitbox.set_position(PhysicalPosition::new(ox + x, oy + y));
    let _ = hitbox.set_size(PhysicalSize::new(w.max(1), h.max(1)));
    #[cfg(debug_assertions)]
    eprintln!(
        "[badkat] hitbox   screen=({},{},{},{})",
        ox + x,
        oy + y,
        ox + x + w,
        oy + y + h
    );
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn set_cat_visible(app: AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window("pet") {
        let _ = if visible { w.show() } else { w.hide() };
    }
}

/// Fires a pretend interception so you can see what the cat does
/// without waiting to be caught.
#[tauri::command]
fn preview_bust(app: AppHandle, state: tauri::State<State>) {
    let guard = state.lock().unwrap();
    let payload = BustPayload {
        rule_id: "preview".into(),
        label: "YouTube Shorts".into(),
        action_id: None,
        mode: EnforcementMode::Nag, // a preview never closes anything
        countdown: guard.cfg.countdown_seconds,
        window_center_x: None,
    };
    drop(guard);
    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.emit("bust", payload);
    }
}

/// Lets the overlay write into the same debug stream as the Rust side.
/// A transparent, always-on-top webview has no devtools you can click
/// into, so without this its console is simply unreachable.
#[tauri::command]
fn jslog(msg: String) {
    #[cfg(debug_assertions)]
    eprintln!("[badkat] overlay  {msg}");
    #[cfg(not(debug_assertions))]
    let _ = msg;
}

#[tauri::command]
fn preview_state(app: AppHandle, name: String) {
    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.emit("preview-state", name);
    }
}

/* ------------------------------------------------------------------
   windows
------------------------------------------------------------------ */

fn work_area() -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut rect = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut RECT as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        return (0, 0, 1920, 1080);
    }
    (
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}

/// Windows will happily give an always-on-top window focus when it is
/// clicked. WS_EX_NOACTIVATE is what stops the cat pulling you out of
/// whatever you were typing in.
#[cfg(windows)]
fn make_non_activating(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    };
    if let Ok(handle) = window.hwnd() {
        let hwnd = HWND(handle.0 as *mut std::ffi::c_void);
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
        }
    }
}

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    // The overlay covers the *entire* work area, not just a bottom strip —
    // dragging the cat upward and letting gravity drop it needs the room.
    // It is permanently click-through: see build_hitbox_window() below for
    // how clicks actually reach the cat.
    let (x, y, w, h) = work_area();
    let window = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("pet.html".into()))
        .title("BadKat")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;

    window.set_position(PhysicalPosition::new(x, y))?;
    window.set_size(PhysicalSize::new(w, h))?;
    window.set_ignore_cursor_events(true)?;

    #[cfg(windows)]
    make_non_activating(&window);

    window.show()?;
    Ok(())
}

/* -------------------------------------------------------------------
   click-through, done with a second small window
   ---------------------------------------------------------------
   Two things were tried before this and both failed on Windows:

     - set_ignore_cursor_events is a blunt WS_EX_TRANSPARENT toggle:
       once set, the window receives NO mouse input at all, including
       mousemove, so there is no way to detect "the cursor just
       reached the cat" to turn it back off from the JS side.
     - Subclassing the overlay's WndProc to answer WM_NCHITTEST
       per-pixel is the standard Win32 technique for exactly this and
       *looked* right — but WebView2 does not consult it. A click was
       measurably still delivered to the webview on a pixel the
       hit-test handler had, in the same instant, answered
       HTTRANSPARENT for (confirmed by logging both sides of one
       click). Whatever WebView2 uses to route input to its own
       composited surface does not go through the parent's NCHITTEST
       answer.

   So instead: the "pet" window above is permanently click-through
   (set_ignore_cursor_events(true), never toggled), and this second
   "hitbox" window is small, ordinary, and always fully interactive —
   no transparency trick needed at all, because a normal small window
   only ever blocks clicks within its own bounds. Rust moves it to sit
   exactly over the cat (see set_hitbox()); its own tiny page just
   relays mousedown/mousemove/mouseup to "pet", which owns all the
   actual drag/drop logic and rendering.
------------------------------------------------------------------- */
fn build_hitbox_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "hitbox", WebviewUrl::App("hit.html".into()))
        .title("BadKat hitbox")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;

    // starts with zero footprint, parked off-screen, until pet.js's
    // first set_hitbox() call places it over the cat
    window.set_position(PhysicalPosition::new(-10_000, -10_000))?;
    window.set_size(PhysicalSize::new(1, 1))?;

    #[cfg(windows)]
    make_non_activating(&window);

    window.show()?;
    Ok(())
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("BadKat — Settings")
        .inner_size(940.0, 700.0)
        .min_inner_size(720.0, 520.0)
        .resizable(true)
        .build();
    if let Ok(w) = built {
        let _ = w.set_focus();
    }
}

/* ------------------------------------------------------------------
   the watch
------------------------------------------------------------------ */

fn spawn_monitor(app: AppHandle, state: State) {
    std::thread::spawn(move || {
        // UI Automation is COM; this thread owns the apartment.
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let mut reader = win::UrlReader::new();
        let mut match_id = String::new();
        let mut match_since = Instant::now();
        let mut pending = String::new();
        let mut last_hwnd: isize = 0;
        #[allow(unused_mut, unused_assignments)]
        let mut last_seen = String::new();

        loop {
            let (poll_ms, url_ms) = {
                let guard = state.lock().unwrap();
                (guard.cfg.poll_ms.max(250), guard.cfg.url_poll_ms.max(500))
            };

            if let Some(snap) = win::foreground(&mut reader, Duration::from_millis(url_ms)) {
                if snap.hwnd != last_hwnd {
                    last_hwnd = snap.hwnd;
                    pending.clear();
                }

                let mut guard = state.lock().unwrap();
                guard.last = Some(snap.clone());

                let paused = guard.snoozing()
                    || guard
                        .cooldown_until
                        .map(|t| Instant::now() < t)
                        .unwrap_or(false);

                let raw_hit = rules::matches(&guard.cfg, &snap).cloned();
                let sample_now = Instant::now();
                let cfg = guard.cfg.clone();
                let usage_decision =
                    guard
                        .usage
                        .tick(&cfg, usage::local_now(), sample_now, raw_hit.is_some());
                if guard.usage.should_flush(sample_now) {
                    let saved = usage::save(&guard.dir, guard.usage.record()).is_ok();
                    if saved {
                        guard.usage.mark_flushed(sample_now);
                    }
                }

                let hit = if paused || !usage_decision.enforcement_allowed {
                    None
                } else {
                    raw_hit
                };

                #[cfg(debug_assertions)]
                {
                    let key = format!("{}|{}", snap.title, snap.url);
                    if key != last_seen {
                        last_seen = key;
                        eprintln!(
                            "[badkat] front    proc={} url={:?} title={:?}",
                            snap.proc, snap.url, snap.title
                        );
                    }
                }

                match hit {
                    None => {
                        guard.matched = None;
                        guard.remaining = 0.0;
                        match_id.clear();
                        drop(guard);
                        let _ = app.emit_to(
                            "pet",
                            "status",
                            StatusPayload {
                                watching: false,
                                label: String::new(),
                                remaining: 0.0,
                            },
                        );
                    }
                    Some(rule) => {
                        // The clock runs per matched rule, not per window:
                        // switching from a work tab to Reels inside an
                        // already-focused browser has to start the grace
                        // at zero, and swiping to the next reel must not
                        // restart it.
                        let id = format!("{}|{}", rule.id, snap.hwnd);
                        if match_id != id {
                            match_id = id;
                            match_since = Instant::now();
                            guard.note("spotted", &rule.id, &snap.title);
                        }

                        let held = match_since.elapsed().as_secs_f64();
                        let remaining = (rule.grace as f64 - held).max(0.0);
                        guard.matched = Some(rule.label.clone());
                        guard.remaining = remaining;

                        let key = format!("{}|{}|{}", snap.hwnd, snap.title, snap.url);
                        let countdown = guard.cfg.countdown_seconds;
                        let mode = guard.cfg.mode;

                        if remaining > 0.0 {
                            drop(guard);
                            let _ = app.emit_to(
                                "pet",
                                "status",
                                StatusPayload {
                                    watching: true,
                                    label: rule.label.clone(),
                                    remaining,
                                },
                            );
                        } else if pending != key {
                            pending = key;
                            let action_id = if mode.is_close() {
                                let action = rules::action_for(&rule, &snap);
                                guard.action_tickets.issue(
                                    &snap,
                                    &rule,
                                    action,
                                    Duration::from_secs(countdown),
                                    Instant::now(),
                                )
                            } else {
                                None
                            };
                            guard.note("caught", &rule.id, &snap.title);
                            drop(guard);

                            // local to the overlay, which sits at the work
                            // area's own origin — see build_overlay()
                            let overlay_x = work_area().0;
                            let window_center_x =
                                win::window_center_x(snap.hwnd).map(|cx| cx - overlay_x as f64);

                            let sent = app.emit_to(
                                "pet",
                                "bust",
                                BustPayload {
                                    rule_id: rule.id.clone(),
                                    label: rule.label.clone(),
                                    action_id,
                                    mode,
                                    countdown,
                                    window_center_x,
                                },
                            );
                            #[cfg(debug_assertions)]
                            eprintln!("[badkat] emit_to(pet, bust) -> {sent:?}");
                        }
                    }
                }
            } else {
                // Advance the monotonic usage clock even when Windows has
                // no readable foreground target. Otherwise the next matched
                // sample would incorrectly charge this entire gap.
                let sample_now = Instant::now();
                let mut guard = state.lock().unwrap();
                let cfg = guard.cfg.clone();
                guard
                    .usage
                    .tick(&cfg, usage::local_now(), sample_now, false);
                if guard.usage.should_flush(sample_now) {
                    let saved = usage::save(&guard.dir, guard.usage.record()).is_ok();
                    if saved {
                        guard.usage.mark_flushed(sample_now);
                    }
                }
                guard.last = None;
                guard.matched = None;
                guard.remaining = 0.0;
                match_id.clear();
                pending.clear();
                drop(guard);
                let _ = app.emit_to(
                    "pet",
                    "status",
                    StatusPayload {
                        watching: false,
                        label: String::new(),
                        remaining: 0.0,
                    },
                );
            }

            std::thread::sleep(Duration::from_millis(poll_ms));
        }
    });
}

/* ------------------------------------------------------------------
   tray
------------------------------------------------------------------ */

fn build_tray(app: &AppHandle, state: State) -> tauri::Result<()> {
    let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    let enabled = CheckMenuItemBuilder::with_id("enabled", "On patrol")
        .checked(state.lock().unwrap().cfg.enabled)
        .build(app)?;
    let snooze_item = MenuItemBuilder::with_id("snooze", "Snooze").build(app)?;
    let show = MenuItemBuilder::with_id("show", "Show / hide the cat").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&settings])
        .separator()
        .items(&[&enabled, &snooze_item, &show])
        .separator()
        .items(&[&quit])
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::WebviewNotFound)?;

    let handle = app.clone();
    TrayIconBuilder::with_id("tray")
        .icon(icon)
        .tooltip("BadKat")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let state = app.state::<State>();
            match event.id().as_ref() {
                "settings" => show_settings(app),
                "enabled" => {
                    let mut guard = state.lock().unwrap();
                    guard.cfg.enabled = !guard.cfg.enabled;
                    guard.action_tickets.clear();
                    let dir = guard.dir.clone();
                    let cfg = guard.cfg.clone();
                    let _ = config::save(&dir, &cfg);
                }
                "snooze" => {
                    let mut guard = state.lock().unwrap();
                    let mins = guard.cfg.snooze_minutes;
                    guard.snooze_until = Some(Instant::now() + Duration::from_secs(mins * 60));
                    guard.action_tickets.clear();
                }
                "show" => {
                    if let Some(w) = app.get_webview_window("pet") {
                        let visible = w.is_visible().unwrap_or(true);
                        let _ = if visible { w.hide() } else { w.show() };
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(move |_tray, event| {
            if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                show_settings(&handle);
            }
        })
        .build(app)?;

    Ok(())
}

/* ------------------------------------------------------------------
   entry
------------------------------------------------------------------ */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();

            let dir = handle
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            // check before loading: load() writes the defaults out on
            // first run, so asking afterwards always says "not first run"
            let first_run = !config::config_path(&dir).exists();
            let (cfg, mut notes) = config::load(&dir);
            #[cfg(debug_assertions)]
            eprintln!(
                "[badkat] config {} - {} rule(s), mode {}, enabled {}{}",
                config::config_path(&dir).display(),
                cfg.rules.len(),
                cfg.mode.as_str(),
                cfg.enabled,
                if notes.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", notes.join("; "))
                }
            );

            let (earned, progress_notes) = progress::load(&dir);
            notes.extend(progress_notes);
            let civil = usage::local_now();
            let (daily_usage, usage_notes) = usage::load(&dir, civil.date());
            notes.extend(usage_notes);
            #[cfg(debug_assertions)]
            eprintln!(
                "[badkat] progress level {} - {}/{} xp, {} closed, {} pats",
                earned.level,
                earned.xp,
                earned.needed(),
                earned.closes,
                earned.pats
            );

            let state: State = Arc::new(Mutex::new(Shared {
                cfg,
                dir,
                notes,
                snooze_until: None,
                cooldown_until: None,
                last: None,
                matched: None,
                remaining: 0.0,
                trail: Vec::new(),
                started: Instant::now(),
                progress: earned,
                last_pat: None,
                action_tickets: ActionTickets::default(),
                usage: usage::UsageTracker::new(daily_usage, Instant::now()),
            }));
            app.manage(state.clone());

            build_overlay(&handle)?;
            build_hitbox_window(&handle)?;
            build_tray(&handle, state.clone())?;
            spawn_monitor(handle.clone(), state);

            // first run has nothing configured yet; --settings forces it
            if first_run || std::env::args().any(|a| a == "--settings") {
                show_settings(&handle);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            reset_rules,
            status,
            act,
            snooze,
            cancel_snooze,
            set_hitbox,
            open_settings,
            set_cat_visible,
            preview_bust,
            preview_state,
            get_progress,
            award_pat,
            check_update,
            install_update,
            open_link,
            jslog
        ])
        .build(tauri::generate_context!())
        .expect("error building BadKat")
        .run(|app, event| {
            // A tray app outlives its windows: closing the settings window
            // must not end the process. But Tauri raises ExitRequested for
            // BOTH cases, so preventing it unconditionally also swallowed
            // the tray's own Quit. `code` is what tells them apart — it is
            // None when the last window closed, and Some(n) when something
            // asked to exit deliberately via app.exit(n).
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                } else {
                    let state = app.state::<State>();
                    let guard = state.lock().unwrap();
                    let _ = usage::save(&guard.dir, guard.usage.record());
                }
            }
        });
}
