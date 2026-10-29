#![cfg(unix)]
use rhythm::{
    authoring::{self, Added, Adoption, Caller, NewJob, Recorder},
    jobs::{Exit, Label, Schedule},
    kick,
    launchd::Launchd,
    ledger::{self, Ledger},
    operation_error::Error,
    private_storage::Root,
    runs::{Outcome, Timestamp},
    schedule_input::{self, ScheduleInput},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;

mod command_fixture;

/// A user launchd that remembers which labels are loaded, from which file, and
/// in which domain. Labels come from the definitions themselves.
const LAUNCHCTL: &str = r#"#!/bin/sh
cd "$(dirname "$0")"
printf '%s\n' "$*" >> calls
label_in() { tr -d '\n\t ' < "$1" | sed -n 's:.*<key>Label</key><string>\([^<]*\)</string>.*:\1:p'; }
case "$1" in
  print)
    case "$2" in
      gui/501) [ ! -f no-gui ] ;;
      user/501) true ;;
      */*/*)
        label="${2##*/}"; domain="${2%/*}"
        [ -f "loaded/$label" ] && [ "$(cat "loaded/$label")" = "$domain" ] || exit 113
        printf '\tpath = %s\n' "$(cat "paths/$label")"
        cat "observed/$label" 2>/dev/null; true ;;
      *) exit 64 ;;
    esac ;;
  bootstrap)
    if [ -f fail-bootstrap ]; then exit "$(cat fail-bootstrap)"; fi
    if [ -f fail-next-bootstrap ]; then rm fail-next-bootstrap; exit 1; fi
    label=$(label_in "$3")
    [ -n "$label" ] || exit 1
    [ -f "loaded/$label" ] && exit 37
    printf '%s' "$2" > "loaded/$label"
    printf '%s' "$3" > "paths/$label"
    printf '\truns = 0\n' > "observed/$label" ;;
  bootout)
    label="${2##*/}"
    [ -f "loaded/$label" ] || exit 113
    rm "loaded/$label" "observed/$label" ;;
  kickstart)
    label="${2##*/}"
    [ -f "loaded/$label" ] || exit 113
    if [ -f "kick-busy/$label" ]; then printf '\truns = 1\n\tpid = 9\n' > "observed/$label"; exit 37; fi
    cp "after-kick/$label" "observed/$label" ;;
  *) exit 64 ;;
esac
"#;

struct Fixture {
    temp: TempDir,
    launchd: Launchd,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        for dir in [
            "home/Library/LaunchAgents",
            "loaded",
            "paths",
            "observed",
            "after-kick",
            "kick-busy",
            "work",
        ] {
            fs::create_dir_all(temp.path().join(dir)).unwrap();
        }
        command_fixture::write(&temp.path().join("launchctl"), LAUNCHCTL);
        let launchd = Launchd::at_boundary(
            temp.path().join("home/Library/LaunchAgents"),
            temp.path().join("launchctl"),
            501,
        );
        Self { temp, launchd }
    }
    fn path(&self) -> &Path {
        self.temp.path()
    }
    fn recorder(&self) -> Recorder {
        Recorder {
            executable: "/opt/tools/rhythm".into(),
            data_dir: None,
            diagnostics: "/private/state/recorder.log".into(),
        }
    }
    fn add(
        &self,
        name: &str,
        schedule: Schedule,
        command: &[&str],
        replace: bool,
    ) -> Result<(Label, Added), Error> {
        let home = self.path().join("home");
        let work = self.path().join("work");
        authoring::add(
            &self.launchd,
            &self.recorder(),
            &Caller {
                home: &home,
                current_directory: &work,
                search_path: Some("relative/bin:/usr/bin:/bin"),
            },
            NewJob {
                name: name.into(),
                schedule,
                command: command.iter().map(|c| c.to_string()).collect(),
                working_directory: None,
                environment: vec!["SAMPLE_MODE=fast".into()],
                replace,
            },
        )
    }
    fn agent(&self, label: &str) -> PathBuf {
        self.path()
            .join(format!("home/Library/LaunchAgents/{label}.plist"))
    }
    fn loaded(&self, label: &str) -> Option<String> {
        fs::read_to_string(self.path().join("loaded").join(label)).ok()
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with("print"))
            .map(|l| l.replace(self.path().to_str().unwrap(), "$TMP"))
            .collect()
    }
    /// A hand-written job, loaded from `file` in the gui domain.
    fn foreign_at(&self, file: &Path, label: &str, extra: &str) {
        fs::write(
            file,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>/bin/sh</string><string>-c</string><string>echo sample</string></array><key>Nice</key><integer>5</integer>{extra}</dict></plist>"#
            ),
        )
        .unwrap();
        fs::write(self.path().join("loaded").join(label), "gui/501").unwrap();
        fs::write(
            self.path().join("paths").join(label),
            file.to_str().unwrap(),
        )
        .unwrap();
        fs::write(
            self.path().join("observed").join(label),
            "\truns = 1\n\tlast exit code = 0\n",
        )
        .unwrap();
    }
    fn foreign(&self, label: &str, extra: &str) {
        self.foreign_at(&self.agent(label), label, extra);
    }
    fn arguments_in(&self, file: &Path) -> Vec<String> {
        let plist = plist::Value::from_file(file).unwrap();
        plist.as_dictionary().unwrap()["ProgramArguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_string().unwrap().to_owned())
            .collect()
    }
    fn arguments(&self, label: &str) -> Vec<String> {
        self.arguments_in(&self.agent(label))
    }
}

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&path, actual).unwrap();
    }
    assert_eq!(actual, fs::read_to_string(&path).unwrap(), "{name}");
}

fn on_demand() -> Schedule {
    Schedule::Triggered {
        at_load: false,
        every_seconds: None,
        calendar: vec![],
    }
}

fn schedule(daily: &[&str], weekly: &[&str], every: Option<&str>) -> Schedule {
    let daily: Vec<String> = daily.iter().map(|s| s.to_string()).collect();
    let weekly: Vec<String> = weekly.iter().map(|s| s.to_string()).collect();
    schedule_input::schedule(&ScheduleInput {
        every,
        daily: &daily,
        weekly: &weekly,
        at_load: true,
        keep_alive: false,
    })
    .unwrap()
}

fn label(text: &str) -> Label {
    text.parse().unwrap()
}

#[test]
fn adds_a_recorded_job_with_a_predictable_environment() {
    let f = Fixture::new();
    let (label, added) = f
        .add(
            "report",
            schedule(&["08:07"], &["sat@09:00"], Some("2h")),
            &["sh", "-c", "echo sample"],
            false,
        )
        .unwrap();
    assert_eq!(
        (label.as_str(), added),
        ("dev.rhythm.report", Added::Created)
    );
    let written = fs::read_to_string(f.agent("dev.rhythm.report"))
        .unwrap()
        .replace(f.path().to_str().unwrap(), "$TMP");
    golden("plist-added.xml", &written);
    assert_eq!(f.loaded("dev.rhythm.report").as_deref(), Some("gui/501"));
    assert_eq!(
        f.calls(),
        ["bootstrap gui/501 $TMP/home/Library/LaunchAgents/dev.rhythm.report.plist"]
    );
    let mode = fs::metadata(f.agent("dev.rhythm.report"))
        .unwrap()
        .permissions();
    assert_eq!(mode.mode() & 0o777, 0o644);
}

#[test]
fn headless_machines_get_background_jobs() {
    let f = Fixture::new();
    fs::write(f.path().join("no-gui"), "").unwrap();
    f.add("sync", Schedule::KeepAlive, &["/usr/bin/true"], false)
        .unwrap();
    let written = fs::read_to_string(f.agent("dev.rhythm.sync")).unwrap();
    assert!(written.contains("<key>LimitLoadToSessionType</key>\n\t<string>Background</string>"));
    assert!(written.contains("<key>KeepAlive</key>\n\t<true/>"));
    assert_eq!(f.loaded("dev.rhythm.sync").as_deref(), Some("user/501"));
}

#[test]
fn refuses_unrunnable_commands_and_bad_input() {
    let f = Fixture::new();
    fs::write(f.path().join("work/not-executable"), "").unwrap();
    for command in [
        &["no-such-program"][..],
        &["/no/such/file"],
        &["not-executable"],
        &["./not-executable"],
        &[],
    ] {
        assert_eq!(
            f.add("x", on_demand(), command, false),
            Err(Error::InvalidRequest),
            "{command:?}"
        );
    }
    assert_eq!(
        f.add("Bad Name", on_demand(), &["/usr/bin/true"], false),
        Err(Error::InvalidRequest)
    );
    assert!(!f.path().join("calls").exists());
}

#[test]
fn commands_keep_their_symlinked_names() {
    // A mise shim is a symlink whose name selects the tool; resolving it would break it.
    let f = Fixture::new();
    std::os::unix::fs::symlink("/usr/bin/true", f.path().join("work/tool-shim")).unwrap();
    f.add("shim", on_demand(), &["./tool-shim"], false).unwrap();
    assert_eq!(
        f.arguments("dev.rhythm.shim").last().unwrap(),
        f.path().join("work/tool-shim").to_str().unwrap()
    );
}

#[test]
fn never_overwrites_a_file_that_is_not_the_jobs() {
    let f = Fixture::new();
    // Another job's definition sits where Rhythm's would go.
    f.foreign_at(&f.agent("dev.rhythm.report"), "com.example.other", "");
    let before = fs::read(f.agent("dev.rhythm.report")).unwrap();
    for replace in [false, true] {
        assert_eq!(
            f.add("report", on_demand(), &["/usr/bin/true"], replace),
            Err(Error::AlreadyExists)
        );
    }
    fs::write(f.agent("dev.rhythm.broken"), "{not a plist").unwrap();
    assert_eq!(
        f.add("broken", on_demand(), &["/usr/bin/true"], false),
        Err(Error::AlreadyExists)
    );
    assert_eq!(fs::read(f.agent("dev.rhythm.report")).unwrap(), before);
    assert_eq!(
        fs::read(f.agent("dev.rhythm.broken")).unwrap(),
        b"{not a plist"
    );
}

#[test]
fn replacing_is_explicit_and_limited_to_rhythm_jobs() {
    let f = Fixture::new();
    f.add("report", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    assert_eq!(
        f.add("report", on_demand(), &["/usr/bin/true"], false),
        Err(Error::AlreadyExists)
    );
    let (_, added) = f
        .add(
            "report",
            schedule(&["06:00"], &[], None),
            &["/usr/bin/true"],
            true,
        )
        .unwrap();
    assert_eq!(added, Added::Replaced);
    assert!(
        fs::read_to_string(f.agent("dev.rhythm.report"))
            .unwrap()
            .contains("<integer>6</integer>")
    );
    assert_eq!(f.loaded("dev.rhythm.report").as_deref(), Some("gui/501"));
    // Nothing to replace yet: a plain creation.
    let (_, added) = f
        .add("fresh", on_demand(), &["/usr/bin/true"], true)
        .unwrap();
    assert_eq!(added, Added::Created);

    // A hand-written job that happens to use the prefix is not Rhythm's to replace.
    f.foreign("dev.rhythm.manual", "");
    assert_eq!(
        f.add("manual", on_demand(), &["/usr/bin/true"], true),
        Err(Error::NotOwned)
    );
}

#[test]
fn a_refused_definition_leaves_nothing_behind() {
    let f = Fixture::new();
    fs::write(f.path().join("fail-bootstrap"), "1").unwrap();
    assert_eq!(
        f.add("report", on_demand(), &["/usr/bin/true"], false),
        Err(Error::Refused)
    );
    assert_eq!(
        fs::read_dir(f.path().join("home/Library/LaunchAgents"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_refused_replacement_restores_and_reloads_the_previous_job() {
    let f = Fixture::new();
    f.add(
        "report",
        schedule(&["08:00"], &[], None),
        &["/usr/bin/true"],
        false,
    )
    .unwrap();
    let before = fs::read(f.agent("dev.rhythm.report")).unwrap();
    fs::write(f.path().join("fail-next-bootstrap"), "").unwrap();
    let result = f.add(
        "report",
        schedule(&["09:00"], &[], None),
        &["/usr/bin/true"],
        true,
    );
    assert_eq!(result, Err(Error::Refused));
    assert_eq!(fs::read(f.agent("dev.rhythm.report")).unwrap(), before);
    assert_eq!(f.loaded("dev.rhythm.report").as_deref(), Some("gui/501"));

    // When the previous job cannot be reloaded either, say so.
    fs::write(f.path().join("fail-bootstrap"), "1").unwrap();
    let result = f.add(
        "report",
        schedule(&["10:00"], &[], None),
        &["/usr/bin/true"],
        true,
    );
    assert_eq!(result, Err(Error::RollbackIncomplete));
    assert_eq!(fs::read(f.agent("dev.rhythm.report")).unwrap(), before);
}

#[test]
fn adoption_wraps_only_the_command_and_unadoption_restores_it() {
    let f = Fixture::new();
    let scan = label("com.example.scan");
    f.foreign(
        "com.example.scan",
        "<key>StandardOutPath</key><string>/tmp/scan.log</string>",
    );
    fs::set_permissions(
        f.agent("com.example.scan"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let original = f.arguments("com.example.scan");
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &scan, false),
        Ok(Adoption::Adopted)
    );
    assert_eq!(
        f.arguments("com.example.scan"),
        [
            "/opt/tools/rhythm",
            "run",
            "com.example.scan",
            "--forward",
            "--",
            "/bin/sh",
            "-c",
            "echo sample"
        ]
    );
    let adopted = plist::Value::from_file(f.agent("com.example.scan")).unwrap();
    assert_eq!(
        adopted.as_dictionary().unwrap()["Nice"].as_signed_integer(),
        Some(5)
    );
    // A private definition stays private.
    let mode = fs::metadata(f.agent("com.example.scan"))
        .unwrap()
        .permissions();
    assert_eq!(mode.mode() & 0o777, 0o600);
    assert_eq!(f.loaded("com.example.scan").as_deref(), Some("gui/501"));
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &scan, false),
        Ok(Adoption::AlreadyRecorded)
    );

    authoring::release(&f.launchd, &scan, false).unwrap();
    assert_eq!(f.arguments("com.example.scan"), original);
    assert_eq!(
        authoring::release(&f.launchd, &scan, false),
        Err(Error::NotRecorded)
    );
    assert_eq!(
        f.calls(),
        [
            "bootout gui/501/com.example.scan",
            "bootstrap gui/501 $TMP/home/Library/LaunchAgents/com.example.scan.plist",
            "bootout gui/501/com.example.scan",
            "bootstrap gui/501 $TMP/home/Library/LaunchAgents/com.example.scan.plist",
        ]
    );
}

#[test]
fn adopting_again_repoints_a_job_at_the_current_recorder() {
    let f = Fixture::new();
    let scan = label("com.example.scan");
    f.foreign("com.example.scan", "");
    let mut old = f.recorder();
    old.executable = "/opt/old/rhythm".into();
    authoring::adopt(&f.launchd, &old, &scan, false).unwrap();
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &scan, false),
        Ok(Adoption::Repointed)
    );
    assert_eq!(
        f.arguments("com.example.scan"),
        [
            "/opt/tools/rhythm",
            "run",
            "com.example.scan",
            "--",
            "/bin/sh",
            "-c",
            "echo sample"
        ]
    );
}

#[test]
fn adoption_changes_the_file_launchd_loaded() {
    let f = Fixture::new();
    let scan = label("com.example.scan");
    // Two files declare the label; launchd loaded the unconventional one.
    let elsewhere = f.path().join("home/Library/LaunchAgents/scan-copy.plist");
    f.foreign("com.example.scan", "");
    f.foreign_at(&elsewhere, "com.example.scan", "");
    authoring::adopt(&f.launchd, &f.recorder(), &scan, false).unwrap();
    assert_eq!(f.arguments_in(&elsewhere)[0], "/opt/tools/rhythm");
    assert_eq!(f.arguments("com.example.scan")[0], "/bin/sh");

    // A definition loaded from outside LaunchAgents is not Rhythm's to rewrite.
    let outside = f.path().join("work/outside.plist");
    f.foreign_at(&outside, "com.example.outside", "");
    assert_eq!(
        authoring::adopt(
            &f.launchd,
            &f.recorder(),
            &label("com.example.outside"),
            false
        ),
        Err(Error::UnsupportedDefinition)
    );
}

#[test]
fn adopting_a_running_job_needs_restart_and_unloaded_jobs_stay_unloaded() {
    let f = Fixture::new();
    let server = label("com.example.server");
    f.foreign("com.example.server", "<key>KeepAlive</key><true/>");
    fs::write(
        f.path().join("observed/com.example.server"),
        "\truns = 1\n\tpid = 42\n",
    )
    .unwrap();
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &server, false),
        Err(Error::RestartRequired)
    );
    assert_eq!(f.arguments("com.example.server")[0], "/bin/sh");
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &server, true),
        Ok(Adoption::Adopted)
    );

    let idle = label("com.example.idle");
    f.foreign("com.example.idle", "");
    fs::remove_file(f.path().join("loaded/com.example.idle")).unwrap();
    authoring::adopt(&f.launchd, &f.recorder(), &idle, false).unwrap();
    assert_eq!(f.loaded("com.example.idle"), None);
    // Without log files of its own, output is only recorded.
    assert!(
        !f.arguments("com.example.idle")
            .contains(&"--forward".to_owned())
    );
}

#[test]
fn a_program_only_job_round_trips() {
    let f = Fixture::new();
    let tool = label("com.example.tool");
    fs::write(
        f.agent("com.example.tool"),
        r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>com.example.tool</string><key>Program</key><string>/usr/bin/true</string></dict></plist>"#,
    )
    .unwrap();
    authoring::adopt(&f.launchd, &f.recorder(), &tool, false).unwrap();
    assert_eq!(
        f.arguments("com.example.tool"),
        [
            "/opt/tools/rhythm",
            "run",
            "com.example.tool",
            "--",
            "/usr/bin/true"
        ]
    );
    let adopted = plist::Value::from_file(f.agent("com.example.tool")).unwrap();
    assert!(!adopted.as_dictionary().unwrap().contains_key("Program"));
    authoring::release(&f.launchd, &tool, false).unwrap();
    assert_eq!(f.arguments("com.example.tool"), ["/usr/bin/true"]);
}

#[test]
fn definitions_rhythm_cannot_change_safely_are_left_alone() {
    let f = Fixture::new();
    f.foreign(
        "com.example.both",
        "<key>Program</key><string>/bin/sh</string>",
    );
    assert_eq!(
        authoring::adopt(&f.launchd, &f.recorder(), &label("com.example.both"), false),
        Err(Error::UnsupportedDefinition)
    );

    f.foreign("com.example.linked", "");
    fs::remove_file(f.path().join("loaded/com.example.linked")).unwrap();
    let target = f.path().join("work/linked.plist");
    fs::rename(f.agent("com.example.linked"), &target).unwrap();
    std::os::unix::fs::symlink(&target, f.agent("com.example.linked")).unwrap();
    assert_eq!(
        authoring::adopt(
            &f.launchd,
            &f.recorder(),
            &label("com.example.linked"),
            false
        ),
        Err(Error::UnsupportedDefinition)
    );

    assert_eq!(
        authoring::adopt(
            &f.launchd,
            &f.recorder(),
            &label("com.example.missing"),
            false
        ),
        Err(Error::JobNotFound)
    );
}

#[test]
fn removes_only_jobs_rhythm_created() {
    let f = Fixture::new();
    f.add("report", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    f.foreign("com.example.scan", "");
    let scan = label("com.example.scan");
    authoring::adopt(&f.launchd, &f.recorder(), &scan, false).unwrap();
    assert_eq!(authoring::remove(&f.launchd, &scan), Err(Error::NotOwned));
    assert!(f.agent("com.example.scan").exists());

    let report = authoring::resolve("report").unwrap();
    authoring::remove(&f.launchd, &report).unwrap();
    assert!(!f.agent("dev.rhythm.report").exists());
    assert_eq!(f.loaded("dev.rhythm.report"), None);
}

#[test]
fn kick_starts_a_job_and_waits_for_launchds_result() {
    let f = Fixture::new();
    let (report, _) = f
        .add("report", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    fs::write(
        f.path().join("after-kick/dev.rhythm.report"),
        "\truns = 1\n\tlast exit code = 3\n",
    )
    .unwrap();
    let kicked = kick::kick(&f.launchd, &report, Some(Duration::from_secs(5)), &|_| None).unwrap();
    let finished = kicked.finished.unwrap();
    assert_eq!(finished.exit, Some(Exit::Code { code: 3 }));
    assert!(!finished.succeeded());
    assert!(
        f.calls()
            .contains(&"kickstart gui/501/dev.rhythm.report".to_owned())
    );

    // Already running: launchd would not start another instance.
    fs::write(
        f.path().join("observed/dev.rhythm.report"),
        "\truns = 2\n\tpid = 7\n",
    )
    .unwrap();
    assert_eq!(
        kick::kick(&f.launchd, &report, None, &|_| None),
        Err(Error::Busy)
    );
    assert_eq!(
        kick::kick(&f.launchd, &label("com.example.unknown"), None, &|_| None),
        Err(Error::JobNotLoaded)
    );
}

#[test]
fn kick_reports_a_job_that_started_on_its_own_as_busy() {
    let f = Fixture::new();
    let (report, _) = f
        .add("report", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    fs::write(f.path().join("kick-busy/dev.rhythm.report"), "").unwrap();
    assert_eq!(
        kick::kick(&f.launchd, &report, None, &|_| None),
        Err(Error::Busy)
    );
}

#[test]
fn kick_shows_only_the_run_it_started() {
    let f = Fixture::new();
    let (report, _) = f
        .add("report", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    let state = f.path().join("state");
    {
        // An earlier run, recorded before this kick.
        let ledger = Ledger::open(Root::open(state.clone()).unwrap(), ledger::READER_WAIT).unwrap();
        let id = ledger.start(&report, 1, Timestamp(1000)).unwrap();
        ledger
            .finish(id, Timestamp(2000), Outcome::Code { code: 0 })
            .unwrap();
    }
    // This kick's run was never recorded, as when the recorder cannot start.
    fs::write(
        f.path().join("after-kick/dev.rhythm.report"),
        "\truns = 1\n\tlast exit code = 78\n",
    )
    .unwrap();
    let ledgers = |_: &_| Ledger::open_existing(Root::open(state.clone()).unwrap()).unwrap();
    let finished = kick::kick(&f.launchd, &report, Some(Duration::from_secs(5)), &ledgers)
        .unwrap()
        .finished
        .unwrap();
    assert_eq!(finished.exit, Some(Exit::Code { code: 78 }));
    assert_eq!(finished.run, None);
    assert_eq!(finished.output, None);
}

#[test]
fn kick_gives_up_waiting_without_stopping_the_run() {
    let f = Fixture::new();
    let (slow, _) = f
        .add("slow", on_demand(), &["/usr/bin/true"], false)
        .unwrap();
    fs::write(
        f.path().join("after-kick/dev.rhythm.slow"),
        "\truns = 1\n\tpid = 9\n",
    )
    .unwrap();
    assert_eq!(
        kick::kick(&f.launchd, &slow, Some(Duration::from_millis(300)), &|_| {
            None
        }),
        Err(Error::TimedOut)
    );
    assert!(!f.calls().iter().any(|c| c.starts_with("bootout")));
}
