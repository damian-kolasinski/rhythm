use crate::discovery::ObservedState;
use std::collections::BTreeMap;

pub(super) fn parse(output: &str) -> Option<BTreeMap<String, ObservedState>> {
    let mut lines = output.lines();
    if lines.next()?.split_whitespace().collect::<Vec<_>>() != ["PID", "Status", "Label"] {
        return None;
    }
    let mut states = BTreeMap::new();
    for line in lines {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() != 3 {
            return None;
        }
        let code = columns[1].parse().ok()?;
        let state = if columns[0] == "-" {
            ObservedState::Idle {
                last_exit_code: code,
            }
        } else {
            let pid = columns[0].parse::<u32>().ok().filter(|pid| *pid > 0)?;
            ObservedState::Running {
                pid,
                previous_exit_code: code,
            }
        };
        if states.insert(columns[2].to_owned(), state).is_some() {
            return None;
        }
    }
    Some(states)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_running_idle_failure_and_signal() {
        let states =
            parse("PID\tStatus\tLabel\n52\t-9\tserver\n-\t1\tscan\n-\t0\twarmer\n").unwrap();
        assert_eq!(
            states["server"],
            ObservedState::Running {
                pid: 52,
                previous_exit_code: -9
            }
        );
        assert_eq!(states["scan"], ObservedState::Idle { last_exit_code: 1 });
        assert_eq!(states["warmer"], ObservedState::Idle { last_exit_code: 0 });
    }

    #[test]
    fn rejects_partial_or_unexpected_output_instead_of_reporting_not_loaded() {
        for s in [
            "",
            "permission denied",
            "PID Status Label\n- ? work",
            "PID Status Label\n0 0 work",
            "PID Status Label\n- 0 a\n- 0 a",
        ] {
            assert!(parse(s).is_none());
        }
    }
}
