//! The affection system: what the cat has earned from you.
//!
//! Kept in its own file — `progress.json`, not `config.json` — because
//! this is earned state, not a setting. The settings window writes the
//! whole config back whenever you change a slider, and "Restore
//! defaults" wipes it; neither of those should ever cost the cat its
//! levels. Separate files make that impossible rather than merely
//! unlikely.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// XP for the two things you can do for the cat. Closing a window is
/// the whole point of the app, so it is worth a lot more than a pat —
/// and a pat is cheap enough to spam, which is what PAT_COOLDOWN is for.
pub const XP_CLOSE: u32 = 25;
pub const XP_PAT: u32 = 5;

/// The first level costs this much; every level after costs more.
const BASE_COST: f64 = 50.0;
const GROWTH: f64 = 1.28;

/// What it costs to get from `level` to `level + 1`. Grows geometrically
/// so early levels come quickly and later ones are a real haul, rounded
/// to a multiple of 5 so the number on the dashboard reads as a target
/// rather than as noise.
pub fn cost_of(level: u32) -> u32 {
    let raw = BASE_COST * GROWTH.powi(level.saturating_sub(1) as i32);
    let rounded = (raw / 5.0).round() * 5.0;
    (rounded as u32).max(5)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub level: u32,
    /// XP banked toward the NEXT level, never the lifetime total
    pub xp: u32,
    pub total_xp: u64,
    pub closes: u32,
    pub pats: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            level: 1,
            xp: 0,
            total_xp: 0,
            closes: 0,
            pats: 0,
        }
    }
}

impl Progress {
    pub fn needed(&self) -> u32 {
        cost_of(self.level)
    }

    /// Bank `amount` and return how many levels that crossed. Loops
    /// rather than subtracting once, because a single close can carry a
    /// low level over more than one threshold.
    pub fn award(&mut self, amount: u32) -> u32 {
        self.xp = self.xp.saturating_add(amount);
        self.total_xp = self.total_xp.saturating_add(amount as u64);
        let mut gained = 0;
        while self.xp >= self.needed() {
            self.xp -= self.needed();
            self.level += 1;
            gained += 1;
            // a runaway award must not spin here forever
            if gained > 50 {
                break;
            }
        }
        gained
    }
}

pub fn progress_path(dir: &Path) -> PathBuf {
    dir.join("progress.json")
}

/// A missing or unreadable file is a brand new cat, not an error worth
/// stopping for — the app still has to start.
pub fn load(dir: &Path) -> (Progress, Vec<String>) {
    match crate::storage::load_json(&progress_path(dir)) {
        crate::storage::LoadJson::Primary(progress) => (progress, Vec::new()),
        crate::storage::LoadJson::Backup(progress) => (
            progress,
            vec!["progress.json was recovered from its backup".into()],
        ),
        crate::storage::LoadJson::Missing => (Progress::default(), Vec::new()),
        crate::storage::LoadJson::Invalid { primary, backup } => {
            let backup = backup
                .map(|err| format!("; backup: {err}"))
                .unwrap_or_default();
            (
                Progress::default(),
                vec![format!(
                    "progress.json could not be read ({primary}{backup}), using defaults"
                )],
            )
        }
    }
}

pub fn save(dir: &Path, p: &Progress) -> std::io::Result<()> {
    crate::storage::save_json(&progress_path(dir), p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "badkat-progress-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn each_level_costs_more_than_the_last() {
        for l in 1..40 {
            assert!(
                cost_of(l + 1) > cost_of(l),
                "level {} cost {} did not exceed level {} cost {}",
                l + 1,
                cost_of(l + 1),
                l,
                cost_of(l)
            );
        }
    }

    #[test]
    fn a_pat_does_not_level_a_fresh_cat() {
        let mut p = Progress::default();
        assert_eq!(p.award(XP_PAT), 0);
        assert_eq!(p.level, 1);
        assert_eq!(p.xp, XP_PAT);
    }

    #[test]
    fn crossing_a_threshold_carries_the_remainder_over() {
        let mut p = Progress::default();
        let need = p.needed();
        assert_eq!(p.award(need + 7), 1);
        assert_eq!(p.level, 2);
        assert_eq!(p.xp, 7);
    }

    #[test]
    fn one_huge_award_can_cross_several_levels() {
        let mut p = Progress::default();
        let gained = p.award(10_000);
        assert!(gained > 1, "expected multiple levels, got {gained}");
        assert!(p.xp < p.needed());
    }

    #[test]
    fn totals_track_every_award_not_just_the_banked_remainder() {
        let mut p = Progress::default();
        p.award(30);
        p.award(30);
        assert_eq!(p.total_xp, 60);
    }

    #[test]
    fn invalid_primary_recovers_progress_from_backup_with_note() {
        let dir = temp_dir();
        let path = progress_path(&dir);
        let mut expected = Progress::default();
        expected.award(30);
        fs::write(&path, "invalid").unwrap();
        fs::write(
            path.with_extension("json.bak"),
            serde_json::to_string_pretty(&expected).unwrap(),
        )
        .unwrap();

        let (loaded, notes) = load(&dir);
        assert_eq!(loaded.total_xp, 30);
        assert!(notes
            .iter()
            .any(|note| note.contains("recovered from its backup")));
        fs::remove_dir_all(dir).unwrap();
    }
}
