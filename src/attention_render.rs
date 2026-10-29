//! Terminal rendering of attention: short lines an agent can act on.
use crate::{
    attention::{AttentionOutcome, AttentionResponse, Item},
    authoring::OWNED_PREFIX,
    jobs::Health,
    runs::{RunState, Timestamp},
    targets::Target,
    terminal_text::clean,
    time_text,
};
use std::fmt::Write;

/// Nothing at all when nothing needs attention, so session hooks stay silent.
pub fn render(response: &AttentionResponse, now: Timestamp) -> String {
    let mut lines = vec![];
    for host in &response.hosts {
        let prefix = if host.host == Target::LOCAL {
            String::new()
        } else {
            format!("{}: ", clean(&host.host))
        };
        match &host.outcome {
            AttentionOutcome::Failed { error } => {
                lines.push(format!("? {prefix}could not check: {}", error.message()))
            }
            AttentionOutcome::Items { items } => {
                for item in items {
                    lines.push(format!(
                        "{} {prefix}{}",
                        marker(item),
                        line(item, &host.host, now)
                    ));
                }
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("Background jobs need attention:\n");
    for line in lines {
        writeln!(out, "{line}").unwrap();
    }
    out
}

fn marker(item: &Item) -> &'static str {
    if item.health.needs_attention() {
        "!"
    } else {
        "~"
    }
}

fn line(item: &Item, host: &str, now: Timestamp) -> String {
    let label = clean(&item.label);
    let short = label.strip_prefix(OWNED_PREFIX).unwrap_or(&label);
    // Hints name the machine, so they work as written from here.
    let on = if host == Target::LOCAL {
        String::new()
    } else {
        format!(" --host {}", clean(host))
    };
    if let Some(failures) = &item.recovered_from {
        let count = failures.count;
        let runs = if failures.first.id == failures.last.id {
            format!("run {}", failures.first.id)
        } else {
            format!("runs {}–{}", failures.first.id, failures.last.id)
        };
        return format!(
            "{label} recovered after {count} failed run{} since last check ({runs}). See `rhythm history {short}{on}`.",
            if count == 1 { "" } else { "s" },
        );
    }
    let what = match (item.health, &item.last_run) {
        (Health::Invalid, _) => "has a definition launchd cannot load".to_owned(),
        (Health::Down, _) => "is a keep-alive job that is not running".to_owned(),
        // Recorded detail when it shows the failure; launchd's own exit otherwise.
        (_, Some(run)) if run.state.failed() => match &run.state {
            RunState::Finished { ended_at, outcome } => {
                format!("failed: {outcome}, {}", time_text::ago(*ended_at, now))
            }
            RunState::Lost => format!(
                "lost its run started {}",
                time_text::ago(run.started_at, now)
            ),
            RunState::Running => "is failing".to_owned(),
        },
        _ => match item.last_exit {
            Some(exit) => format!("failed: {exit}"),
            None => "is failing".to_owned(),
        },
    };
    let next = if item.last_run.as_ref().is_some_and(|r| r.state.failed()) {
        format!(" See `rhythm logs {short}{on}`.")
    } else if item.last_run.is_some() {
        format!(" See `rhythm history {short}{on}`.")
    } else if item.health == Health::Failing {
        if on.is_empty() {
            format!(" Record its runs with `rhythm adopt {label}`.")
        } else {
            format!(" Record its runs by running `rhythm adopt {label}` there.")
        }
    } else {
        String::new()
    };
    format!("{label} {what}.{next}")
}
