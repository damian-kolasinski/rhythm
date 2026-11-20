//! Problems a job reports as open right now, and how that open set changes
//! from one run to the next. A job states the complete set in each result it
//! writes; Rhythm never keeps a table of its own, only what each run stated.
use crate::{
    runs::{ResultError, Timestamp},
    terminal_text::clean,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Findings kept from one result; the rest are counted as dropped.
pub const MAX_FINDINGS: usize = 100;
/// Keys Rhythm reports itself; a job's own keys may not start with it.
pub const RESERVED_PREFIX: &str = "rhythm:";
const RESULT_KEY: &str = "rhythm:result";

/// Identifies one problem across runs: printable ASCII, 1 to 200 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FindingKey(String);
impl FindingKey {
    pub const MAX_BYTES: usize = 200;
    pub fn parse(text: &str) -> Option<Self> {
        let valid = (1..=Self::MAX_BYTES).contains(&text.len())
            && text.bytes().all(|b| (b' '..=b'~').contains(&b));
        valid.then(|| Self(text.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn reserved(&self) -> bool {
        self.0.starts_with(RESERVED_PREFIX)
    }
}
impl TryFrom<String> for FindingKey {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or("a finding key is 1 to 200 bytes of printable ASCII")
    }
}
impl From<FindingKey> for String {
    fn from(key: FindingKey) -> Self {
        key.0
    }
}

/// Text safe to print in a terminal, at most `MAX` characters. `LINES`
/// keeps line breaks, which longer text may need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct Text<const MAX: usize, const LINES: bool>(String);
pub type Title = Text<200, false>;
pub type Detail = Text<2000, true>;
/// A link or who handles a problem.
pub type Note = Text<500, false>;

impl<const MAX: usize, const LINES: bool> Text<MAX, LINES> {
    /// As a job wrote it: `Ok(None)` when nothing is left after cleaning,
    /// an error when it is longer than `MAX`.
    fn exact(text: &str) -> Result<Option<Self>, Invalid> {
        let text = Self::cleaned(text);
        match text.chars().count() {
            0 => Ok(None),
            n if n <= MAX => Ok(Some(Self(text))),
            _ => Err(Invalid),
        }
    }
    fn cleaned(text: &str) -> String {
        let text = text.trim();
        if LINES {
            text.lines()
                .map(|line| clean(line.trim_end()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            clean(text)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Text read back from storage or another machine is cleaned and shortened
/// the same way, so nothing unsafe reaches a terminal.
impl<const MAX: usize, const LINES: bool> From<String> for Text<MAX, LINES> {
    fn from(text: String) -> Self {
        let text = Self::cleaned(&text);
        if text.chars().count() <= MAX {
            return Self(text);
        }
        let mut short: String = text.chars().take(MAX - 1).collect();
        short.push('…');
        Self(short)
    }
}
impl<const MAX: usize, const LINES: bool> From<Text<MAX, LINES>> for String {
    fn from(text: Text<MAX, LINES>) -> Self {
        text.0
    }
}
impl<const MAX: usize, const LINES: bool> std::fmt::Display for Text<MAX, LINES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One problem as a job describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub key: FindingKey,
    pub title: Title,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Note>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handled_by: Option<Note>,
}

/// An open problem and since when it has been open without a break.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    #[serde(flatten)]
    pub problem: Problem,
    pub first_seen: Timestamp,
}

/// The `findings` a job's result listed: the complete set open right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reported {
    pub problems: Vec<Problem>,
    /// Problems beyond [`MAX_FINDINGS`], left out.
    pub dropped: u32,
}

/// A `findings` array that breaks the contract, which makes the whole result
/// invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid;

/// Reads the `findings` array of a version 1 result. Any entry that breaks
/// the contract makes the whole result invalid.
pub fn parse(value: &serde_json::Value) -> Result<Reported, Invalid> {
    #[derive(Deserialize)]
    struct Entry {
        key: String,
        title: String,
        #[serde(default)]
        detail: Option<String>,
        #[serde(default)]
        link: Option<String>,
        #[serde(default)]
        handled_by: Option<String>,
    }
    let entries = Vec::<Entry>::deserialize(value).map_err(|_| Invalid)?;
    let mut keys = BTreeSet::new();
    let mut problems = Vec::with_capacity(entries.len().min(MAX_FINDINGS));
    for entry in &entries {
        let key = FindingKey::parse(&entry.key).ok_or(Invalid)?;
        if key.reserved() || !keys.insert(key.clone()) {
            return Err(Invalid);
        }
        problems.push(Problem {
            key,
            title: Title::exact(&entry.title)?.ok_or(Invalid)?,
            detail: optional(&entry.detail)?,
            link: optional(&entry.link)?,
            handled_by: optional(&entry.handled_by)?,
        });
    }
    let dropped = problems.len().saturating_sub(MAX_FINDINGS) as u32;
    problems.truncate(MAX_FINDINGS);
    Ok(Reported { problems, dropped })
}

fn optional<const MAX: usize, const LINES: bool>(
    text: &Option<String>,
) -> Result<Option<Text<MAX, LINES>>, Invalid> {
    text.as_deref().map_or(Ok(None), Text::exact)
}

/// What a finished run's result file said about findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// No result file.
    Nothing,
    /// A readable result, with its `findings` when it listed them.
    Readable(Option<Reported>),
    Unreadable(ResultError),
}

/// The open set as of one run, which is how each run that says anything
/// about findings stores them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSet {
    pub open: Vec<Finding>,
    /// Keys open now that were not open before this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new: Vec<FindingKey>,
    /// How many problems open before this run are no longer open.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub resolved: u32,
    /// Problems the job listed beyond [`MAX_FINDINGS`].
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u32,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
impl OpenSet {
    /// The open set after a run that said `said` at `at`, given the open set
    /// after the last run before it that stated one. `None` when the run
    /// leaves the open set as it was.
    ///
    /// A result Rhythm could not read keeps the job's own findings and adds
    /// one of Rhythm's, which the next readable result resolves.
    pub fn after(previous: Option<&OpenSet>, said: Said, at: Timestamp) -> Option<Self> {
        let earlier = previous.map_or(&[][..], |p| &p.open[..]);
        let carried = || {
            earlier
                .iter()
                .filter(|f| f.problem.key.as_str() != RESULT_KEY)
                .map(|f| f.problem.clone())
        };
        let (problems, dropped): (Vec<Problem>, u32) = match said {
            Said::Nothing => return None,
            Said::Readable(Some(reported)) => (reported.problems, reported.dropped),
            Said::Readable(None)
                if earlier.iter().any(|f| f.problem.key.as_str() == RESULT_KEY) =>
            {
                (carried().collect(), previous.map_or(0, |p| p.dropped))
            }
            Said::Readable(None) => return None,
            Said::Unreadable(error) => (
                std::iter::once(unreadable(error))
                    .chain(carried())
                    .collect(),
                previous.map_or(0, |p| p.dropped),
            ),
        };
        let open: Vec<Finding> = problems
            .into_iter()
            .map(|problem| Finding {
                first_seen: earlier
                    .iter()
                    .find(|f| f.problem.key == problem.key)
                    .map_or(at, |f| f.first_seen),
                problem,
            })
            .collect();
        let change = Change::between(earlier, &open);
        Some(Self {
            new: change.new.iter().map(|f| f.problem.key.clone()).collect(),
            resolved: change.resolved.len() as u32,
            dropped,
            open,
        })
    }
    /// The findings this run opened, in the order the job listed them.
    pub fn opened(&self) -> impl Iterator<Item = &Finding> {
        self.open
            .iter()
            .filter(|f| self.new.contains(&f.problem.key))
    }
}

/// What a run's record says about the open set it stated: how many problems
/// are open, which opened, and how many were resolved. The problems
/// themselves stay with the job's newest statement; see `rhythm findings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFindings {
    pub open: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new: Vec<FindingKey>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub resolved: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u32,
}
impl From<&OpenSet> for RunFindings {
    fn from(set: &OpenSet) -> Self {
        Self {
            open: set.open.len(),
            new: set.new.clone(),
            resolved: set.resolved,
            dropped: set.dropped,
        }
    }
}

/// The open set as of the run `run`, which ended `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stated {
    pub run: i64,
    pub at: Timestamp,
    pub set: OpenSet,
}

/// Rhythm's own finding for a result it could not read.
fn unreadable(error: ResultError) -> Problem {
    Problem {
        key: FindingKey(RESULT_KEY.to_owned()),
        title: Title::from(format!(
            "The job's result could not be read ({})",
            error.code()
        )),
        detail: Some(Detail::from(
            "The file the job wrote to RHYTHM_RESULT does not follow the result contract. \
             See `rhythm skill show results`."
                .to_owned(),
        )),
        link: None,
        handled_by: None,
    }
}

/// A problem known only by its key and title, as one no longer open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub key: FindingKey,
    pub title: Title,
}
impl From<&Finding> for Resolved {
    fn from(finding: &Finding) -> Self {
        Self {
            key: finding.problem.key.clone(),
            title: finding.problem.title.clone(),
        }
    }
}

/// How one open set differs from an earlier one.
#[derive(Debug, PartialEq, Eq)]
pub struct Change<'a, R> {
    pub new: Vec<&'a Finding>,
    pub resolved: Vec<&'a R>,
}
impl<'a> Change<'a, Finding> {
    pub fn between(earlier: &'a [Finding], now: &'a [Finding]) -> Self {
        Change::since(earlier, |f| &f.problem.key, now)
    }
}
impl<'a, R> Change<'a, R> {
    /// `earlier` is anything that names the problems once open, by `key`.
    pub fn since(earlier: &'a [R], key: impl Fn(&R) -> &FindingKey, now: &'a [Finding]) -> Self {
        let was: BTreeSet<&FindingKey> = earlier.iter().map(&key).collect();
        let is: BTreeSet<&FindingKey> = now.iter().map(|f| &f.problem.key).collect();
        Self {
            new: now
                .iter()
                .filter(|f| !was.contains(&f.problem.key))
                .collect(),
            resolved: earlier.iter().filter(|r| !is.contains(key(r))).collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.resolved.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn problem(key: &str, title: &str) -> Problem {
        Problem {
            key: FindingKey::parse(key).unwrap(),
            title: Title::from(title.to_owned()),
            detail: None,
            link: None,
            handled_by: None,
        }
    }
    fn reported(keys: &[&str]) -> Said {
        Said::Readable(Some(Reported {
            problems: keys
                .iter()
                .map(|k| problem(k, &format!("{k} broke")))
                .collect(),
            dropped: 0,
        }))
    }
    fn keys(set: &OpenSet) -> Vec<&str> {
        set.open.iter().map(|f| f.problem.key.as_str()).collect()
    }
    fn first_seen(set: &OpenSet) -> Vec<i64> {
        set.open.iter().map(|f| f.first_seen.0).collect()
    }

    #[test]
    fn reads_the_documented_shape() {
        let value = json!([
            {"key": "regression/example-module@1a2b3c4d", "title": " example-module: 2 tests broken ",
             "detail": "line one\nline\ttwo\n", "link": "https://example.com/pull/123",
             "handled_by": "", "severity": "ignored"},
            {"key": "lint", "title": "lint\u{1b}[2J fails", "detail": null}
        ]);
        let reported = parse(&value).unwrap();
        assert_eq!(reported.dropped, 0);
        let first = &reported.problems[0];
        assert_eq!(first.key.as_str(), "regression/example-module@1a2b3c4d");
        assert_eq!(first.title.as_str(), "example-module: 2 tests broken");
        assert_eq!(
            first.detail.as_ref().unwrap().as_str(),
            "line one\nline?two"
        );
        assert_eq!(
            first.link.as_ref().unwrap().as_str(),
            "https://example.com/pull/123"
        );
        assert_eq!(first.handled_by, None);
        assert_eq!(reported.problems[1].title.as_str(), "lint?[2J fails");
        assert_eq!(parse(&json!([])), Ok(Reported::default()));
    }

    #[test]
    fn refuses_entries_that_break_the_contract() {
        let long = |n: usize| "é".repeat(n);
        for invalid in [
            json!({}),
            json!("a"),
            json!([{"title": "no key"}]),
            json!([{"key": "no title"}]),
            json!([{"key": "k", "title": " \n "}]),
            json!([{"key": "", "title": "t"}]),
            json!([{"key": "tab\there", "title": "t"}]),
            json!([{"key": "é", "title": "t"}]),
            json!([{"key": "k".repeat(201), "title": "t"}]),
            json!([{"key": "rhythm:result", "title": "t"}]),
            json!([{"key": "k", "title": "t"}, {"key": "k", "title": "u"}]),
            json!([{"key": "k", "title": long(201)}]),
            json!([{"key": "k", "title": "t", "detail": long(2001)}]),
            json!([{"key": "k", "title": "t", "link": long(501)}]),
            json!([{"key": "k", "title": "t", "handled_by": long(501)}]),
            json!([{"key": "k", "title": "t", "link": 3}]),
        ] {
            assert_eq!(parse(&invalid), Err(Invalid), "{invalid}");
        }
        let at_limits = json!([{"key": "k".repeat(200), "title": long(200), "detail": long(2000),
                                "link": long(500), "handled_by": long(500)}]);
        assert!(parse(&at_limits).is_ok());
    }

    #[test]
    fn keeps_the_first_hundred_and_counts_the_rest() {
        let many: Vec<_> = (0..103)
            .map(|i| json!({"key": format!("k{i}"), "title": "t"}))
            .collect();
        let reported = parse(&json!(many)).unwrap();
        assert_eq!((reported.problems.len(), reported.dropped), (100, 3));
        assert_eq!(reported.problems[99].key.as_str(), "k99");
    }

    #[test]
    fn only_a_findings_list_or_an_unreadable_result_changes_the_open_set() {
        let first = OpenSet::after(None, reported(&["a"]), Timestamp(10)).unwrap();
        assert_eq!(
            OpenSet::after(Some(&first), Said::Nothing, Timestamp(20)),
            None
        );
        assert_eq!(
            OpenSet::after(Some(&first), Said::Readable(None), Timestamp(20)),
            None
        );
        let cleared = OpenSet::after(Some(&first), reported(&[]), Timestamp(20)).unwrap();
        assert!(cleared.open.is_empty());
        assert_eq!((cleared.new.len(), cleared.resolved), (0, 1));
    }

    #[test]
    fn compares_with_the_previous_open_set_and_carries_first_seen() {
        let first = OpenSet::after(None, reported(&["a", "b"]), Timestamp(10)).unwrap();
        assert_eq!(first.new.len(), 2);
        assert_eq!(first_seen(&first), [10, 10]);
        let second = OpenSet::after(Some(&first), reported(&["c", "b"]), Timestamp(20)).unwrap();
        assert_eq!(keys(&second), ["c", "b"]);
        assert_eq!(first_seen(&second), [20, 10]);
        assert_eq!(second.new, [FindingKey::parse("c").unwrap()]);
        assert_eq!(second.resolved, 1);
        assert_eq!(
            second
                .opened()
                .map(|f| f.problem.title.as_str())
                .collect::<Vec<_>>(),
            ["c broke"]
        );
        // A problem that was resolved and comes back is open since its return.
        let third = OpenSet::after(Some(&second), reported(&["a", "b"]), Timestamp(30)).unwrap();
        assert_eq!(first_seen(&third), [30, 10]);
    }

    #[test]
    fn an_unreadable_result_is_a_finding_of_its_own_until_a_readable_one() {
        let first = OpenSet::after(None, reported(&["a"]), Timestamp(10)).unwrap();
        let broken = OpenSet::after(
            Some(&first),
            Said::Unreadable(ResultError::Invalid),
            Timestamp(20),
        )
        .unwrap();
        assert_eq!(keys(&broken), ["rhythm:result", "a"]);
        assert_eq!(first_seen(&broken), [20, 10]);
        assert_eq!(
            broken.open[0].problem.title.as_str(),
            "The job's result could not be read (invalid)"
        );
        assert_eq!((broken.new.len(), broken.resolved), (1, 0));
        let still = OpenSet::after(
            Some(&broken),
            Said::Unreadable(ResultError::TooLarge),
            Timestamp(30),
        )
        .unwrap();
        assert_eq!(first_seen(&still), [20, 10]);
        assert!(still.new.is_empty());
        assert!(
            still.open[0]
                .problem
                .title
                .as_str()
                .ends_with("(too_large)")
        );
        // Without a file the job said nothing, so the result is still unread.
        assert_eq!(
            OpenSet::after(Some(&still), Said::Nothing, Timestamp(40)),
            None
        );
        let fixed = OpenSet::after(Some(&still), Said::Readable(None), Timestamp(40)).unwrap();
        assert_eq!(keys(&fixed), ["a"]);
        assert_eq!(first_seen(&fixed), [10]);
        assert_eq!((fixed.new.len(), fixed.resolved), (0, 1));
        let alone = OpenSet::after(
            None,
            Said::Unreadable(ResultError::Unreadable),
            Timestamp(5),
        )
        .unwrap();
        assert_eq!(keys(&alone), ["rhythm:result"]);
    }

    #[test]
    fn findings_round_trip_and_text_from_elsewhere_is_cleaned() {
        let set = OpenSet::after(None, reported(&["a"]), Timestamp(1_790_000_000_000)).unwrap();
        let json = serde_json::to_value(&set).unwrap();
        assert_eq!(
            json,
            json!({"open": [{"key": "a", "title": "a broke", "first_seen": "2026-09-21T14:13:20.000Z"}],
                   "new": ["a"]})
        );
        assert_eq!(serde_json::from_value::<OpenSet>(json).unwrap(), set);
        let remote: Finding = serde_json::from_value(json!({
            "key": "a", "title": format!("x\u{202E}{}", "y".repeat(300)),
            "first_seen": "2026-09-21T14:13:20.000Z"
        }))
        .unwrap();
        assert!(remote.problem.title.as_str().starts_with("x?y"));
        assert_eq!(remote.problem.title.as_str().chars().count(), 200);
        assert!(
            serde_json::from_value::<Finding>(json!({
                "key": "bad\nkey", "title": "t", "first_seen": "2026-09-21T14:13:20.000Z"
            }))
            .is_err()
        );
    }

    #[test]
    fn changes_name_what_opened_and_what_closed() {
        let earlier = OpenSet::after(None, reported(&["a", "b"]), Timestamp(1)).unwrap();
        let now = OpenSet::after(Some(&earlier), reported(&["b", "c"]), Timestamp(2)).unwrap();
        let change = Change::between(&earlier.open, &now.open);
        assert_eq!(
            change
                .new
                .iter()
                .map(|f| f.problem.key.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert_eq!(
            change
                .resolved
                .iter()
                .map(|f| f.problem.key.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert!(Change::between(&now.open, &now.open).is_empty());
    }
}
