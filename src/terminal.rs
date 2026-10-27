//! Human output escapes all external metadata to avoid terminal control injection.
use crate::discovery::{Inventory, ObservedState, Trigger, WorkKind};
use std::fmt::Write;

fn safe(s: &str) -> String {
    s.chars().flat_map(char::escape_default).collect()
}

pub fn render(inventory: &Inventory) -> String {
    let mut out = String::from(
        "External launchd registrations (read-only, current user)\n\nSTATE\tKIND\tLABEL\tTRIGGERS\n",
    );
    for r in &inventory.registrations {
        let state = match r.observed {
            ObservedState::Running { pid, .. } => format!("running:{pid}"),
            ObservedState::Idle { last_exit_code: 0 } => "idle".into(),
            ObservedState::Idle { last_exit_code } => format!("exit:{last_exit_code}"),
            ObservedState::NotObserved => "not-observed".into(),
            ObservedState::Unknown => "unknown".into(),
        };
        let kind = match r.kind {
            WorkKind::Service => "service",
            WorkKind::Schedule => "schedule",
            WorkKind::OnDemand => "on-demand",
        };
        let triggers = r
            .triggers
            .iter()
            .map(|t| match t {
                Trigger::AtLoad => "at-load".into(),
                Trigger::Interval { seconds } => format!("every {seconds}s"),
                Trigger::Calendar { components: c } => {
                    let component = |v: Option<u8>| v.map_or("*".into(), |v| format!("{v:02}"));
                    format!(
                        "calendar {}:{} month={} day={} weekday={}",
                        component(c.hour),
                        component(c.minute),
                        component(c.month),
                        component(c.day),
                        component(c.weekday)
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        write!(out, "{state}\t{kind}\t{}", safe(&r.label)).unwrap();
        if !triggers.is_empty() {
            write!(out, "\t{triggers}").unwrap();
        }
        out.push('\n');
    }
    writeln!(
        out,
        "\n{} registrations; snapshot {}.",
        inventory.registrations.len(),
        if inventory.complete {
            "complete"
        } else {
            "partial"
        }
    )
    .unwrap();
    for d in &inventory.diagnostics {
        writeln!(
            out,
            "Warning: {:?}{}",
            d.code,
            d.source
                .as_ref()
                .map(|p| format!(" ({})", safe(&p.to_string_lossy())))
                .unwrap_or_default()
        )
        .unwrap();
    }
    out.push_str("Scope excludes system agents/daemons, cron, app-managed schedules, and unregistered processes.\nObservation covers the caller's launchd context; not-observed may be loaded in another domain.\nIdle/exit status describes a launcher; downstream completion needs separate evidence.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::*;
    #[test]
    fn renders_empty_inventory_golden() {
        let inventory = Inventory {
            schema_version: 1,
            scope: "current_user_launch_agents",
            observation_scope: "calling_launchd_context",
            ownership: "external_read_only",
            complete: true,
            registrations: vec![],
            diagnostics: vec![],
        };
        assert_eq!(
            render(&inventory),
            include_str!("../tests/golden/empty.txt")
        );
    }
    #[test]
    fn escapes_terminal_controls() {
        assert_eq!(safe("job\u{1b}[2J\n"), "job\\u{1b}[2J\\n");
    }

    #[test]
    fn renders_populated_partial_inventory_golden() {
        let rows = [
            (
                "a-service",
                WorkKind::Service,
                ObservedState::Running {
                    pid: 42,
                    previous_exit_code: -9,
                },
                vec![Trigger::AtLoad],
            ),
            (
                "b-interval",
                WorkKind::Schedule,
                ObservedState::Idle { last_exit_code: 0 },
                vec![Trigger::Interval { seconds: 300 }],
            ),
            (
                "c-weekly",
                WorkKind::Schedule,
                ObservedState::Idle { last_exit_code: 1 },
                vec![Trigger::Calendar {
                    components: Calendar {
                        month: None,
                        day: None,
                        weekday: Some(6),
                        hour: Some(9),
                        minute: Some(0),
                    },
                }],
            ),
            (
                "d-unobserved",
                WorkKind::OnDemand,
                ObservedState::NotObserved,
                vec![],
            ),
            (
                "e-unknown\u{1b}[2J",
                WorkKind::OnDemand,
                ObservedState::Unknown,
                vec![],
            ),
        ];
        let inventory = Inventory {
            schema_version: 1,
            scope: "current_user_launch_agents",
            observation_scope: "calling_launchd_context",
            ownership: "external_read_only",
            complete: false,
            registrations: rows
                .into_iter()
                .map(|(label, kind, observed, triggers)| Registration {
                    label: label.into(),
                    source: "/tmp/sample.plist".into(),
                    kind,
                    observed,
                    triggers,
                    environment_variable_count: 0,
                    requirements: vec![],
                })
                .collect(),
            diagnostics: vec![
                Diagnostic {
                    source: Some("/tmp/broken\n.plist".into()),
                    code: DiagnosticCode::InvalidRegistration,
                },
                Diagnostic {
                    source: None,
                    code: DiagnosticCode::ObservationUnavailable,
                },
            ],
        };
        assert_eq!(
            render(&inventory),
            include_str!("../tests/golden/populated-partial.txt")
        );
    }
}
