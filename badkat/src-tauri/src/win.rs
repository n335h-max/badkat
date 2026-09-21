//! The Windows half: what is in front, and how to close it.
//!
//! Two facts about browsers drive this whole module:
//!
//!   1. A window title only ever carries the site's own `<title>`, so
//!      Instagram Reels reads as "Instagram" and a YouTube Short as
//!      "<name> - YouTube". The word "reels" or "shorts" exists only in
//!      the URL, which has to be read out of the UI Automation tree.
//!   2. A browser playing fullscreen video removes its address bar from
//!      that tree entirely, so the URL goes blank exactly while you are
//!      watching a film — and only the title is left.
//!
//! So both are collected, and the rules match against both.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::config::{Rule, RuleAction};

use uiautomation::{UIAutomation, UIElement};
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_W,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, PostMessageW,
    WM_CLOSE,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub hwnd: isize,
    pub pid: u32,
    pub proc: String,
    pub title: String,
    pub url: String,
}

pub const BROWSERS: &[&str] = &[
    "chrome",
    "msedge",
    "firefox",
    "brave",
    "opera",
    "vivaldi",
    "arc",
    "chromium",
    "librewolf",
    "zen",
    "opera_gx",
];

pub fn is_browser(proc_name: &str) -> bool {
    let p = proc_name.to_ascii_lowercase();
    BROWSERS.iter().any(|b| p == *b)
}

/// The horizontal centre of a window, in physical screen pixels. Used so
/// the cat can walk to beneath the actual offending window rather than
/// an arbitrary spot on screen.
pub fn window_center_x(hwnd_val: isize) -> Option<f64> {
    let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect).ok()? };
    if rect.right <= rect.left {
        return None;
    }
    Some((rect.left + rect.right) as f64 / 2.0)
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 1024];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// Drops a leading unread-count badge — "(8) ", "(12) " — that browsers
/// prepend to the tab title and tick on their own between the moment the
/// cat is triggered and the moment it acts. Without this, a badge that
/// changed from "(8)" to "(9)" during the countdown reads as "the window
/// moved on" and the close is skipped.
fn strip_notification_prefix(title: &str) -> &str {
    let trimmed = title.trim_start();
    if let Some(rest) = trimmed.strip_prefix('(') {
        if let Some(close) = rest.find(')') {
            let inner = &rest[..close];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return rest[close + 1..].trim_start();
            }
        }
    }
    trimmed
}

fn process_name(pid: u32) -> String {
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_string()
    }
}

/// Reads a browser's address bar out of the UI Automation tree.
///
/// The resolved element is cached per window: walking the tree costs
/// ~100ms and would otherwise be paid on every poll.
pub struct UrlReader {
    automation: Option<UIAutomation>,
    cached_for: isize,
    cached_bar: Option<UIElement>,
    last_url: String,
    last_read: Option<Instant>,
}

impl UrlReader {
    pub fn new() -> Self {
        Self {
            automation: UIAutomation::new().ok(),
            cached_for: 0,
            cached_bar: None,
            last_url: String::new(),
            last_read: None,
        }
    }

    fn resolve_bar(&mut self, hwnd: isize) -> Option<UIElement> {
        if self.cached_for == hwnd {
            if let Some(bar) = &self.cached_bar {
                return Some(bar.clone());
            }
        }

        let automation = self.automation.as_ref()?;
        let root = automation
            .element_from_handle(uiautomation::types::Handle::from(hwnd))
            .ok()?;
        let matcher = automation
            .create_matcher()
            .from_ref(&root)
            .control_type(uiautomation::controls::ControlType::Edit)
            .depth(12)
            .timeout(400);

        let edits = matcher.find_all().ok()?;

        // Chrome/Edge call it "Address and search bar"; Firefox says
        // "Search with ... or enter address".
        let mut chosen = edits
            .iter()
            .find(|e| {
                e.get_name()
                    .map(|n| n.to_ascii_lowercase().contains("address"))
                    .unwrap_or(false)
            })
            .cloned();

        // otherwise take the first edit already holding something URL-shaped
        if chosen.is_none() {
            chosen = edits
                .iter()
                .find(|e| {
                    read_value(e)
                        .map(|v| v.contains('.') || v.starts_with("http"))
                        .unwrap_or(false)
                })
                .cloned();
        }

        self.cached_for = hwnd;
        self.cached_bar = chosen.clone();
        chosen
    }

    pub fn url_for(&mut self, hwnd: isize, throttle: Duration) -> String {
        let fresh_enough = self
            .last_read
            .map(|t| t.elapsed() < throttle && self.cached_for == hwnd)
            .unwrap_or(false);
        if fresh_enough {
            return self.last_url.clone();
        }

        let value = self
            .resolve_bar(hwnd)
            .and_then(|bar| read_value(&bar))
            .unwrap_or_default();

        // a stale cached element throws instead of returning: drop it so
        // the next poll re-resolves against the rebuilt tree
        if value.is_empty() {
            self.cached_bar = None;
        }

        self.last_url = value.clone();
        self.last_read = Some(Instant::now());
        value
    }

    pub fn forget(&mut self) {
        self.cached_for = 0;
        self.cached_bar = None;
        self.last_url.clear();
    }
}

fn read_value(element: &UIElement) -> Option<String> {
    let pattern = element
        .get_pattern::<uiautomation::patterns::UIValuePattern>()
        .ok()?;
    pattern.get_value().ok()
}

/// The window that currently has your attention.
pub fn foreground(reader: &mut UrlReader, url_throttle: Duration) -> Option<Snapshot> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }

    snapshot_for_hwnd(hwnd.0 as isize, reader, url_throttle)
}

fn snapshot_for_hwnd(
    hwnd_value: isize,
    reader: &mut UrlReader,
    url_throttle: Duration,
) -> Option<Snapshot> {
    let hwnd = HWND(hwnd_value as *mut std::ffi::c_void);

    let title = window_title(hwnd);
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let proc = process_name(pid);

    let url = if is_browser(&proc) {
        reader.url_for(hwnd.0 as isize, url_throttle)
    } else {
        reader.forget();
        String::new()
    };

    Some(Snapshot {
        hwnd: hwnd.0 as isize,
        pid,
        proc,
        title,
        url,
    })
}

fn confirmation_result(
    action: RuleAction,
    original: &Snapshot,
    rule: &Rule,
    url_required: bool,
    observed: Option<&Snapshot>,
) -> Option<bool> {
    let Some(observed) = observed else {
        return Some(true);
    };
    if observed.hwnd != original.hwnd || observed.pid != original.pid {
        return Some(true);
    }
    if action == RuleAction::Close {
        return Some(false);
    }
    if observed.proc.trim().is_empty() {
        return None;
    }
    if !observed.proc.eq_ignore_ascii_case(&original.proc) {
        return Some(true);
    }
    if url_required && observed.url.trim().is_empty() {
        return None;
    }
    Some(!crate::rules::matches_rule(rule, observed))
}

pub fn confirm_closed(
    original: &Snapshot,
    rule: &Rule,
    url_required: bool,
    action: RuleAction,
) -> ActResult {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut reader = UrlReader::new();
    let confirmed = loop {
        let observed = snapshot_for_hwnd(original.hwnd, &mut reader, Duration::ZERO);
        if confirmation_result(action, original, rule, url_required, observed.as_ref())
            == Some(true)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(125));
    };
    if initialized {
        unsafe { CoUninitialize() };
    }

    if confirmed {
        ActResult {
            acted: true,
            reason: action.as_str().into(),
        }
    } else {
        ActResult {
            acted: false,
            reason: "the target did not close".into(),
        }
    }
}

/// Captures the foreground identity and browser URL at action time.
/// This deliberately uses a new reader so a cached address-bar value
/// from the monitor cannot authorize a tab that changed during the
/// countdown.
pub fn fresh_foreground() -> Option<Snapshot> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut reader = UrlReader::new();
    let snap = foreground(&mut reader, Duration::ZERO);
    if initialized {
        unsafe { CoUninitialize() };
    }
    snap
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActResult {
    pub acted: bool,
    pub reason: String,
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Closes the offending window — but only if it is *still* the
/// foreground window and its title still matches what was detected.
///
/// This re-check is the whole safety story. Between the cat deciding to
/// act and this running, you may have alt-tabbed to real work; sending
/// Ctrl+W blind would close a tab of whatever happened to be in front.
pub fn close_target(target: &Snapshot, action: RuleAction) -> ActResult {
    let current = unsafe { GetForegroundWindow() };
    if current.is_invalid() || current.0 as isize != target.hwnd {
        return ActResult {
            acted: false,
            reason: "window is no longer in front".into(),
        };
    }

    // Browsers rewrite the leading part of a title constantly (unread
    // counts, "(2) ..."), so the volatile prefix is stripped from both
    // sides and only the trailing part is compared.
    let now_title = strip_notification_prefix(&window_title(current)).to_string();
    let want = strip_notification_prefix(&target.title);
    // Take the last 40 chars of what is left. On a short title ("(8)
    // Instagram - Google Chrome") that is the whole thing *after* the
    // "(8)" has already been removed, which is what we want.
    let expect: String = want.chars().rev().take(40).collect();
    let expect: String = expect.chars().rev().collect();
    if !expect.is_empty() && !now_title.contains(&expect) {
        return ActResult {
            acted: false,
            reason: "the window moved on".into(),
        };
    }

    if action == RuleAction::Tab {
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_W, false),
            key(VK_W, true),
            key(VK_CONTROL, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return ActResult {
                acted: false,
                reason: "the keystroke was blocked".into(),
            };
        }
    } else {
        if unsafe { PostMessageW(Some(current), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_err() {
            return ActResult {
                acted: false,
                reason: "the close message was blocked".into(),
            };
        }
    }

    ActResult {
        acted: true,
        reason: action.as_str().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn drops_a_leading_unread_badge() {
        assert_eq!(
            strip_notification_prefix("(8) Instagram - Google Chrome"),
            "Instagram - Google Chrome"
        );
        assert_eq!(
            strip_notification_prefix("(127) WhatsApp - Brave"),
            "WhatsApp - Brave"
        );
    }

    #[test]
    fn a_badge_change_no_longer_looks_like_a_new_window() {
        // this is the bug: the tail comparison used to keep the "(8)"
        // because the whole title was shorter than 40 chars
        assert_eq!(
            strip_notification_prefix("(8) Instagram - Google Chrome"),
            strip_notification_prefix("(9) Instagram - Google Chrome")
        );
    }

    #[test]
    fn leaves_ordinary_titles_alone() {
        assert_eq!(
            strip_notification_prefix("Instagram - Google Chrome"),
            "Instagram - Google Chrome"
        );
        // parenthesised but not a count: not a badge, leave it
        assert_eq!(
            strip_notification_prefix("(Draft) Report - Word"),
            "(Draft) Report - Word"
        );
    }

    fn target(url: &str) -> Snapshot {
        Snapshot {
            hwnd: 7,
            pid: 42,
            proc: "chrome".into(),
            title: "A Short - YouTube".into(),
            url: url.into(),
        }
    }

    #[test]
    fn missing_or_reused_window_confirms_the_original_is_gone() {
        let cfg = Config::default();
        let rule = cfg.rules[0].clone();
        let original = target("youtube.com/shorts/abc");
        assert_eq!(
            confirmation_result(RuleAction::Close, &original, &rule, true, None),
            Some(true)
        );

        let mut reused = original.clone();
        reused.pid = 99;
        assert_eq!(
            confirmation_result(RuleAction::Close, &original, &rule, true, Some(&reused)),
            Some(true)
        );
    }

    #[test]
    fn tab_requires_the_original_rule_to_stop_matching() {
        let cfg = Config::default();
        let rule = cfg.rules[0].clone();
        let original = target("youtube.com/shorts/abc");
        assert_eq!(
            confirmation_result(RuleAction::Tab, &original, &rule, true, Some(&original)),
            Some(false)
        );

        let changed = target("youtube.com/watch?v=abc");
        assert_eq!(
            confirmation_result(RuleAction::Tab, &original, &rule, true, Some(&changed)),
            Some(true)
        );
    }

    #[test]
    fn unreadable_required_url_is_inconclusive() {
        let cfg = Config::default();
        let rule = cfg.rules[0].clone();
        let original = target("youtube.com/shorts/abc");
        let unreadable = target("");
        assert_eq!(
            confirmation_result(RuleAction::Tab, &original, &rule, true, Some(&unreadable)),
            None
        );
    }
}
