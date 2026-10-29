//! Run a helper program with a deadline and bounded output.
use std::{
    io::Read,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Unavailable,
    TimedOut,
    /// Either stream exceeded the limit; the program was stopped.
    TooLarge,
}

pub fn run(mut command: Command, timeout: Duration, limit: u64) -> Result<Output, Failure> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Failure::Unavailable)?;
    let (sender, streams) = mpsc::channel();
    collect(0, child.stdout.take().unwrap(), limit, sender.clone());
    collect(1, child.stderr.take().unwrap(), limit, sender);
    let deadline = Instant::now() + timeout;
    let mut collected: [Option<Vec<u8>>; 2] = [None, None];
    let mut status = None;
    // Descendants may hold the pipes after the program exits, so the deadline
    // covers reading as well as waiting.
    while status.is_none() || collected.iter().any(Option::is_none) {
        match streams.recv_timeout(Duration::from_millis(5)) {
            Ok((_, None)) => return Err(stop(&mut child, Failure::TooLarge)),
            Ok((index, Some(bytes))) => collected[index] = Some(bytes),
            Err(_) => {}
        }
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|_| stop(&mut child, Failure::Unavailable))?;
        }
        if Instant::now() >= deadline {
            return Err(stop(&mut child, Failure::TimedOut));
        }
    }
    let [stdout, stderr] = collected.map(Option::unwrap_or_default);
    Ok(Output {
        status: status.unwrap(),
        stdout,
        stderr,
    })
}

fn stop(child: &mut Child, failure: Failure) -> Failure {
    let _ = child.kill();
    let _ = child.wait();
    failure
}

/// Sends the stream's bytes, or `None` once it exceeds `limit`.
fn collect(
    index: usize,
    stream: impl Read + Send + 'static,
    limit: u64,
    streams: mpsc::Sender<(usize, Option<Vec<u8>>)>,
) {
    std::thread::spawn(move || {
        let mut bytes = vec![];
        let _ = stream.take(limit + 1).read_to_end(&mut bytes);
        let _ = streams.send((index, (bytes.len() as u64 <= limit).then_some(bytes)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn collects_both_streams() {
        let output = run(
            shell("printf out; printf err >&2; exit 3"),
            Duration::from_secs(5),
            64,
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(
            (output.stdout.as_slice(), output.stderr.as_slice()),
            (&b"out"[..], &b"err"[..])
        );
    }

    #[test]
    fn does_not_wait_for_descendants_holding_the_pipes() {
        let started = Instant::now();
        assert_eq!(
            run(shell("sleep 5 & exit 0"), Duration::from_millis(300), 64).err(),
            Some(Failure::TimedOut)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stops_slow_and_noisy_programs() {
        let started = Instant::now();
        assert_eq!(
            run(shell("sleep 5"), Duration::from_millis(200), 64).err(),
            Some(Failure::TimedOut)
        );
        assert_eq!(
            run(shell("yes; sleep 5"), Duration::from_secs(5), 1024).err(),
            Some(Failure::TooLarge)
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(
            run(
                Command::new("/nonexistent/program"),
                Duration::from_secs(1),
                1
            )
            .err(),
            Some(Failure::Unavailable)
        );
    }
}
