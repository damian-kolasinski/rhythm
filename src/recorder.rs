//! Run a job's command in place of launchd's direct launch and record the run.
//! The command keeps launchd's environment, working directory, stdin, and
//! process group, and the recorder exits the way the command did, so launchd
//! still sees the job's real result. Recording never stops the job from running.
use crate::{
    failure_notice::Notice,
    jobs::Label,
    ledger::{Ledger, RunLock},
    notifier::Notifier,
    operation_error::Result,
    run_output::OutputLog,
    runs::{Outcome, Timestamp},
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, RawFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// How long the recorder keeps collecting output after the command exits
/// before handing the pipes to a detached drainer.
const DRAIN: Duration = Duration::from_secs(2);
const FORWARDED: [libc::c_int; 6] = [
    libc::SIGTERM,
    libc::SIGINT,
    libc::SIGHUP,
    libc::SIGQUIT,
    libc::SIGUSR1,
    libc::SIGUSR2,
];
/// fds the drainer reads the command's stdout and stderr from.
const DRAIN_FDS: [RawFd; 2] = [3, 4];

static CHILD: AtomicI32 = AtomicI32::new(0);
/// Signals received before the command started, as a bit set.
static PENDING: AtomicU64 = AtomicU64::new(0);
/// Set when a stop request arrived, as when the job is unloaded or the user logs out.
static STOPPED: AtomicBool = AtomicBool::new(false);

extern "C" fn forward(signal: libc::c_int) {
    // Only async-signal-safe work here.
    if matches!(
        signal,
        libc::SIGTERM | libc::SIGINT | libc::SIGHUP | libc::SIGQUIT
    ) {
        STOPPED.store(true, Ordering::SeqCst);
    }
    match CHILD.load(Ordering::SeqCst) {
        0 => {
            PENDING.fetch_or(1 << signal, Ordering::SeqCst);
        }
        child => unsafe {
            libc::kill(child, signal);
        },
    }
}

extern "C" fn outlive(_: libc::c_int) {}

pub struct Recording {
    pub program: Vec<OsString>,
    /// Also copy output to the recorder's own stdout and stderr, which launchd
    /// may route to the job's original log files.
    pub forward: bool,
}

/// Records under `label` in `ledger`, or only runs the command when either is unusable.
pub fn record(target: Result<(Ledger, Label)>, recording: Recording) -> ! {
    // In a terminal's foreground, interrupt and quit already reach the command
    // from the terminal itself; launchd never gives a job a terminal.
    let interactive = unsafe {
        libc::isatty(libc::STDIN_FILENO) == 1
            && libc::tcgetpgrp(libc::STDIN_FILENO) == libc::getpgrp()
    };
    for signal in FORWARDED {
        // The recorder outlives the command's own handling of a terminal
        // interrupt. A handler, unlike SIG_IGN, is not inherited by the command.
        let handler = if interactive && matches!(signal, libc::SIGINT | libc::SIGQUIT) {
            outlive as extern "C" fn(libc::c_int)
        } else {
            forward
        };
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
    let started = target.and_then(|(ledger, label)| {
        let id = ledger.start(&label, std::process::id(), Timestamp::now())?;
        // A run without stored output is still a recorded run.
        let storage = ledger.output_dir(id).ok().and_then(|dir| {
            let lock = RunLock::hold(&dir).ok()?;
            Some((OutputLog::create(&dir).ok()?, dir, lock))
        });
        Ok((ledger, label, id, storage))
    });
    stop_if_signalled(&started);
    let (ledger, label, id, storage) = match started {
        Ok(started) => started,
        Err(error) => {
            eprintln!("rhythm: this run cannot be recorded ({error}); running the command anyway");
            let error = command(&recording.program).exec();
            eprintln!("rhythm: the command could not be started");
            std::process::exit(exit_code_for(&error));
        }
    };
    let (output, dir, _lock) = match storage {
        Some((output, dir, lock)) => (Some(output), Some(dir), Some(lock)),
        None => (None, None, None),
    };

    let mut child = match command(&recording.program)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = ledger.finish(id, Timestamp::now(), Outcome::SpawnFailed);
            // A deleted script or a wrong path is the most common silent breakage.
            notify(&ledger, &label, id, Outcome::SpawnFailed);
            eprintln!("rhythm: the command could not be started");
            std::process::exit(exit_code_for(&error));
        }
    };
    let pid = child.id() as i32;
    CHILD.store(pid, Ordering::SeqCst);
    for signal in pending() {
        unsafe { libc::kill(pid, signal) };
    }

    let output = Arc::new(Mutex::new(output));
    let (drained, done) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let pipes = [stdout.as_raw_fd(), stderr.as_raw_fd()];
    let forward = recording.forward;
    pump(
        stdout,
        output.clone(),
        forward.then(stdout_target),
        drained.clone(),
    );
    pump(stderr, output.clone(), forward.then(stderr_target), drained);

    // The command stays a zombie until the end, so its pid cannot be reused
    // by another process while signals may still be forwarded to it.
    let outcome = exit_without_reaping(pid);
    let deadline = Instant::now() + DRAIN;
    let mut open = 2;
    while open > 0
        && done
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    {
        open -= 1;
    }
    let outcome = match outcome {
        Outcome::Signal { signal } if STOPPED.load(Ordering::SeqCst) => Outcome::Stopped { signal },
        outcome => outcome,
    };
    if ledger.finish(id, Timestamp::now(), outcome).is_err() {
        eprintln!("rhythm: the outcome of this run could not be recorded");
    }
    notify(&ledger, &label, id, outcome);
    if open > 0 {
        // Descendants still hold the pipes. Closing them would kill those
        // descendants with SIGPIPE, which a direct launch would not.
        if let Ok(mut output) = output.lock() {
            output.take();
        }
        hand_off(pipes, dir.as_deref(), forward);
    }
    CHILD.store(0, Ordering::SeqCst);
    reap(&mut child);
    exit_as(outcome)
}

/// Tells the operator when the job starts failing or recovers. A run ended by
/// a stop request is not a failure of the job.
fn notify(ledger: &Ledger, label: &Label, id: i64, outcome: Outcome) {
    if STOPPED.load(Ordering::SeqCst) {
        return;
    }
    let previous = ledger.previous_finished(label.as_str(), id).ok().flatten();
    if let Some(notice) = Notice::after(previous.as_ref().map(|r| &r.state), outcome) {
        Notifier::from_environment().deliver(
            &notice.title(label.as_str()),
            &notice.message(label.as_str()),
        );
    }
}

fn command(program: &[OsString]) -> Command {
    let mut command = Command::new(&program[0]);
    command.args(&program[1..]);
    command
}

fn pending() -> Vec<libc::c_int> {
    let bits = PENDING.swap(0, Ordering::SeqCst);
    (1..64).filter(|s| bits & (1 << s) != 0).collect()
}

/// launchd asked the job to stop before the command started: do not start it.
fn stop_if_signalled<T>(started: &Result<(Ledger, Label, i64, T)>) {
    if let Some(signal) = pending().first() {
        let outcome = Outcome::Signal { signal: *signal };
        if let Ok((ledger, _, id, _)) = started {
            let _ = ledger.finish(*id, Timestamp::now(), outcome);
        }
        exit_as(outcome);
    }
}

fn stdout_target() -> Box<dyn Write + Send> {
    Box::new(io::stdout())
}
fn stderr_target() -> Box<dyn Write + Send> {
    Box::new(io::stderr())
}

/// Copies one stream into the run's output, stopping writes but never reads on
/// a storage error so the command cannot block on a full pipe.
fn pump(
    mut stream: impl Read + Send + 'static,
    output: Arc<Mutex<Option<OutputLog>>>,
    mut forward: Option<Box<dyn Write + Send>>,
    drained: mpsc::Sender<()>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let chunk = &buffer[..read];
            if let Ok(mut output) = output.lock()
                && let Some(log) = output.as_mut()
                && log.write(chunk).is_err()
            {
                *output = None;
            }
            if let Some(target) = forward.as_mut()
                && target
                    .write_all(chunk)
                    .and_then(|_| target.flush())
                    .is_err()
            {
                forward = None;
            }
        }
        let _ = drained.send(());
    });
}

/// Starts `rhythm __drain` in its own session holding the command's pipes.
fn hand_off(pipes: [RawFd; 2], dir: Option<&Path>, forward: bool) {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let mut drainer = Command::new(executable);
    drainer.arg("__drain");
    if let Some(dir) = dir {
        drainer.arg("--dir").arg(dir);
    }
    if forward {
        drainer.arg("--forward");
    }
    drainer.stdin(Stdio::null());
    if !forward {
        // Only forwarded output needs the recorder's own streams.
        drainer.stdout(Stdio::null()).stderr(Stdio::null());
    }
    unsafe {
        drainer.pre_exec(move || {
            libc::setsid();
            // Copy above the target numbers first so neither dup2 clobbers the other.
            let high = pipes.map(|fd| libc::fcntl(fd, libc::F_DUPFD, 10));
            for (fd, target) in high.into_iter().zip(DRAIN_FDS) {
                if fd < 0 || libc::dup2(fd, target) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let _ = drainer.spawn();
}

/// The detached drainer: reads both pipes to EOF so late writers keep working.
pub fn drain(dir: Option<PathBuf>, forward: bool) {
    let output = Arc::new(Mutex::new(dir.and_then(|d| OutputLog::append(&d).ok())));
    let (drained, done) = mpsc::channel();
    let [out, err] = DRAIN_FDS.map(|fd| unsafe { std::fs::File::from_raw_fd(fd) });
    pump(
        out,
        output.clone(),
        forward.then(stdout_target),
        drained.clone(),
    );
    pump(err, output, forward.then(stderr_target), drained);
    for _ in 0..2 {
        let _ = done.recv();
    }
}

/// Waits for the command to exit while leaving it unreaped.
fn exit_without_reaping(pid: i32) -> Outcome {
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            let status = child_status(&info);
            match info.si_code {
                libc::CLD_EXITED => return Outcome::Code { code: status },
                libc::CLD_KILLED | libc::CLD_DUMPED => {
                    return Outcome::Signal { signal: status };
                }
                _ => continue,
            }
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Outcome::SpawnFailed;
        }
    }
}

#[cfg(target_os = "linux")]
fn child_status(info: &libc::siginfo_t) -> i32 {
    unsafe { info.si_status() }
}
#[cfg(not(target_os = "linux"))]
fn child_status(info: &libc::siginfo_t) -> i32 {
    info.si_status
}

fn reap(child: &mut Child) {
    let _ = child.wait();
}

fn exit_code_for(error: &io::Error) -> i32 {
    if error.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

/// Leaves with the command's own status: its exit code, or the same signal.
fn exit_as(outcome: Outcome) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    match outcome {
        Outcome::Code { code } => std::process::exit(code),
        Outcome::Signal { signal } | Outcome::Stopped { signal } => unsafe {
            // Dying like the command must not leave a core or crash report of our own.
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            libc::signal(signal, libc::SIG_DFL);
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, signal);
            libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
            libc::raise(signal);
            std::process::exit(128 + signal)
        },
        Outcome::SpawnFailed => std::process::exit(126),
    }
}
