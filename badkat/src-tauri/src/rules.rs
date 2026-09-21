//! Deciding whether what is in front of you counts as doomscrolling.

use crate::config::{Config, Rule, RuleAction};
use crate::win::Snapshot;

/// URL first — it is the only place "reels" or "shorts" ever appears.
/// Title second — it is all that is left when a browser goes fullscreen
/// for video and drops its address bar out of the UI tree.
pub fn haystack(snap: &Snapshot) -> String {
    format!(
        "{} | {} | {}",
        snap.url.to_ascii_lowercase(),
        snap.title.to_ascii_lowercase(),
        snap.proc.to_ascii_lowercase()
    )
}

pub fn matches<'a>(cfg: &'a Config, snap: &Snapshot) -> Option<&'a Rule> {
    if !cfg.enabled {
        return None;
    }
    if snap.title.is_empty() && snap.url.is_empty() {
        return None;
    }

    let hay = haystack(snap);
    for pattern in &cfg.never {
        let p = pattern.trim().to_ascii_lowercase();
        if !p.is_empty() && hay.contains(&p) {
            return None;
        }
    }

    cfg.rules.iter().find(|rule| rule_matches(rule, snap))
}

/// Whether this rule only matched because the foreground URL was
/// available. A close ticket remembers this so a failed address-bar
/// read at action time cannot silently weaken a URL rule into a title
/// rule.
pub fn requires_url(rule: &Rule, snap: &Snapshot) -> bool {
    if snap.url.is_empty() || !rule_matches(rule, snap) {
        return false;
    }
    let mut without_url = snap.clone();
    without_url.url.clear();
    !rule_matches(rule, &without_url)
}

pub fn matches_rule(rule: &Rule, snap: &Snapshot) -> bool {
    rule_matches(rule, snap)
}

fn rule_matches(rule: &Rule, snap: &Snapshot) -> bool {
    if !rule.enabled {
        return false;
    }
    let hay = haystack(snap);
    let all_hit = rule
        .all
        .iter()
        .all(|p| hay.contains(&p.trim().to_ascii_lowercase()));
    let any_hit = rule.any.is_empty()
        || rule
            .any
            .iter()
            .any(|p| hay.contains(&p.trim().to_ascii_lowercase()));
    !(rule.all.is_empty() && rule.any.is_empty()) && all_hit && any_hit
}

/// A browser tab is cheap to close; a desktop app is not, so anything
/// that is not a browser gets a real window close instead of Ctrl+W.
pub fn action_for(rule: &Rule, snap: &Snapshot) -> RuleAction {
    if rule.action == RuleAction::Close {
        return RuleAction::Close;
    }
    if crate::win::is_browser(&snap.proc) {
        RuleAction::Tab
    } else {
        RuleAction::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, title: &str, proc: &str) -> Snapshot {
        Snapshot {
            hwnd: 1,
            pid: 1,
            proc: proc.into(),
            title: title.into(),
            url: url.into(),
        }
    }

    #[test]
    fn reels_are_only_visible_in_the_url() {
        let cfg = Config::default();
        // the title says nothing but "Instagram"
        let s = snap(
            "instagram.com/reels/Dcq3rV9B6gh/",
            "(8) Instagram",
            "chrome",
        );
        assert_eq!(matches(&cfg, &s).unwrap().id, "instagram-reels");
    }

    #[test]
    fn fullscreen_video_has_no_url_so_the_title_carries_it() {
        let cfg = Config::default();
        let s = snap("", "Crunchyroll - Season 3 Ordeals", "msedge");
        assert_eq!(matches(&cfg, &s).unwrap().id, "streaming");
    }

    #[test]
    fn shorts_beat_the_long_youtube_leash() {
        let cfg = Config::default();
        let s = snap("youtube.com/shorts/xyz", "Some Short - YouTube", "chrome");
        assert_eq!(matches(&cfg, &s).unwrap().id, "youtube-shorts");
        let s = snap("youtube.com/watch?v=abc", "A Talk - YouTube", "chrome");
        assert_eq!(matches(&cfg, &s).unwrap().grace, 240);
    }

    #[test]
    fn dms_are_not_reels() {
        let cfg = Config::default();
        let s = snap("instagram.com/direct/inbox/", "Instagram", "chrome");
        assert_eq!(matches(&cfg, &s).unwrap().id, "instagram");
    }

    #[test]
    fn work_is_left_alone() {
        let cfg = Config::default();
        assert!(matches(&cfg, &snap("github.com/greensock/GSAP", "GSAP", "chrome")).is_none());
        assert!(matches(&cfg, &snap("", "badkat - Visual Studio Code", "Code")).is_none());
    }

    #[test]
    fn never_list_wins() {
        let mut cfg = Config::default();
        cfg.never.push("standup".into());
        let s = snap("tiktok.com", "standup notes", "chrome");
        assert!(matches(&cfg, &s).is_none());
    }

    #[test]
    fn disabled_rules_are_skipped() {
        let mut cfg = Config::default();
        for r in cfg.rules.iter_mut() {
            if r.id == "tiktok" {
                r.enabled = false;
            }
        }
        assert!(matches(&cfg, &snap("tiktok.com/foryou", "TikTok", "chrome")).is_none());
    }
}
