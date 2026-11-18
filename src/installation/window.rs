use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use plist::{Dictionary, Value};
use sha2::{Digest, Sha256};

use crate::{
    bounded_command,
    operation_error::{Error, Result},
};

pub const READY_ARGUMENT: &str = "--rhythm-ready";
const IDENTIFIER: &str = "dev.rhythm.desktop";
const EXECUTABLE: &str = "Contents/MacOS/rhythm-ui";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub version: String,
    pub digest: String,
}

/// The launch boundary also owns signing so staging is exercised with the real
/// filesystem in tests, with the platform actions supplied by a test adapter.
pub trait Desktop {
    fn available(&self) -> bool;
    fn sign(&self, staged: &Path) -> Result<()>;
    fn open(&self, app: &Path, home: &Path) -> Result<()>;
}

pub struct MacDesktop;

impl Desktop for MacDesktop {
    fn available(&self) -> bool {
        let mut command = Command::new("/bin/launchctl");
        command.args(["print", &format!("gui/{}", unsafe { libc::getuid() })]);
        checked(command, Duration::from_secs(5), Error::WindowUnavailable).is_ok()
    }

    fn sign(&self, stage: &Path) -> Result<()> {
        let mut command = Command::new("/usr/bin/codesign");
        command.args(["--force", "--sign", "-"]).arg(stage);
        checked(command, Duration::from_secs(30), Error::WindowUnavailable)
    }

    fn open(&self, app: &Path, home: &Path) -> Result<()> {
        let scratch = tempfile::Builder::new()
            .prefix("rhythm-ready-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .map_err(|_| Error::WindowUnavailable)?;
        let socket = scratch.path().join("window");
        let ready = UnixListener::bind(&socket).map_err(|_| Error::WindowUnavailable)?;
        ready
            .set_nonblocking(true)
            .map_err(|_| Error::WindowUnavailable)?;
        let mut home_env = std::ffi::OsString::from("HOME=");
        home_env.push(home);
        let mut command = Command::new("/usr/bin/open");
        command
            .args(["-n", "--env"])
            .arg(home_env)
            .arg(app)
            .args(["--args", READY_ARGUMENT])
            .arg(&socket);
        checked(command, Duration::from_secs(10), Error::WindowUnavailable)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match ready.accept() {
                Ok((peer, _)) => {
                    let answer = read_ready(peer).map_err(|_| Error::WindowUnavailable)?;
                    return if &answer == b"ready" {
                        Ok(())
                    } else {
                        Err(Error::WindowUnavailable)
                    };
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(30));
                }
                Err(_) => return Err(Error::WindowUnavailable),
            }
        }
    }
}

fn read_ready(mut peer: UnixStream) -> io::Result<[u8; 5]> {
    let mut answer = [0; 5];
    // Darwin inherits nonblocking mode from the listener. The read timeout
    // only waits for the rest of the acknowledgement on a blocking socket.
    peer.set_nonblocking(false)?;
    peer.set_read_timeout(Some(Duration::from_secs(1)))?;
    peer.read_exact(&mut answer)?;
    Ok(answer)
}

pub fn acknowledge(socket: &Path) -> Result<()> {
    UnixStream::connect(socket)
        .and_then(|mut stream| stream.write_all(b"ready"))
        .map_err(|_| Error::WindowUnavailable)
}

fn checked(command: Command, timeout: Duration, error: Error) -> Result<()> {
    bounded_command::run(command, timeout, 1024 * 1024)
        .ok()
        .filter(|output| output.status.success())
        .map(|_| ())
        .ok_or(error)
}

pub(super) fn write(app: &Path, source: &Path, version: &str) -> Result<()> {
    let bytes = fs::read(source).map_err(|_| Error::InstallationFailed)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    for directory in ["Contents/MacOS", "Contents/Resources"] {
        fs::create_dir_all(app.join(directory)).map_err(|_| Error::InstallationFailed)?;
    }
    super::copy_executable(source, &app.join(EXECUTABLE))?;
    fs::write(
        app.join("Contents/Resources/Rhythm.icns"),
        include_bytes!("../../assets/Rhythm.icns"),
    )
    .map_err(|_| Error::InstallationFailed)?;
    let values: Dictionary = [
        ("CFBundleDevelopmentRegion", "en"),
        ("CFBundleDisplayName", "Rhythm"),
        ("CFBundleExecutable", "rhythm-ui"),
        ("CFBundleIconFile", "Rhythm.icns"),
        ("CFBundleIdentifier", IDENTIFIER),
        ("CFBundleName", "Rhythm"),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleShortVersionString", version),
        ("CFBundleVersion", version),
        ("LSMinimumSystemVersion", "13.0"),
        ("RhythmBuild", digest.as_str()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), Value::String(value.into())))
    .collect();
    plist::to_file_xml(app.join("Contents/Info.plist"), &values)
        .map_err(|_| Error::InstallationFailed)
}

pub fn build(app: &Path) -> Result<Option<Build>> {
    match super::paths::owned(app)? {
        None => return Ok(None),
        Some(metadata) if metadata.is_dir() => {}
        _ => return Err(Error::InstallationConflict),
    }
    let plist = app.join("Contents/Info.plist");
    for path in [app.join("Contents"), app.join("Contents/MacOS")] {
        if super::paths::owned(&path)?.is_none_or(|metadata| !metadata.is_dir()) {
            return Err(Error::InstallationConflict);
        }
    }
    if super::paths::owned(&plist)?
        .is_none_or(|metadata| !metadata.is_file() || metadata.len() > 65536)
    {
        return Err(Error::InstallationConflict);
    }
    let info: Dictionary = plist::from_file(plist).map_err(|_| Error::InstallationConflict)?;
    let field = |key| info.get(key).and_then(Value::as_string);
    if field("CFBundleIdentifier") != Some(IDENTIFIER) {
        return Err(Error::InstallationConflict);
    }
    Ok(Some(Build {
        version: field("CFBundleShortVersionString")
            .ok_or(Error::InstallationConflict)?
            .into(),
        digest: field("RhythmBuild")
            .ok_or(Error::InstallationConflict)?
            .into(),
    }))
}

pub fn running_app(executable: &Path) -> Option<&Path> {
    let (app, child) = executable.ancestors().nth(3).zip(executable.file_name())?;
    (app.extension().is_some_and(|name| name == "app")
        && child == "rhythm-ui"
        && executable.strip_prefix(app).ok()? == Path::new(EXECUTABLE))
    .then_some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_accepts_an_acknowledgement_sent_after_connecting() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        writer.write_all(b"re").unwrap();
        let window = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            writer.write_all(b"ady")
        });
        let received = read_ready(reader);
        let sent = window.join().unwrap();
        assert_eq!(received.unwrap(), *b"ready");
        sent.unwrap();
    }
}
