//! A guided tour: disposable synthetic jobs in the real user launchd, driven by
//! the same commands an agent would run, and removed when the tour ends.
use crate::operation_error::{Error, Result};
use std::{
    ffi::OsStr,
    fs,
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const BACKUP: &str = "demo-backup";
const REPORT: &str = "demo-report";
/// Written by hand, the way agents usually leave jobs behind.
const SYNC: &str = "com.example.demo-sync";
const LABELS: [&str; 3] = ["dev.rhythm.demo-backup", "dev.rhythm.demo-report", SYNC];

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

struct Tour {
    executable: PathBuf,
    dir: PathBuf,
    domain: String,
    pause: Duration,
    color: bool,
}

pub fn run(executable: PathBuf, fast: bool, color: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(Error::UnsupportedPlatform);
    }
    let user = unsafe { libc::getuid() };
    let gui = format!("gui/{user}");
    let domain = if launchctl(&["print".as_ref(), gui.as_ref()]) {
        gui
    } else {
        format!("user/{user}")
    };
    // Never touch jobs a previous, interrupted tour may have left loaded.
    if LABELS
        .iter()
        .any(|label| launchctl(&["print".as_ref(), format!("{domain}/{label}").as_ref()]))
    {
        return Err(Error::DemoJobsLoaded);
    }
    let dir = std::env::temp_dir().join(format!(
        "rhythm-demo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos())
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .and_then(|_| fs::create_dir_all(dir.join("home/Library/LaunchAgents")))
        .map_err(|_| Error::StorageUnavailable)?;
    unsafe {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(signal, interrupted as *const () as libc::sighandler_t);
        }
        // A closed terminal or pipe must end the tour, not skip its cleanup.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let tour = Tour {
        executable,
        dir,
        domain,
        pause: Duration::from_millis(if fast { 0 } else { 1600 }),
        color,
    };
    tour.show();
    Ok(())
}

/// The tour cleans up however it ends, including by a panic.
impl Drop for Tour {
    fn drop(&mut self) {
        self.clean_up();
    }
}

impl Tour {
    fn show(&self) {
        self.say(
            "Rhythm demo: three disposable jobs in your real launchd, removed when the demo ends.",
        );
        self.say("Create two jobs the predictable way:");
        self.rhythm(&[
            "add",
            BACKUP,
            "--every",
            "1h",
            "--",
            "/bin/sh",
            "-c",
            r#"echo "backing up 3 projects"; sleep 1; echo "done""#,
        ]);
        self.rhythm(&[
            "add",
            REPORT,
            "--daily",
            "09:00",
            "--",
            "/bin/sh",
            "-c",
            r#"echo "fetching sales data"; sleep 0.2; echo "error: upstream returned 503" >&2; exit 1"#,
        ]);
        self.say("A third one was written by hand, as agents usually do:");
        self.hand_written();
        self.rhythm(&["status"]);
        self.say("Record its runs without changing anything else about it:");
        self.rhythm(&["adopt", SYNC]);
        self.say("Run all three now and wait for each result:");
        self.rhythm(&["kick", BACKUP, "--wait"]);
        self.rhythm(&["kick", REPORT, "--wait"]);
        self.rhythm(&["kick", SYNC, "--wait"]);
        self.rhythm(&["status"]);
        self.say("Every run of a recorded job keeps its timing and output:");
        self.rhythm(&["history", REPORT]);
        self.rhythm(&["logs", REPORT]);
        self.say("A session-start hook asks what is new since it last looked:");
        self.rhythm(&["attention", "--since-last-seen", "--as", "demo"]);
        self.say("Asking again right away reports nothing, so hooks stay quiet:");
        self.rhythm(&["attention", "--since-last-seen", "--as", "demo"]);
    }

    fn hand_written(&self) {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return;
        }
        let plist = self.agent(SYNC);
        let log = self.dir.join("sync.log");
        let _ = fs::write(
            &plist,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{SYNC}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>-c</string>
		<string>echo 'synced 12 files'</string>
	</array>
	<key>StartInterval</key>
	<integer>900</integer>
	<key>StandardOutPath</key>
	<string>{}</string>
</dict>
</plist>
"#,
                log.display()
            ),
        );
        // The session's numeric user id stays out of the tour's output.
        let session = self.domain.split('/').next().unwrap_or("gui");
        self.say_command(&format!(
            "launchctl bootstrap {session}/$UID ~/Library/LaunchAgents/{SYNC}.plist"
        ));
        launchctl(&[
            "bootstrap".as_ref(),
            self.domain.as_ref(),
            plist.as_os_str(),
        ]);
        self.pause();
    }

    fn clean_up(&self) {
        let quiet = |args: &[&str]| {
            let _ = self
                .command(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        quiet(&["remove", BACKUP]);
        quiet(&["remove", REPORT]);
        launchctl(&[
            "bootout".as_ref(),
            format!("{}/{SYNC}", self.domain).as_ref(),
        ]);
        let _ = fs::remove_dir_all(&self.dir);
        self.say("Demo jobs removed.");
    }

    fn agent(&self, label: &str) -> PathBuf {
        self.dir
            .join(format!("home/Library/LaunchAgents/{label}.plist"))
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .env("HOME", self.dir.join("home"))
            .arg("--data-dir")
            .arg(self.dir.join("state"))
            .args(args);
        command
    }

    fn rhythm(&self, args: &[&str]) {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return;
        }
        let shown: Vec<String> = args.iter().map(|a| shell_word(a)).collect();
        self.say_command(&format!("rhythm {}", shown.join(" ")));
        let _ = self.command(args).status();
        self.pause();
    }

    fn say(&self, text: &str) {
        if INTERRUPTED.load(Ordering::SeqCst) && text != "Demo jobs removed." {
            return;
        }
        let text = if self.color {
            format!("\x1b[2m# {text}\x1b[0m")
        } else {
            format!("# {text}")
        };
        let _ = writeln!(std::io::stdout(), "\n{text}");
    }

    fn say_command(&self, text: &str) {
        let line = if self.color {
            format!("\x1b[1m$ {text}\x1b[0m")
        } else {
            format!("$ {text}")
        };
        let _ = writeln!(std::io::stdout(), "{line}");
        let _ = std::io::stdout().flush();
    }

    fn pause(&self) {
        std::thread::sleep(self.pause);
    }
}

fn shell_word(word: &str) -> String {
    if word
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_./:@".contains(&b))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn launchctl(args: &[&OsStr]) -> bool {
    Command::new("/bin/launchctl")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
