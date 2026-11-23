//! The script a job's command runs, which outlives the job when the job is
//! removed: either the program itself, when it is a text file starting with
//! `#!`, or the file an interpreter is given to run. Files the system or a
//! package manager owns, such as version-manager shims, are not the job's
//! script. When in doubt it names nothing rather than the wrong file.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Interpreters by executable name. A name in this list followed by a version,
/// such as `python3.13` or `ruby3`, is the same interpreter.
const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "python",
    "ruby",
    "perl",
    "node",
    "bun",
    "deno",
    "php",
    "osascript",
    "lua",
    "Rscript",
    "swift",
    "pwsh",
    "tclsh",
];

/// Interpreters that run a file named after a `run` subcommand.
const SUBCOMMANDS: &[&str] = &["bun", "deno"];

/// Where the system and package managers keep the programs they own.
const OWNED_ELSEWHERE: &[&str] = &["/usr/", "/bin/", "/sbin/", "/System/", "/opt/homebrew/"];

/// `env` options that take the next argument as their value.
const ENV_VALUES: &[&str] = &["-u", "-P", "-C"];

/// The script `program` runs, resolved against `working_directory` when
/// relative; `None` when it runs no script of the job's own, or the script is
/// not a regular file at an absolute path.
pub fn script(program: &[String], working_directory: Option<&Path>) -> Option<PathBuf> {
    let base = working_directory.unwrap_or(Path::new("/"));
    let (first, mut arguments) = program.split_first()?;
    let mut executable = first.as_str();
    if name(executable) == "env" {
        (executable, arguments) = after_env(arguments)?;
    }
    let candidate = if is_interpreter(name(executable)) {
        interpreted(name(executable), arguments)?
    } else if executable.contains('/') {
        executable
    } else {
        return None;
    };
    let path = base.join(candidate);
    let own = path.is_absolute()
        && !path.to_str().is_some_and(|p| {
            p.contains("/shims/") || OWNED_ELSEWHERE.iter().any(|dir| p.starts_with(dir))
        });
    let is_script = if candidate == executable {
        starts_with_shebang(&path)
    } else {
        is_file(&path)
    };
    (own && is_script).then_some(path)
}

/// `env [-i] [-u NAME] [NAME=VALUE]… PROGRAM …`: the program and its
/// arguments; `None` for forms that split or rewrite the command line.
fn after_env(arguments: &[String]) -> Option<(&str, &[String])> {
    let mut rest = arguments;
    loop {
        let (argument, tail) = rest.split_first()?;
        if ENV_VALUES.contains(&argument.as_str()) {
            rest = tail.get(1..)?;
        } else if argument.starts_with("-S") || argument == "--split-string" {
            return None;
        } else if argument.starts_with('-') || argument.contains('=') {
            rest = tail;
        } else {
            return Some((argument, tail));
        }
    }
}

/// The file an interpreter is given: its first argument that is not an
/// option, unless an option says the code comes from the command line.
fn interpreted<'a>(interpreter: &str, arguments: &'a [String]) -> Option<&'a str> {
    let mut positional = arguments.iter().filter(|argument| {
        let inline = argument.starts_with('-')
            && !argument.starts_with("--")
            && argument[1..].contains(['c', 'e', 'm']);
        !argument.starts_with('-') || inline
    });
    let mut first = positional.next()?;
    if SUBCOMMANDS.iter().any(|s| interpreter.starts_with(s)) && first == "run" {
        first = positional.next()?;
    }
    (!first.starts_with('-') && !arguments.iter().any(|a| a == "--eval" || a == "--command"))
        .then_some(first.as_str())
}

fn name(executable: &str) -> &str {
    executable.rsplit('/').next().unwrap_or(executable)
}

fn is_interpreter(name: &str) -> bool {
    INTERPRETERS.iter().any(|interpreter| {
        name.strip_prefix(interpreter)
            .is_some_and(|version| version.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
    })
}

fn is_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

fn starts_with_shebang(path: &Path) -> bool {
    let mut start = [0; 2];
    is_file(path)
        && fs::File::open(path)
            .and_then(|mut file| file.read_exact(&mut start))
            .is_ok()
        && &start == b"#!"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn finds_the_script_a_command_runs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = |name: &str, contents: &[u8]| {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        };
        let tool = file("bin/report", b"#!/bin/sh\necho sample\n");
        let backup = file("backup.py", b"print('sample')\n");
        let shim = file("versions/shims/python3", b"#!/usr/bin/env bash\n");
        let at = |parts: &[&str]| script(&command(parts), Some(root));
        let s = |path: &Path| path.to_str().unwrap().to_owned();

        assert_eq!(at(&[&s(&tool), "discover"]), Some(tool.clone()));
        assert_eq!(
            at(&["bin/report"]),
            Some(tool.clone()),
            "relative to the job"
        );
        assert_eq!(at(&["/usr/bin/python3", &s(&backup)]), Some(backup.clone()));
        assert_eq!(at(&["python3.13", "-u", "backup.py"]), Some(backup.clone()));
        assert_eq!(
            at(&[
                "/usr/bin/env",
                "-i",
                "-u",
                "HOME",
                "MODE=fast",
                "python3",
                "backup.py"
            ]),
            Some(backup.clone())
        );
        assert_eq!(at(&["/usr/bin/env", &s(&tool)]), Some(tool.clone()));
        assert_eq!(
            at(&["deno", "run", "--allow-net", "backup.py"]),
            Some(backup.clone())
        );
        assert_eq!(
            at(&[&s(&shim), "backup.py"]),
            Some(backup.clone()),
            "a shim named after its interpreter runs the file it is given"
        );
    }

    #[test]
    fn names_nothing_rather_than_the_wrong_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = |name: &str, contents: &[u8]| {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        };
        file("backup.py", b"print('sample')\n");
        file("data.txt", b"sample\n");
        let binary = file("bin/native", b"\xcf\xfa\xed\xfe");
        let launcher = file("tools/shims/cli", b"#!/bin/sh\n");
        let at = |parts: &[&str]| script(&command(parts), Some(root));
        let s = |path: &Path| path.to_str().unwrap().to_owned();

        assert_eq!(at(&[&s(&binary)]), None, "a native program is not a script");
        assert_eq!(at(&["/bin/sh", "-c", "backup.py"]), None, "inline code");
        assert_eq!(at(&["bash", "-lc", "backup.py"]), None, "inline code");
        assert_eq!(
            at(&["perl", "-ne", "print", "data.txt"]),
            None,
            "inline code"
        );
        assert_eq!(
            at(&["node", "--eval", "x", "backup.py"]),
            None,
            "inline code"
        );
        assert_eq!(at(&["python3", "-m", "backup.py"]), None, "a module");
        assert_eq!(
            at(&["sh", "missing.sh", "data.txt"]),
            None,
            "an argument is not the script"
        );
        assert_eq!(
            at(&["/usr/bin/rsync", "backup.py"]),
            None,
            "not an interpreter"
        );
        assert_eq!(at(&["/usr/bin/pythonic", "backup.py"]), None);
        assert_eq!(at(&["sh", "bin"]), None, "a directory is not a script");
        assert_eq!(at(&[&s(&launcher)]), None, "a package manager's shim");
        assert_eq!(at(&["/usr/bin/env", "-S", "python3 backup.py"]), None);
        assert_eq!(at(&["python3", "/usr/lib/sample.py"]), None, "the system's");
        assert_eq!(
            script(
                &command(&["python3", "backup.py"]),
                Some(Path::new("relative"))
            ),
            None,
            "only an absolute path is shown"
        );
        assert_eq!(script(&[], Some(root)), None);
    }
}
