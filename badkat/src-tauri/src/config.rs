//! Settings, their defaults, and how they reach disk.
//!
//! Everything the settings window can change lives in this one struct,
//! serialised straight to `config.json` in the app's config dir. Field
//! names are camelCase on the wire so the frontend can bind to them
//! without a translation layer.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnforcementMode {
    Close,
    Nag,
}

impl EnforcementMode {
    pub fn is_close(self) -> bool {
        self == Self::Close
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Nag => "nag",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    #[default]
    Tab,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyAllowanceConfig {
    pub enabled: bool,
    pub minutes: u64,
}

impl Default for DailyAllowanceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            minutes: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatrolScheduleConfig {
    pub enabled: bool,
    pub weekdays: Vec<Weekday>,
    pub start: String,
    pub end: String,
}

impl Default for PatrolScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            weekdays: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start: "09:00".into(),
            end: "17:00".into(),
        }
    }
}

impl RuleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tab => "tab",
            Self::Close => "close",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

fn issue(path: impl Into<String>, code: &str, message: &str) -> ValidationIssue {
    ValidationIssue {
        path: path.into(),
        code: code.into(),
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub label: String,
    /// every pattern must appear
    #[serde(default)]
    pub all: Vec<String>,
    /// at least one pattern must appear
    #[serde(default)]
    pub any: Vec<String>,
    /// seconds this rule must hold the foreground before the cat acts
    pub grace: u64,
    /// `tab` (Ctrl+W) or `close` (WM_CLOSE) on disk.
    #[serde(default)]
    pub action: RuleAction,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatSettings {
    /// px per design unit; the rig is drawn in a 200x150 unit box
    pub scale: f64,
    /// multiplier on the animation and the walk speed together
    pub speed: f64,
    /// wander along the screen edge when idle
    pub wander: bool,
    /// let the cat doze off between wanders
    pub sleepy: bool,
}

impl Default for CatSettings {
    fn default() -> Self {
        Self {
            scale: 1.25,
            speed: 1.0,
            wander: true,
            sleepy: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub enabled: bool,
    /// `close` acts on the window, `nag` only makes the cat cross.
    pub mode: EnforcementMode,
    pub countdown_seconds: u64,
    pub snooze_minutes: u64,
    pub poll_ms: u64,
    /// address-bar reads are the expensive part, so they get their own rate
    pub url_poll_ms: u64,
    /// substrings that are never acted on, whatever else matches
    pub never: Vec<String>,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub cat: CatSettings,
    #[serde(default)]
    pub daily_allowance: DailyAllowanceConfig,
    #[serde(default)]
    pub schedule: PatrolScheduleConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: EnforcementMode::Close,
            countdown_seconds: 3,
            snooze_minutes: 5,
            poll_ms: 1000,
            url_poll_ms: 1500,
            never: vec![
                "zoom meeting".into(),
                "microsoft teams".into(),
                "google meet".into(),
            ],
            rules: default_rules(),
            cat: CatSettings::default(),
            daily_allowance: DailyAllowanceConfig::default(),
            schedule: PatrolScheduleConfig::default(),
        }
    }
}

/// Order matters: the first match wins, so the specific rules
/// (shorts, reels) have to sit above the general ones (youtube,
/// instagram) or the long leash would swallow them.
pub fn default_rules() -> Vec<Rule> {
    let r = |id: &str, label: &str, any: &[&str], grace: u64| Rule {
        id: id.into(),
        label: label.into(),
        all: vec![],
        any: any.iter().map(|s| s.to_string()).collect(),
        grace,
        action: RuleAction::Tab,
        enabled: true,
    };

    vec![
        r(
            "youtube-shorts",
            "YouTube Shorts",
            &["youtube.com/shorts"],
            6,
        ),
        r(
            "instagram-reels",
            "Instagram Reels",
            &["instagram.com/reel"],
            6,
        ),
        r("tiktok", "TikTok", &["tiktok.com"], 6),
        r(
            "facebook-reels",
            "Facebook Reels",
            &["facebook.com/reel", "facebook.com/watch", "fb.watch"],
            10,
        ),
        r("snapchat", "Snapchat", &["snapchat.com"], 15),
        r(
            "streaming",
            "Streaming",
            &[
                "netflix.com",
                "netflix",
                "primevideo.com",
                "prime video",
                "hotstar.com",
                "hotstar",
                "disneyplus.com",
                "disney+",
                "crunchyroll",
                "hulu.com",
                "jiocinema",
                "sonyliv",
                "zee5",
                "aha.video",
                "mxplayer",
                "peacocktv.com",
                "tv.apple.com",
            ],
            20,
        ),
        r("instagram", "Instagram", &["instagram.com"], 45),
        r("reddit", "Reddit", &["reddit.com"], 90),
        // a long leash: plenty of YouTube is work
        r(
            "youtube-watch",
            "YouTube",
            &["youtube.com/watch", "- youtube"],
            240,
        ),
    ]
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Reads the config, repairing what it can rather than refusing to
/// start. Returns the config plus anything that had to be dropped, so
/// the settings window can say what happened.
pub fn load(dir: &Path) -> (Config, Vec<String>) {
    let path = config_path(dir);
    let mut notes: Vec<String> = Vec::new();

    let cfg = match crate::storage::load_json::<Config>(&path) {
        crate::storage::LoadJson::Primary(cfg) if cfg.validate().is_empty() => cfg,
        crate::storage::LoadJson::Primary(cfg) => {
            let reason = summarize_issues(&cfg.validate());
            let backup_path = path.with_extension("json.bak");
            match crate::storage::load_json::<Config>(&backup_path) {
                crate::storage::LoadJson::Primary(backup) if backup.validate().is_empty() => {
                    notes.push(format!(
                        "config.json failed validation ({reason}) and was recovered from its backup"
                    ));
                    backup
                }
                _ => {
                    notes.push(format!(
                        "config.json failed validation ({reason}), using defaults"
                    ));
                    return (Config::default(), notes);
                }
            }
        }
        crate::storage::LoadJson::Backup(cfg) => {
            let issues = cfg.validate();
            if !issues.is_empty() {
                notes.push(format!(
                    "recovered config.json failed validation ({}), using defaults",
                    summarize_issues(&issues)
                ));
                return (Config::default(), notes);
            }
            notes.push("config.json was recovered from its backup".into());
            cfg
        }
        crate::storage::LoadJson::Missing => {
            let cfg = Config::default();
            let _ = save(dir, &cfg);
            return (cfg, notes);
        }
        crate::storage::LoadJson::Invalid { primary, backup } => {
            let backup = backup
                .map(|err| format!("; backup: {err}"))
                .unwrap_or_default();
            notes.push(format!(
                "config.json could not be read ({primary}{backup}), using defaults"
            ));
            return (Config::default(), notes);
        }
    };

    (cfg, notes)
}

fn summarize_issues(issues: &[ValidationIssue]) -> String {
    issues
        .iter()
        .take(3)
        .map(|item| format!("{}: {}", item.path, item.message))
        .collect::<Vec<_>>()
        .join("; ")
}

impl Config {
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        check_range(
            &mut issues,
            "countdownSeconds",
            self.countdown_seconds,
            0,
            30,
        );
        check_range(&mut issues, "snoozeMinutes", self.snooze_minutes, 1, 240);
        check_range(&mut issues, "pollMs", self.poll_ms, 250, 10_000);
        check_range(&mut issues, "urlPollMs", self.url_poll_ms, 500, 10_000);
        check_float(&mut issues, "cat.scale", self.cat.scale, 0.7, 2.2);
        check_float(&mut issues, "cat.speed", self.cat.speed, 0.5, 2.0);
        check_range(
            &mut issues,
            "dailyAllowance.minutes",
            self.daily_allowance.minutes,
            0,
            1_440,
        );
        if self.schedule.enabled && self.schedule.weekdays.is_empty() {
            issues.push(issue(
                "schedule.weekdays",
                "required",
                "Select at least one scheduled weekday",
            ));
        }
        if parse_clock(&self.schedule.start).is_none() {
            issues.push(issue(
                "schedule.start",
                "invalid_time",
                "Start time must use HH:MM",
            ));
        }
        if parse_clock(&self.schedule.end).is_none() {
            issues.push(issue(
                "schedule.end",
                "invalid_time",
                "End time must use HH:MM",
            ));
        }

        let mut ids = HashSet::new();
        for (index, rule) in self.rules.iter().enumerate() {
            let base = format!("rules[{index}]");
            let id = rule.id.trim();
            if id.is_empty() || id.len() > 64 {
                issues.push(issue(
                    format!("{base}.id"),
                    "invalid_length",
                    "Rule ID must be 1 to 64 characters",
                ));
            } else {
                if id != rule.id {
                    issues.push(issue(
                        format!("{base}.id"),
                        "not_trimmed",
                        "Rule ID must not start or end with whitespace",
                    ));
                }
                if !ids.insert(id.to_ascii_lowercase()) {
                    issues.push(issue(
                        format!("{base}.id"),
                        "duplicate",
                        "Rule IDs must be unique",
                    ));
                }
            }

            let label = rule.label.trim();
            if label.is_empty() || label.len() > 120 {
                issues.push(issue(
                    format!("{base}.label"),
                    "invalid_length",
                    "Rule name must be 1 to 120 characters",
                ));
            } else if label != rule.label {
                issues.push(issue(
                    format!("{base}.label"),
                    "not_trimmed",
                    "Rule name must not start or end with whitespace",
                ));
            }
            validate_patterns(&mut issues, &base, "all", &rule.all);
            validate_patterns(&mut issues, &base, "any", &rule.any);
            if rule
                .all
                .iter()
                .chain(rule.any.iter())
                .all(|pattern| pattern.trim().is_empty())
            {
                issues.push(issue(
                    format!("{base}.any"),
                    "patterns_required",
                    "At least one non-empty pattern is required",
                ));
            }
            check_range(&mut issues, &format!("{base}.grace"), rule.grace, 0, 86_400);
        }
        if self.rules.is_empty() {
            issues.push(issue("rules", "required", "At least one rule is required"));
        }
        issues
    }
}

pub fn parse_clock(value: &str) -> Option<u16> {
    let (hour, minute) = value.split_once(':')?;
    if hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    let hour: u16 = hour.parse().ok()?;
    let minute: u16 = minute.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(hour * 60 + minute)
}

fn check_range(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    value: u64,
    minimum: u64,
    maximum: u64,
) {
    if value < minimum || value > maximum {
        issues.push(issue(
            path,
            "out_of_range",
            &format!("Value must be between {minimum} and {maximum}"),
        ));
    }
}

fn check_float(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
) {
    if !value.is_finite() || value < minimum || value > maximum {
        issues.push(issue(
            path,
            "out_of_range",
            &format!("Value must be between {minimum} and {maximum}"),
        ));
    }
}

fn validate_patterns(
    issues: &mut Vec<ValidationIssue>,
    base: &str,
    field: &str,
    patterns: &[String],
) {
    for (index, pattern) in patterns.iter().enumerate() {
        let trimmed = pattern.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            issues.push(issue(
                format!("{base}.{field}[{index}]"),
                "invalid_length",
                "Pattern must be 1 to 256 characters",
            ));
        } else if trimmed != pattern {
            issues.push(issue(
                format!("{base}.{field}[{index}]"),
                "not_trimmed",
                "Pattern must not start or end with whitespace",
            ));
        }
    }
}

pub fn save(dir: &Path, cfg: &Config) -> std::io::Result<()> {
    crate::storage::save_json(&config_path(dir), cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "badkat-config-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn version_0_1_1_json_remains_compatible() {
        let dir = temp_dir();
        let cfg = Config::default();
        fs::write(
            config_path(&dir),
            serde_json::to_string_pretty(&cfg).unwrap(),
        )
        .unwrap();

        let (loaded, notes) = load(&dir);
        assert_eq!(loaded.mode, cfg.mode);
        assert_eq!(loaded.rules, cfg.rules);
        assert!(notes.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_primary_recovers_config_from_backup_with_note() {
        let dir = temp_dir();
        let path = config_path(&dir);
        fs::write(&path, "invalid").unwrap();
        fs::write(
            path.with_extension("json.bak"),
            serde_json::to_string_pretty(&Config::default()).unwrap(),
        )
        .unwrap();

        let (loaded, notes) = load(&dir);
        assert!(loaded.enabled);
        assert!(notes
            .iter()
            .any(|note| note.contains("recovered from its backup")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typed_values_keep_the_legacy_json_shape() {
        let cfg = Config::default();
        let json = serde_json::to_value(&cfg).unwrap();

        assert_eq!(json["mode"], "close");
        assert_eq!(json["rules"][0]["action"], "tab");
        assert_eq!(cfg.mode, EnforcementMode::Close);
        assert_eq!(cfg.rules[0].action, RuleAction::Tab);
    }

    #[test]
    fn valid_configuration_has_no_validation_issues() {
        assert!(Config::default().validate().is_empty());
    }

    #[test]
    fn validation_reports_field_paths_for_invalid_values() {
        let mut cfg = Config::default();
        cfg.countdown_seconds = 31;
        cfg.snooze_minutes = 0;
        cfg.poll_ms = 249;
        cfg.url_poll_ms = 499;
        cfg.cat.scale = f64::NAN;
        cfg.cat.speed = 2.1;
        cfg.rules[0].id = " duplicate ".into();
        cfg.rules[0].label.clear();
        cfg.rules[0].all = vec!["  ".into()];
        cfg.rules[0].any.clear();
        cfg.rules[0].grace = 86_401;
        cfg.rules[1].id = "duplicate".into();

        let paths: Vec<_> = cfg.validate().into_iter().map(|issue| issue.path).collect();
        for expected in [
            "countdownSeconds",
            "snoozeMinutes",
            "pollMs",
            "urlPollMs",
            "cat.scale",
            "cat.speed",
            "rules[0].id",
            "rules[0].label",
            "rules[0].all[0]",
            "rules[0].grace",
            "rules[1].id",
        ] {
            assert!(
                paths.iter().any(|path| path == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn unknown_legacy_enum_value_uses_backup_recovery() {
        let dir = temp_dir();
        let path = config_path(&dir);
        fs::write(&path, r#"{"enabled":true,"mode":"destroy"}"#).unwrap();
        fs::write(
            path.with_extension("json.bak"),
            serde_json::to_string_pretty(&Config::default()).unwrap(),
        )
        .unwrap();

        let (loaded, notes) = load(&dir);
        assert_eq!(loaded.mode, EnforcementMode::Close);
        assert!(notes
            .iter()
            .any(|note| note.contains("recovered from its backup")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn semantically_invalid_primary_uses_valid_backup() {
        let dir = temp_dir();
        let path = config_path(&dir);
        let mut invalid = Config::default();
        invalid.poll_ms = 1;
        fs::write(&path, serde_json::to_string_pretty(&invalid).unwrap()).unwrap();
        fs::write(
            path.with_extension("json.bak"),
            serde_json::to_string_pretty(&Config::default()).unwrap(),
        )
        .unwrap();

        let (loaded, notes) = load(&dir);
        assert_eq!(loaded.poll_ms, 1000);
        assert!(notes.iter().any(|note| note.contains("failed validation")));
        fs::remove_dir_all(dir).unwrap();
    }
}
