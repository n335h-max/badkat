use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::{parse_clock, Config, PatrolScheduleConfig, Weekday};

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub weekday: Weekday,
    pub minute: u16,
}

impl CivilTime {
    pub fn date(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub version: u8,
    pub local_date: String,
    pub used_seconds: u64,
}

impl DailyUsage {
    fn new(date: String) -> Self {
        Self {
            version: 1,
            local_date: date,
            used_seconds: 0,
        }
    }

    fn is_valid(&self) -> bool {
        let bytes = self.local_date.as_bytes();
        self.version == 1
            && bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsageState {
    Disabled,
    OutsideSchedule,
    Available,
    Exhausted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatus {
    pub state: UsageState,
    pub used_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowance_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_seconds: Option<u64>,
    pub local_date: String,
}

pub struct UsageDecision {
    pub enforcement_allowed: bool,
}

pub struct UsageTracker {
    record: DailyUsage,
    last_sample: Instant,
    last_flush: Instant,
    last_countable: bool,
    remainder: Duration,
    dirty: bool,
}

impl UsageTracker {
    pub fn new(record: DailyUsage, now: Instant) -> Self {
        Self {
            record,
            last_sample: now,
            last_flush: now,
            last_countable: false,
            remainder: Duration::ZERO,
            dirty: false,
        }
    }

    pub fn tick(
        &mut self,
        cfg: &Config,
        civil: CivilTime,
        now: Instant,
        matched: bool,
    ) -> UsageDecision {
        let mut elapsed = now.saturating_duration_since(self.last_sample);
        self.last_sample = now;
        let date = civil.date();
        if self.record.local_date != date {
            self.record = DailyUsage::new(date);
            self.remainder = Duration::ZERO;
            self.dirty = true;
            // The sample interval may straddle local midnight (or a
            // timezone change). Its exact split is unknown, so never
            // attribute the previous date's time to the new date.
            elapsed = Duration::ZERO;
        }

        let scheduled = schedule_active(&cfg.schedule, civil);
        let countable = cfg.daily_allowance.enabled && scheduled && matched;
        let limit = cfg.daily_allowance.minutes.saturating_mul(60);
        if countable && self.last_countable {
            self.remainder = self.remainder.saturating_add(elapsed);
            let whole = self.remainder.as_secs();
            if whole > 0 {
                self.record.used_seconds = self.record.used_seconds.saturating_add(whole);
                self.remainder -= Duration::from_secs(whole);
                self.dirty = true;
            }
        } else {
            self.remainder = Duration::ZERO;
        }
        self.last_countable = countable;

        let exhausted = cfg.daily_allowance.enabled && self.record.used_seconds >= limit;
        UsageDecision {
            enforcement_allowed: scheduled && (!cfg.daily_allowance.enabled || exhausted),
        }
    }

    pub fn status(&self, cfg: &Config, civil: CivilTime) -> UsageStatus {
        let scheduled = schedule_active(&cfg.schedule, civil);
        let allowance = cfg.daily_allowance.minutes.saturating_mul(60);
        let used_seconds = if self.record.local_date == civil.date() {
            self.record.used_seconds
        } else {
            0
        };
        let state = if !scheduled {
            UsageState::OutsideSchedule
        } else if !cfg.daily_allowance.enabled {
            UsageState::Disabled
        } else if used_seconds >= allowance {
            UsageState::Exhausted
        } else {
            UsageState::Available
        };
        UsageStatus {
            state,
            used_seconds,
            allowance_seconds: cfg.daily_allowance.enabled.then_some(allowance),
            remaining_seconds: cfg
                .daily_allowance
                .enabled
                .then_some(allowance.saturating_sub(used_seconds)),
            local_date: civil.date(),
        }
    }

    pub fn should_flush(&self, now: Instant) -> bool {
        self.dirty && now.saturating_duration_since(self.last_flush) >= FLUSH_INTERVAL
    }

    pub fn record(&self) -> &DailyUsage {
        &self.record
    }

    pub fn mark_flushed(&mut self, now: Instant) {
        self.dirty = false;
        self.last_flush = now;
    }
}

pub fn local_now() -> CivilTime {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let value = unsafe { GetLocalTime() };
    CivilTime {
        year: value.wYear,
        month: value.wMonth,
        day: value.wDay,
        weekday: match value.wDayOfWeek {
            0 => Weekday::Sun,
            1 => Weekday::Mon,
            2 => Weekday::Tue,
            3 => Weekday::Wed,
            4 => Weekday::Thu,
            5 => Weekday::Fri,
            _ => Weekday::Sat,
        },
        minute: value.wHour * 60 + value.wMinute,
    }
}

pub fn schedule_active(schedule: &PatrolScheduleConfig, now: CivilTime) -> bool {
    if !schedule.enabled {
        return true;
    }
    let (Some(start), Some(end)) = (parse_clock(&schedule.start), parse_clock(&schedule.end))
    else {
        return false;
    };
    let selected = |day| schedule.weekdays.contains(&day);
    if start == end {
        return selected(now.weekday);
    }
    if start < end {
        return selected(now.weekday) && now.minute >= start && now.minute < end;
    }
    if now.minute >= start {
        return selected(now.weekday);
    }
    now.minute < end && selected(previous_weekday(now.weekday))
}

fn previous_weekday(day: Weekday) -> Weekday {
    match day {
        Weekday::Mon => Weekday::Sun,
        Weekday::Tue => Weekday::Mon,
        Weekday::Wed => Weekday::Tue,
        Weekday::Thu => Weekday::Wed,
        Weekday::Fri => Weekday::Thu,
        Weekday::Sat => Weekday::Fri,
        Weekday::Sun => Weekday::Sat,
    }
}

pub fn usage_path(dir: &Path) -> PathBuf {
    dir.join("usage.json")
}

pub fn load(dir: &Path, today: String) -> (DailyUsage, Vec<String>) {
    match crate::storage::load_json::<DailyUsage>(&usage_path(dir)) {
        crate::storage::LoadJson::Primary(record) if record.is_valid() => (record, Vec::new()),
        crate::storage::LoadJson::Primary(_) => {
            let backup_path = usage_path(dir).with_extension("json.bak");
            match crate::storage::load_json::<DailyUsage>(&backup_path) {
                crate::storage::LoadJson::Primary(record) if record.is_valid() => (
                    record,
                    vec!["usage.json failed validation and was recovered from its backup".into()],
                ),
                _ => (
                    DailyUsage::new(today),
                    vec!["usage.json failed validation, using today's zero usage".into()],
                ),
            }
        }
        crate::storage::LoadJson::Backup(record) if record.is_valid() => (
            record,
            vec!["usage.json was recovered from its backup".into()],
        ),
        crate::storage::LoadJson::Backup(_) => (
            DailyUsage::new(today),
            vec!["recovered usage.json failed validation, using today's zero usage".into()],
        ),
        crate::storage::LoadJson::Missing => (DailyUsage::new(today), Vec::new()),
        crate::storage::LoadJson::Invalid { primary, backup } => {
            let backup = backup
                .map(|err| format!("; backup: {err}"))
                .unwrap_or_default();
            (
                DailyUsage::new(today),
                vec![format!(
                    "usage.json could not be read ({primary}{backup}), using today's zero usage"
                )],
            )
        }
    }
}

pub fn save(dir: &Path, record: &DailyUsage) -> std::io::Result<()> {
    crate::storage::save_json(&usage_path(dir), record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "badkat-usage-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn civil(weekday: Weekday, hour: u16, minute: u16, day: u16) -> CivilTime {
        CivilTime {
            year: 2026,
            month: 9,
            day,
            weekday,
            minute: hour * 60 + minute,
        }
    }

    #[test]
    fn overnight_schedule_uses_the_previous_selected_day() {
        let schedule = PatrolScheduleConfig {
            enabled: true,
            weekdays: vec![Weekday::Mon],
            start: "22:00".into(),
            end: "06:00".into(),
        };
        assert!(schedule_active(&schedule, civil(Weekday::Mon, 23, 0, 21)));
        assert!(schedule_active(&schedule, civil(Weekday::Tue, 2, 0, 22)));
        assert!(!schedule_active(&schedule, civil(Weekday::Tue, 23, 0, 22)));
    }

    #[test]
    fn equal_schedule_times_mean_all_day() {
        let schedule = PatrolScheduleConfig {
            enabled: true,
            weekdays: vec![Weekday::Wed],
            start: "09:00".into(),
            end: "09:00".into(),
        };
        assert!(schedule_active(&schedule, civil(Weekday::Wed, 2, 0, 23)));
        assert!(!schedule_active(&schedule, civil(Weekday::Thu, 2, 0, 24)));
    }

    #[test]
    fn allowance_crossing_starts_enforcement_at_the_boundary() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        cfg.daily_allowance.minutes = 10;
        let start = Instant::now();
        let record = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 599,
        };
        let mut tracker = UsageTracker::new(record, start);
        tracker.tick(&cfg, civil(Weekday::Sun, 12, 0, 20), start, true);
        let decision = tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 0, 20),
            start + Duration::from_secs(1),
            true,
        );
        assert!(decision.enforcement_allowed);
        assert_eq!(
            tracker.status(&cfg, civil(Weekday::Sun, 12, 0, 20)).state,
            UsageState::Exhausted
        );
    }

    #[test]
    fn date_change_resets_usage_without_negative_elapsed_time() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        let start = Instant::now();
        let record = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 500,
        };
        let mut tracker = UsageTracker::new(record, start);
        tracker.tick(&cfg, civil(Weekday::Mon, 0, 0, 21), start, true);
        let status = tracker.status(&cfg, civil(Weekday::Mon, 0, 0, 21));
        assert_eq!(status.used_seconds, 0);
        assert_eq!(status.local_date, "2026-09-21");
    }

    #[test]
    fn date_change_does_not_charge_the_previous_days_sample() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        let start = Instant::now();
        let record = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 500,
        };
        let mut tracker = UsageTracker::new(record, start);
        tracker.tick(
            &cfg,
            civil(Weekday::Mon, 0, 0, 21),
            start + Duration::from_secs(5),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 0);
    }

    #[test]
    fn clock_rollback_never_subtracts_usage() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        let start = Instant::now();
        let record = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 25,
        };
        let mut tracker = UsageTracker::new(record, start);
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 0, 20),
            start.checked_sub(Duration::from_secs(1)).unwrap(),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 25);
    }

    #[test]
    fn an_unmatched_gap_is_not_charged_to_the_next_match() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        let start = Instant::now();
        let record = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 0,
        };
        let mut tracker = UsageTracker::new(record, start);
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 0, 20),
            start + Duration::from_secs(300),
            false,
        );
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 5, 20),
            start + Duration::from_secs(301),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 0);
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 5, 20),
            start + Duration::from_secs(302),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 1);
    }

    #[test]
    fn entering_the_schedule_does_not_charge_the_outside_interval() {
        let mut cfg = Config::default();
        cfg.daily_allowance.enabled = true;
        cfg.schedule.enabled = true;
        cfg.schedule.weekdays = vec![Weekday::Sun];
        cfg.schedule.start = "12:00".into();
        cfg.schedule.end = "13:00".into();
        let start = Instant::now();
        let mut tracker = UsageTracker::new(DailyUsage::new("2026-09-20".into()), start);
        tracker.tick(&cfg, civil(Weekday::Sun, 11, 59, 20), start, true);
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 0, 20),
            start + Duration::from_secs(60),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 0);
        tracker.tick(
            &cfg,
            civil(Weekday::Sun, 12, 0, 20),
            start + Duration::from_secs(61),
            true,
        );
        assert_eq!(tracker.record().used_seconds, 1);
    }

    #[test]
    fn usage_file_contains_only_aggregate_fields() {
        let value = DailyUsage {
            version: 1,
            local_date: "2026-09-20".into(),
            used_seconds: 42,
        };
        let json = serde_json::to_value(value).unwrap();
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["localDate", "usedSeconds", "version"]);
    }

    #[test]
    fn invalid_primary_recovers_usage_from_backup() {
        let dir = temp_dir();
        let path = usage_path(&dir);
        fs::write(&path, "invalid").unwrap();
        fs::write(
            path.with_extension("json.bak"),
            r#"{"version":1,"localDate":"2026-09-20","usedSeconds":42}"#,
        )
        .unwrap();

        let (loaded, notes) = load(&dir, "2026-09-20".into());
        assert_eq!(loaded.used_seconds, 42);
        assert!(notes
            .iter()
            .any(|note| note.contains("recovered from its backup")));
        fs::remove_dir_all(dir).unwrap();
    }
}
