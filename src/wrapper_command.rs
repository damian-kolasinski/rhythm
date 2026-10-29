//! The `rhythm run` command line a job definition uses to record its runs.
use crate::jobs::Label;
use std::path::PathBuf;

const RUN: &str = "run";
const DATA_DIR: &str = "--data-dir";
const FORWARD: &str = "--forward";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped {
    pub executable: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub label: Label,
    /// Copy the command's output to the recorder's own stdout and stderr too.
    pub forward: bool,
    pub program: Vec<String>,
}
impl Wrapped {
    /// Recognizes only the exact shape `arguments` produces.
    pub fn parse(arguments: &[String]) -> Option<Self> {
        let (executable, mut rest) = arguments.split_first()?;
        let executable = PathBuf::from(executable);
        if executable.file_name()? != "rhythm" {
            return None;
        }
        let mut data_dir = None;
        if let [flag, dir, tail @ ..] = rest
            && flag == DATA_DIR
        {
            // The recorder refuses a relative directory, so it would not record.
            let dir = PathBuf::from(dir);
            if !dir.is_absolute() {
                return None;
            }
            data_dir = Some(dir);
            rest = tail;
        }
        let [run, label, tail @ ..] = rest else {
            return None;
        };
        if run != RUN {
            return None;
        }
        let label = label.parse().ok()?;
        let (forward, tail) = match tail {
            [flag, tail @ ..] if flag == FORWARD => (true, tail),
            _ => (false, tail),
        };
        let [separator, program @ ..] = tail else {
            return None;
        };
        if separator != "--" || program.is_empty() {
            return None;
        }
        Some(Self {
            executable,
            data_dir,
            label,
            forward,
            program: program.to_vec(),
        })
    }
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![self.executable.to_string_lossy().into_owned()];
        if let Some(dir) = &self.data_dir {
            arguments.extend([DATA_DIR.into(), dir.to_string_lossy().into_owned()]);
        }
        arguments.extend([RUN.into(), self.label.to_string()]);
        if self.forward {
            arguments.push(FORWARD.into());
        }
        arguments.push("--".into());
        arguments.extend(self.program.iter().cloned());
        arguments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn round_trips_every_shape() {
        for forward in [false, true] {
            for data_dir in [None, Some(PathBuf::from("/private/state"))] {
                let wrapped = Wrapped {
                    executable: "/opt/tools/rhythm".into(),
                    data_dir: data_dir.clone(),
                    label: "com.example.scan".parse().unwrap(),
                    forward,
                    program: strings(&["/bin/sh", "-c", "exit 0", "--"]),
                };
                assert_eq!(Wrapped::parse(&wrapped.arguments()), Some(wrapped));
            }
        }
    }

    #[test]
    fn ignores_other_commands() {
        for arguments in [
            vec![],
            strings(&["/bin/sh", "run", "com.example.x", "--", "true"]),
            strings(&["/opt/rhythm", "status"]),
            strings(&["/opt/rhythm", "run", "com.example.x", "true"]),
            strings(&["/opt/rhythm", "run", "com.example.x", "--"]),
            strings(&["/opt/rhythm", "run", "bad label", "--", "true"]),
            strings(&["/opt/rhythm", "--data-dir", "/s", "status"]),
            strings(&[
                "/opt/rhythm",
                "--data-dir",
                "state",
                "run",
                "com.example.x",
                "--",
                "true",
            ]),
        ] {
            assert_eq!(Wrapped::parse(&arguments), None, "{arguments:?}");
        }
    }
}
