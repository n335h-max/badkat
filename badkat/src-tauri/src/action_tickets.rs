use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::config::{Config, EnforcementMode, Rule, RuleAction};
use crate::win::Snapshot;

const MAX_TICKETS: usize = 32;
const EXPIRY_GRACE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct AuthorizedAction {
    pub target: Snapshot,
    pub action: RuleAction,
    pub rule: Rule,
    pub url_required: bool,
}

#[derive(Debug, Clone)]
struct PendingAction {
    id: String,
    target: Snapshot,
    rule: Rule,
    action: RuleAction,
    url_required: bool,
    issued_at: Instant,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct ActionTickets {
    entries: VecDeque<PendingAction>,
}

impl ActionTickets {
    pub fn issue(
        &mut self,
        target: &Snapshot,
        rule: &Rule,
        action: RuleAction,
        countdown: Duration,
        now: Instant,
    ) -> Option<String> {
        self.prune(now);
        while self.entries.len() >= MAX_TICKETS {
            self.entries.pop_front();
        }

        let id = new_ticket_id()?;
        self.entries.push_back(PendingAction {
            id: id.clone(),
            target: target.clone(),
            rule: rule.clone(),
            action,
            url_required: crate::rules::requires_url(rule, target),
            issued_at: now,
            expires_at: now + countdown + EXPIRY_GRACE,
        });
        Some(id)
    }

    pub fn authorize(
        &mut self,
        id: &str,
        fresh: &Snapshot,
        cfg: &Config,
        now: Instant,
    ) -> Result<AuthorizedAction, &'static str> {
        self.prune(now);
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return Err("unknown or expired action");
        };
        // Consumption happens before validation. A failed attempt must
        // not leave a destructive capability available for replay.
        let ticket = self.entries.remove(index).expect("ticket index exists");

        if now < ticket.issued_at {
            return Err("invalid action time");
        }
        if cfg.mode != EnforcementMode::Close || !cfg.enabled {
            return Err("patrol is not allowed to act");
        }
        if fresh.hwnd != ticket.target.hwnd
            || fresh.pid != ticket.target.pid
            || !fresh.proc.eq_ignore_ascii_case(&ticket.target.proc)
        {
            return Err("window is no longer in front");
        }
        if ticket.url_required && fresh.url.trim().is_empty() {
            return Err("address bar could not be verified");
        }

        let Some(rule) = crate::rules::matches(cfg, fresh) else {
            return Err("target no longer matches");
        };
        if rule.id != ticket.rule.id || rule != &ticket.rule {
            return Err("rule changed during countdown");
        }

        Ok(AuthorizedAction {
            target: fresh.clone(),
            action: ticket.action,
            rule: ticket.rule,
            url_required: ticket.url_required,
        })
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|entry| entry.expires_at > now);
    }
}

fn new_ticket_id() -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::CoCreateGuid;
        if let Ok(guid) = unsafe { CoCreateGuid() } {
            return Some(format!("{guid:?}"));
        }
    }

    // This application only supports Windows. If its cryptographic GUID
    // source is unavailable, do not mint a weaker destructive capability.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hwnd: isize, pid: u32, url: &str, title: &str) -> Snapshot {
        Snapshot {
            hwnd,
            pid,
            proc: "chrome".into(),
            title: title.into(),
            url: url.into(),
        }
    }

    fn shorts_rule(cfg: &Config) -> Rule {
        cfg.rules
            .iter()
            .find(|rule| rule.id == "youtube-shorts")
            .unwrap()
            .clone()
    }

    #[test]
    fn valid_ticket_authorizes_the_stored_action_once() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();
        let mut tickets = ActionTickets::default();
        let id = tickets
            .issue(&target, &rule, RuleAction::Tab, Duration::from_secs(3), now)
            .unwrap();

        let authorized = tickets.authorize(&id, &target, &cfg, now).unwrap();
        assert_eq!(authorized.action, RuleAction::Tab);
        assert_eq!(authorized.target.hwnd, 7);
        assert!(tickets.authorize(&id, &target, &cfg, now).is_err());
    }

    #[test]
    fn changed_window_or_process_is_rejected() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();

        for fresh in [
            snap(8, 42, "youtube.com/shorts/abc", "A Short - YouTube"),
            snap(7, 99, "youtube.com/shorts/abc", "A Short - YouTube"),
            Snapshot {
                proc: "firefox".into(),
                ..target.clone()
            },
        ] {
            let mut tickets = ActionTickets::default();
            let id = tickets
                .issue(&target, &rule, RuleAction::Tab, Duration::ZERO, now)
                .unwrap();
            assert!(tickets.authorize(&id, &fresh, &cfg, now).is_err());
        }
    }

    #[test]
    fn changed_or_unreadable_url_is_rejected_for_url_rule() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();

        for url in ["youtube.com/watch?v=abc", ""] {
            let mut tickets = ActionTickets::default();
            let id = tickets
                .issue(&target, &rule, RuleAction::Tab, Duration::ZERO, now)
                .unwrap();
            let fresh = snap(7, 42, url, "A Short - YouTube");
            assert!(tickets.authorize(&id, &fresh, &cfg, now).is_err());
        }
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();
        let mut tickets = ActionTickets::default();
        let id = tickets
            .issue(&target, &rule, RuleAction::Tab, Duration::from_secs(3), now)
            .unwrap();

        assert!(tickets
            .authorize(&id, &target, &cfg, now + Duration::from_secs(19))
            .is_err());
    }

    #[test]
    fn edited_or_disabled_rule_is_rejected() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();

        let mut disabled = cfg.clone();
        disabled
            .rules
            .iter_mut()
            .find(|candidate| candidate.id == rule.id)
            .unwrap()
            .enabled = false;
        let mut edited = cfg.clone();
        edited
            .rules
            .iter_mut()
            .find(|candidate| candidate.id == rule.id)
            .unwrap()
            .label
            .push_str(" edited");

        for changed in [disabled, edited] {
            let mut tickets = ActionTickets::default();
            let id = tickets
                .issue(&target, &rule, RuleAction::Tab, Duration::ZERO, now)
                .unwrap();
            assert!(tickets.authorize(&id, &target, &changed, now).is_err());
        }
    }

    #[test]
    fn unknown_and_cleared_tickets_are_rejected() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();
        let mut tickets = ActionTickets::default();

        assert!(tickets.authorize("", &target, &cfg, now).is_err());
        let id = tickets
            .issue(&target, &rule, RuleAction::Tab, Duration::ZERO, now)
            .unwrap();
        tickets.clear();
        assert!(tickets.authorize(&id, &target, &cfg, now).is_err());
    }

    #[test]
    fn ticket_store_is_bounded_and_prunes_expired_entries() {
        let cfg = Config::default();
        let rule = shorts_rule(&cfg);
        let target = snap(7, 42, "youtube.com/shorts/abc", "A Short - YouTube");
        let now = Instant::now();
        let mut tickets = ActionTickets::default();

        for _ in 0..40 {
            let _ = tickets.issue(&target, &rule, RuleAction::Tab, Duration::ZERO, now);
        }
        assert_eq!(tickets.len(), MAX_TICKETS);

        let _ = tickets.issue(
            &target,
            &rule,
            RuleAction::Tab,
            Duration::ZERO,
            now + EXPIRY_GRACE + Duration::from_secs(1),
        );
        assert_eq!(tickets.len(), 1);
    }
}
