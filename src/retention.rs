//! Which of a job's recorded runs are kept. Skipped runs have their own small
//! allowance, so a job that mostly finds nothing to do keeps its real runs.
//! The newest run that stated the job's open findings is always kept, so
//! those findings never disappear just because the job stopped listing them.
use crate::runs::Timestamp;

/// Runs kept per job that were not skipped, with their output.
pub const KEEP: usize = 100;
/// Skipped runs kept per job, with their output.
pub const KEEP_SKIPPED: usize = 20;
/// Unfinished runs younger than this are never removed: they may still be running.
pub const UNFINISHED_GRACE_MS: i64 = 7 * 86_400_000;

/// What retention needs to know about a stored run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    pub id: i64,
    pub started_at: Timestamp,
    pub finished: bool,
    pub skipped: bool,
}

/// The runs to remove at `now`, from one job's runs listed newest first.
/// `stating` is the run whose statement of open findings is current.
pub fn expired(runs: &[Stored], now: Timestamp, stating: Option<i64>) -> Vec<i64> {
    let (mut real, mut skipped) = (0, 0);
    runs.iter()
        .filter(|run| {
            let (kept, allowance) = if run.skipped {
                (&mut skipped, KEEP_SKIPPED)
            } else {
                (&mut real, KEEP)
            };
            if *kept < allowance {
                *kept += 1;
                return false;
            }
            if Some(run.id) == stating {
                return false;
            }
            run.finished || run.started_at.0 < now.0 - UNFINISHED_GRACE_MS
        })
        .map(|run| run.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp(1_790_000_000_000);

    /// Runs numbered from `first`, the newest first, all finished.
    fn runs(first: i64, pattern: &[bool]) -> Vec<Stored> {
        pattern
            .iter()
            .enumerate()
            .map(|(i, &skipped)| Stored {
                id: first - i as i64,
                started_at: NOW,
                finished: true,
                skipped,
            })
            .collect()
    }

    #[test]
    fn keeps_the_newest_real_runs_and_the_newest_skips_separately() {
        // 30 skips, then 105 real runs, then 5 more skips; newest first.
        let mut pattern = vec![true; 30];
        pattern.extend([false; 105]);
        pattern.extend([true; 5]);
        let stored = runs(140, &pattern);
        let expired = expired(&stored, NOW, None);
        // Skips are 140–111 and 5–1, real runs 110–6.
        let skips_dropped: Vec<i64> = (111..=120).rev().collect();
        let real_dropped: Vec<i64> = (6..=10).rev().collect();
        let old_skips: Vec<i64> = (1..=5).rev().collect();
        assert_eq!(
            expired,
            [skips_dropped, real_dropped, old_skips].concat(),
            "the 10 oldest of 30 recent skips, 5 real runs over 100, and every older skip"
        );
    }

    #[test]
    fn a_job_that_never_skips_keeps_100_runs() {
        let stored = runs(103, &[false; 103]);
        assert_eq!(expired(&stored, NOW, None), [3, 2, 1]);
        assert!(expired(&runs(100, &[false; 100]), NOW, None).is_empty());
        assert!(expired(&runs(20, &[true; 20]), NOW, None).is_empty());
    }

    #[test]
    fn unfinished_runs_beyond_the_allowance_wait_out_their_grace() {
        let mut stored = runs(102, &[false; 102]);
        // The two oldest never finished: one may still run, one is long gone.
        stored[100].finished = false;
        stored[100].started_at = Timestamp(NOW.0 - UNFINISHED_GRACE_MS + 1);
        stored[101].finished = false;
        stored[101].started_at = Timestamp(NOW.0 - UNFINISHED_GRACE_MS - 1);
        assert_eq!(expired(&stored, NOW, None), [1]);
    }

    #[test]
    fn the_newest_run_that_stated_findings_outlives_the_allowance() {
        let stored = runs(103, &[false; 103]);
        assert_eq!(expired(&stored, NOW, Some(2)), [3, 1]);
        // It counts toward the allowance while it is within it.
        let stored = runs(101, &[false; 101]);
        assert_eq!(expired(&stored, NOW, Some(101)), [1]);
    }
}
