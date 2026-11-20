//! Terminal rendering of a job's open findings: each problem, how long it has
//! been open, who handles it, and where to read more.
use crate::{open_findings::FindingsResponse, runs::Timestamp, terminal_text::clean, time_text};
use std::fmt::Write;

pub fn render(response: &FindingsResponse, now: Timestamp) -> String {
    let label = clean(&response.label);
    let (Some(run), Some(at)) = (response.run, response.reported_at) else {
        return format!(
            "{label} has not reported any problems. Jobs list them in their result; see `rhythm skill show results`.\n"
        );
    };
    let source = format!("reported by run {run}, {}", time_text::ago(at, now));
    let count = response.findings.len();
    if count == 0 {
        return format!("{label} has no open problems ({source}).\n");
    }
    let mut out = format!("{label} has {} open ({source}):\n", problems(count));
    for finding in &response.findings {
        let problem = &finding.problem;
        writeln!(out, "\n! {}", problem.title).unwrap();
        let mut facts = vec![format!(
            "open for {}",
            time_text::duration(now.0 - finding.first_seen.0)
        )];
        if let Some(who) = &problem.handled_by {
            facts.push(format!("handled by {who}"));
        }
        writeln!(out, "  {}", facts.join(" · ")).unwrap();
        if let Some(link) = &problem.link {
            writeln!(out, "  {link}").unwrap();
        }
        if let Some(detail) = &problem.detail {
            for line in detail.as_str().lines() {
                match line {
                    "" => out.push('\n'),
                    line => writeln!(out, "  {line}").unwrap(),
                }
            }
        }
        writeln!(out, "  key {}", problem.key.as_str()).unwrap();
    }
    if response.dropped > 0 {
        writeln!(
            out,
            "\nThe job listed {} more beyond the first 100; they are not kept.",
            response.dropped
        )
        .unwrap();
    }
    out
}

/// `1 problem`, `2 problems`.
pub fn problems(count: usize) -> String {
    format!("{count} problem{}", if count == 1 { "" } else { "s" })
}
