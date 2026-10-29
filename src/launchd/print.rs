//! Read the job-level fields of `launchctl print` output.
use crate::jobs::{Exit, Observation};

pub fn parse(output: &str) -> Observation {
    let (mut pid, mut runs, mut last_exit, mut reason) = (None, 0, None, None);
    // Only top-level fields: nested blocks repeat names such as `state`.
    for line in output.lines() {
        let Some(line) = line.strip_prefix('\t').filter(|l| !l.starts_with('\t')) else {
            continue;
        };
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        match key {
            "pid" => pid = value.parse().ok(),
            "runs" => runs = value.parse().unwrap_or(0),
            "last exit code" => {
                if let Ok(code) = value.parse() {
                    last_exit = Some(Exit::Code { code });
                }
            }
            // As in "Killed: 9".
            "last terminating signal" => {
                if let Some(signal) = value.rsplit(": ").next().and_then(|s| s.parse().ok()) {
                    last_exit = Some(Exit::Signal { signal });
                }
            }
            // Jetsam terminations have a reason instead of a code or signal.
            "last exit reason" if value.starts_with("JETSAM_") => {
                reason = Some(if value == "JETSAM_REASON_MEMORY_IDLE_EXIT" {
                    Exit::Reclaimed
                } else {
                    Exit::Jetsam
                });
            }
            _ => {}
        }
    }
    Observation::Loaded {
        pid,
        runs,
        last_exit: last_exit.or(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_failed_idle_job() {
        let output = "gui/501/com.example.job = {\n\tactive count = 0\n\tstate = not running\n\truns = 19\n\tlast exit code = 1\n\tevent triggers = {\n\t\tstate = active\n\t}\n}\n";
        assert_eq!(
            parse(output),
            Observation::Loaded {
                pid: None,
                runs: 19,
                last_exit: Some(Exit::Code { code: 1 })
            }
        );
    }

    #[test]
    fn reads_a_running_job_that_never_exited() {
        let output =
            "\tstate = running\n\truns = 1\n\tpid = 4242\n\tlast exit code = (never exited)\n";
        assert_eq!(
            parse(output),
            Observation::Loaded {
                pid: Some(4242),
                runs: 1,
                last_exit: None
            }
        );
    }

    #[test]
    fn reads_jetsam_reasons() {
        for (reason, exit) in [
            ("JETSAM_REASON_MEMORY_IDLE_EXIT", Exit::Reclaimed),
            ("JETSAM_REASON_MEMORY_PERPROCESSLIMIT", Exit::Jetsam),
        ] {
            let output =
                format!("\tstate = not running\n\truns = 5\n\tlast exit reason = {reason}\n");
            assert_eq!(
                parse(&output),
                Observation::Loaded {
                    pid: None,
                    runs: 5,
                    last_exit: Some(exit)
                }
            );
        }
    }

    #[test]
    fn reads_a_terminating_signal() {
        let output = "\tstate = running\n\truns = 2\n\tpid = 7\n\tlast terminating signal = Killed: 9\n\t\tpid = 99\n";
        assert_eq!(
            parse(output),
            Observation::Loaded {
                pid: Some(7),
                runs: 2,
                last_exit: Some(Exit::Signal { signal: 9 })
            }
        );
    }
}
