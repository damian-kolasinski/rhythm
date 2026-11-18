use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
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
const READY_MARKER: &str = "first-frame";
const IDENTIFIER: &str = "dev.rhythm.desktop";
const EXECUTABLE: &str = "Contents/MacOS/rhythm-ui";
pub(super) const ICON: &str = "Rhythm.icns";

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
        checked(command, Duration::from_secs(30), Error::WindowSignFailed)
    }

    fn open(&self, app: &Path, home: &Path) -> Result<()> {
        let applications = app.parent().ok_or(Error::WindowStartFailed)?;
        let scratch = tempfile::Builder::new()
            .prefix(".rhythm-ready-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(applications)
            .map_err(|_| Error::WindowStartFailed)?;
        let mut home_env = std::ffi::OsString::from("HOME=");
        home_env.push(home);
        let mut command = Command::new("/usr/bin/open");
        command
            .args(["-n", "--env"])
            .arg(home_env)
            .arg(app)
            .args(["--args", READY_ARGUMENT])
            .arg(scratch.path());
        checked(command, Duration::from_secs(10), Error::WindowLaunchFailed)?;
        wait_for_frame(scratch.path(), Duration::from_secs(15))
    }
}

fn wait_for_frame(directory: &Path, timeout: Duration) -> Result<()> {
    let marker = directory.join(READY_MARKER);
    let deadline = Instant::now() + timeout;
    loop {
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.is_file() => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::WindowStartTimeout);
                }
                std::thread::sleep(remaining.min(Duration::from_millis(30)));
            }
            _ => return Err(Error::WindowStartFailed),
        }
    }
}

pub fn acknowledge(directory: &Path) -> Result<()> {
    // The installer owns the private directory. Creating the marker atomically
    // lets the first-frame signal survive until the installer observes it.
    fs::File::create_new(directory.join(READY_MARKER))
        .map(|_| ())
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
    let icon = source.with_file_name(ICON);
    let bytes = if icon.is_file() {
        released_icon(&icon)?
    } else {
        include_bytes!("../../assets/Rhythm.icns").to_vec()
    };
    fs::write(app.join("Contents/Resources").join(ICON), bytes)
        .map_err(|_| Error::InstallationFailed)?;
    let values: Dictionary = [
        ("CFBundleDevelopmentRegion", "en"),
        ("CFBundleDisplayName", "Rhythm"),
        ("CFBundleExecutable", "rhythm-ui"),
        ("CFBundleIconFile", ICON),
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

pub(super) fn released_icon(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::InstallationFailed)?;
    if !metadata.is_file() || !(8..=4 * 1024 * 1024).contains(&metadata.len()) {
        return Err(Error::InstallationFailed);
    }
    let bytes = fs::read(path).map_err(|_| Error::InstallationFailed)?;
    if !bytes.starts_with(b"icns") {
        return Err(Error::InstallationFailed);
    }
    Ok(bytes)
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
    fn startup_requires_a_frame_acknowledgement_from_this_launch() {
        let scratch = tempfile::tempdir().unwrap();
        let first = scratch.path().join("a".repeat(110));
        let second = scratch.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();

        acknowledge(&first).unwrap();
        assert_eq!(wait_for_frame(&first, Duration::ZERO), Ok(()));
        assert_eq!(
            wait_for_frame(&second, Duration::ZERO),
            Err(Error::WindowStartTimeout)
        );
    }
}
