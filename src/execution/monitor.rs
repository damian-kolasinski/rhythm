use super::Input;
use crate::{
    execution_records::*,
    operation_error::{Error, Result},
    private_storage::{self, Root},
};
use std::{
    fs::File,
    io::{Read, Write},
    os::{fd::FromRawFd, unix::process::ExitStatusExt},
    process::Stdio,
};
use tokio::{
    io::{AsyncReadExt, unix::AsyncFd},
    process::Command,
    time::{Duration, timeout},
};

pub async fn run_monitor(fd: i32) -> Result<()> {
    if fd < 3 || unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        return Err(Error::InvalidRequest);
    }
    // SAFETY: the parent passes this dedicated owned descriptor through exec.
    let _lock = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(Error::ExecutionUnavailable);
    }
    let input_fd = unsafe { libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 10) };
    if input_fd < 0 {
        return Err(Error::InvalidRequest);
    }
    // Exact framing avoids buffered control bytes and uncancellable stdin threads.
    let mut input = unsafe { File::from_raw_fd(input_fd) };
    let mut length = [0u8; 8];
    input
        .read_exact(&mut length)
        .map_err(|_| Error::InvalidRequest)?;
    let length = u64::from_be_bytes(length);
    if length > 131072 {
        return Err(Error::InvalidRequest);
    }
    let mut frame = vec![0; length as usize];
    input
        .read_exact(&mut frame)
        .map_err(|_| Error::InvalidRequest)?;
    if unsafe { libc::fcntl(input_fd, libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err(Error::ExecutionUnavailable);
    }
    // SAFETY: AsyncFd exclusively owns this File; its descriptor stays open and stable.
    let input = unsafe { AsyncFd::register(input) }.map_err(|_| Error::ExecutionUnavailable)?;
    let config: Input = serde_json::from_slice(&frame).map_err(|_| Error::InvalidRequest)?;
    let root = Root::open(config.root)?;
    let dir = root.run(config.id)?;
    let stdout = private_storage::private_file(&dir.join("stdout.log"))?;
    let stderr = private_storage::private_file(&dir.join("stderr.log"))?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| Error::ExecutionUnavailable)?;
    // This unreaped group leader prevents group-ID reuse even after the workload exits.
    let mut anchor =
        Command::new(std::env::current_exe().map_err(|_| Error::ExecutionUnavailable)?)
            .arg("__anchor")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::ExecutionUnavailable)?;
    let group = anchor.id().unwrap();
    let mut command = Command::new(&config.definition.command()[0]);
    command
        .args(&config.definition.command()[1..])
        .current_dir(config.definition.working_directory())
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(group as i32)
        .kill_on_drop(true);
    let outcome = match command.spawn() {
        Err(_) => {
            unsafe {
                libc::kill(-(group as i32), libc::SIGKILL);
            }
            let _ = anchor.wait().await;
            Outcome::SpawnFailed
        }
        Ok(mut child) => {
            let out = tokio::spawn(capture(child.stdout.take().unwrap(), stdout));
            let err = tokio::spawn(capture(child.stderr.take().unwrap(), stderr));
            let mut reason = tokio::select! {
                status=child.wait()=>match status{Ok(s)=>match s.code(){Some(code)=>Outcome::Exited{code},None=>Outcome::Signalled{signal:s.signal().unwrap_or(9)}},Err(_)=>Outcome::MonitorLost},
                read=control(&input)=>if matches!(read,Ok(Some(b'S'))){Outcome::Stopped}else{Outcome::Interrupted},
                _=term.recv()=>Outcome::Interrupted,
            };
            // Clean the group after every outcome, including a normally exiting leader.
            // The anchor remains owned and unreaped until after the final signal.
            unsafe {
                libc::kill(-(group as i32), libc::SIGTERM);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            unsafe {
                libc::kill(-(group as i32), libc::SIGKILL);
            }
            let _ = child.wait().await;
            let _ = anchor.wait().await;
            let drained = timeout(Duration::from_secs(1), async {
                let out = out.await;
                let err = err.await;
                matches!(out, Ok(true)) && matches!(err, Ok(true))
            })
            .await;
            if !matches!(drained, Ok(true)) {
                reason = Outcome::OutputUnavailable;
            }
            reason
        }
    };
    private_storage::write_json(
        &dir.join("result.json"),
        &Completion {
            outcome,
            finished_at_ms: timestamp_ms(),
        },
    )
}
async fn control(input: &AsyncFd<File>) -> std::io::Result<Option<u8>> {
    loop {
        let mut ready = input.readable().await?;
        let mut byte = [0];
        match ready.try_io(|file| {
            let mut f = file.get_ref();
            std::io::Read::read(&mut f, &mut byte)
        }) {
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(_)) => return Ok(Some(byte[0])),
            Ok(Err(e)) => return Err(e),
            Err(_) => {}
        }
    }
}
async fn capture(mut stream: impl tokio::io::AsyncRead + Unpin, mut file: File) -> bool {
    const LIMIT: usize = 1024 * 1024;
    let mut written = 0;
    let mut buffer = [0u8; 8192];
    let mut available = true;
    loop {
        let n = match stream.read(&mut buffer).await {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return available && file.sync_all().is_ok();
        }
        let keep = n.min(LIMIT - written);
        if keep > 0 {
            if available && file.write_all(&buffer[..keep]).is_err() {
                available = false
            }
            written += keep;
        }
    }
}
