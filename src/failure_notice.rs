//! Decide when a finished run is worth interrupting someone for: when a job
//! starts failing or recovers, when it opens new problems, and when its last
//! open problem is resolved; never for every repeated failure or for problems
//! reported again. One run posts at most one notice.
use crate::{
    authoring::OWNED_PREFIX,
    findings::{OpenSet, Title},
    runs::{Outcome, RunState, Verdict},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Failed(Outcome),
    /// The run opened problems: the first one it listed and how many more.
    Found {
        first: Title,
        more: usize,
    },
    Recovered,
    /// No problem is open any more.
    Resolved,
}
impl Notice {
    /// `previous` is the job's last run before `run` that said whether the
    /// job works, and `findings` the open set `run` stated, if it stated
    /// one. A run that says nothing, as a skip or a requested stop, is
    /// neither a failure nor a recovery. A failure comes first, then new
    /// problems, a recovery, and an empty open set.
    pub fn after(
        previous: Option<&RunState>,
        run: &RunState,
        findings: Option<&OpenSet>,
    ) -> Option<Self> {
        let previously_failed = previous.is_some_and(RunState::failed);
        let found = findings.and_then(|set| {
            let mut opened = set.opened();
            opened.next().map(|first| Self::Found {
                first: first.problem.title.clone(),
                more: opened.count(),
            })
        });
        let resolved = findings
            .filter(|set| set.open.is_empty() && set.resolved > 0)
            .map(|_| Self::Resolved);
        match (run, run.verdict()) {
            (RunState::Finished { outcome, .. }, Some(Verdict::Failed)) if !previously_failed => {
                Some(Self::Failed(*outcome))
            }
            _ if found.is_some() => found,
            (_, Some(Verdict::Succeeded)) if previously_failed => Some(Self::Recovered),
            _ => resolved,
        }
    }
    pub fn title(&self, label: &str) -> String {
        match self {
            Self::Failed(_) => format!("{label} failed"),
            Self::Found { more: 0, .. } => format!("{label} found a problem"),
            Self::Found { more, .. } => format!("{label} found {} problems", more + 1),
            Self::Recovered => format!("{label} recovered"),
            Self::Resolved => format!("{label} has no open problems"),
        }
    }
    pub fn message(&self, label: &str) -> String {
        let short = label.strip_prefix(OWNED_PREFIX).unwrap_or(label);
        match self {
            Self::Failed(outcome) => format!("{outcome}. See rhythm logs {short}"),
            Self::Found { first, more: 0 } => format!("{first}. See rhythm findings {short}"),
            Self::Found { first, more } => {
                format!("{first}, +{more} more. See rhythm findings {short}")
            }
            Self::Recovered => "The latest run succeeded.".into(),
            Self::Resolved => "Every problem it reported is resolved.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        findings::{FindingKey, OpenSet, Problem, Reported, Said},
        runs::{ResultError, RunReport, Timestamp},
    };

    fn ended(outcome: Outcome, skipped: bool) -> RunState {
        RunState::Finished {
            ended_at: Timestamp(0),
            outcome,
            report: RunReport {
                skipped,
                ..RunReport::default()
            },
        }
    }
    fn finished(code: i32) -> RunState {
        ended(Outcome::Code { code }, false)
    }

    #[test]
    fn notifies_on_transitions_only() {
        let failed = Outcome::Code { code: 1 };
        let ok = finished(0);
        assert_eq!(
            Notice::after(None, &finished(1), None),
            Some(Notice::Failed(failed))
        );
        assert_eq!(Notice::after(None, &ok, None), None);
        assert_eq!(
            Notice::after(Some(&finished(0)), &finished(1), None),
            Some(Notice::Failed(failed))
        );
        assert_eq!(Notice::after(Some(&finished(2)), &finished(1), None), None);
        assert_eq!(
            Notice::after(Some(&RunState::Lost), &finished(1), None),
            None
        );
        assert_eq!(
            Notice::after(Some(&finished(2)), &ok, None),
            Some(Notice::Recovered)
        );
        assert_eq!(
            Notice::after(Some(&RunState::Lost), &ok, None),
            Some(Notice::Recovered)
        );
        assert_eq!(Notice::after(Some(&finished(0)), &ok, None), None);
        let stopped = ended(Outcome::Stopped { signal: 15 }, false);
        assert_eq!(Notice::after(None, &stopped, None), None);
        assert_eq!(
            Notice::after(
                Some(&RunState::Running),
                &ended(Outcome::SpawnFailed, false),
                None
            ),
            Some(Notice::Failed(Outcome::SpawnFailed))
        );
    }

    #[test]
    fn a_skip_is_neither_a_failure_nor_a_recovery() {
        let skip = ended(Outcome::Code { code: 0 }, true);
        assert_eq!(Notice::after(Some(&finished(1)), &skip, None), None);
        assert_eq!(Notice::after(None, &skip, None), None);
    }

    /// A run that ended with `code` and stated `said`, after `previous`.
    fn reporting(code: i32, said: Said, previous: Option<&OpenSet>) -> (RunState, Option<OpenSet>) {
        (finished(code), OpenSet::after(previous, said, Timestamp(5)))
    }
    fn notice(previous: &RunState, (run, open): (RunState, Option<OpenSet>)) -> Option<Notice> {
        Notice::after(Some(previous), &run, open.as_ref())
    }
    fn listed(keys: &[&str]) -> Said {
        Said::Readable(Some(Reported {
            problems: keys
                .iter()
                .map(|k| Problem {
                    key: FindingKey::parse(k).unwrap(),
                    title: Title::from(format!("{k} broke")),
                    detail: None,
                    link: None,
                    handled_by: None,
                })
                .collect(),
            dropped: 0,
        }))
    }
    fn found(first: &str, more: usize) -> Option<Notice> {
        Some(Notice::Found {
            first: Title::from(first.to_owned()),
            more,
        })
    }

    #[test]
    fn notifies_when_problems_open_and_when_none_are_left() {
        let ok = finished(0);
        let open = OpenSet::after(None, listed(&["a", "b"]), Timestamp(1)).unwrap();
        assert_eq!(
            notice(&ok, reporting(0, listed(&["a", "b"]), None)),
            found("a broke", 1)
        );
        // Reported again: nothing new.
        let again = reporting(0, listed(&["b", "a"]), Some(&open));
        assert_eq!(notice(&ok, again), None);
        // One resolved while another stays open is not worth a notice.
        assert_eq!(notice(&ok, reporting(0, listed(&["a"]), Some(&open))), None);
        assert_eq!(
            notice(&ok, reporting(0, listed(&["a", "c"]), Some(&open))),
            found("c broke", 0)
        );
        assert_eq!(
            notice(&ok, reporting(0, listed(&[]), Some(&open))),
            Some(Notice::Resolved)
        );
        // Nothing was open, so nothing was resolved.
        assert_eq!(notice(&ok, reporting(0, listed(&[]), None)), None);
    }

    #[test]
    fn one_run_posts_one_notice() {
        let open = OpenSet::after(None, listed(&["a"]), Timestamp(1)).unwrap();
        // A failure wins over the problems the failed run found.
        assert_eq!(
            notice(&finished(0), reporting(1, listed(&["a", "b"]), Some(&open))),
            Some(Notice::Failed(Outcome::Code { code: 1 }))
        );
        // A run that keeps failing still tells about new problems.
        assert_eq!(
            notice(&finished(1), reporting(1, listed(&["a", "b"]), Some(&open))),
            found("b broke", 0)
        );
        // New problems win over a recovery, a recovery over an empty set.
        assert_eq!(
            notice(&finished(1), reporting(0, listed(&["a", "b"]), Some(&open))),
            found("b broke", 0)
        );
        assert_eq!(
            notice(&finished(1), reporting(0, listed(&[]), Some(&open))),
            Some(Notice::Recovered)
        );
        let unread = reporting(0, Said::Unreadable(ResultError::Invalid), Some(&open));
        assert_eq!(
            notice(&finished(0), unread),
            found("The job's result could not be read (invalid)", 0)
        );
    }

    #[test]
    fn names_the_problem_and_where_to_read_more() {
        let notice = found("example-module: 2 tests broken", 2).unwrap();
        assert_eq!(
            notice.title("dev.rhythm.check"),
            "dev.rhythm.check found 3 problems"
        );
        assert_eq!(
            notice.message("dev.rhythm.check"),
            "example-module: 2 tests broken, +2 more. See rhythm findings check"
        );
        let one = found("lint fails", 0).unwrap();
        assert_eq!(
            one.title("com.example.scan"),
            "com.example.scan found a problem"
        );
        assert_eq!(
            one.message("com.example.scan"),
            "lint fails. See rhythm findings com.example.scan"
        );
        assert_eq!(
            Notice::Resolved.title("com.example.scan"),
            "com.example.scan has no open problems"
        );
    }

    #[test]
    fn says_what_failed_and_where_to_look() {
        let notice = Notice::Failed(Outcome::Signal { signal: 9 });
        assert_eq!(
            notice.title("dev.rhythm.report"),
            "dev.rhythm.report failed"
        );
        assert_eq!(
            notice.message("dev.rhythm.report"),
            "signal 9. See rhythm logs report"
        );
        assert_eq!(
            notice.message("com.example.scan"),
            "signal 9. See rhythm logs com.example.scan"
        );
    }
}
