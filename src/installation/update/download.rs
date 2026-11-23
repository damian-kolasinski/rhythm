use std::{
    fs,
    path::{Path, PathBuf},
};

use semver::Version;
use tempfile::TempDir;

use crate::operation_error::{Error, Result};

use super::super::{
    Paths,
    mise::{self, Mise},
};

pub(super) const ISSUER: &str = "https://token.actions.githubusercontent.com";

pub(super) fn identity(repository: &str, version: &Version) -> String {
    format!("https://github.com/{repository}/.github/workflows/release.yml@refs/tags/v{version}")
}

pub(super) struct Verified {
    pub scratch: TempDir,
    pub binary: PathBuf,
}

pub(super) fn verify(
    paths: &Paths,
    mise: &Mise,
    repository: &str,
    version: &Version,
) -> Result<Verified> {
    let scratch = tempfile::Builder::new()
        .prefix("download-")
        .tempdir_in(&paths.area)
        .map_err(|_| Error::ReleaseVerificationFailed)?;
    let root = scratch
        .path()
        .canonicalize()
        .map_err(|_| Error::ReleaseVerificationFailed)?;
    // mise excludes the ceiling itself from configuration discovery.
    let project = root.join("project");
    fs::create_dir(&project).map_err(|_| Error::ReleaseVerificationFailed)?;
    let config = project.join("mise.toml");
    let empty = root.join("empty.toml");
    fs::write(&empty, "").map_err(|_| Error::ReleaseVerificationFailed)?;
    fs::write(&config, format!(
        "[tools]\n\"{}\" = {{ version = \"{version}\", identity = \"{}\", issuer = \"{ISSUER}\" }}\n",
        mise.tool, identity(repository, version)
    )).map_err(|_| Error::ReleaseVerificationFailed)?;
    let invoke = |args: &[&str]| {
        let mut command = mise.command(&paths.home);
        for (key, _) in std::env::vars_os() {
            let name = key.as_encoded_bytes();
            if name.starts_with(b"MISE_") && name != b"MISE_GITHUB_TOKEN" {
                command.env_remove(key);
            }
        }
        command
            .current_dir(&project)
            .env("MISE_YES", "1")
            .env("MISE_CONFIG_FILE", &config)
            .env("MISE_GLOBAL_CONFIG_FILE", &empty)
            .env("MISE_CEILING_PATHS", &root)
            .env("MISE_DATA_DIR", root.join("data"))
            .env("MISE_CACHE_DIR", root.join("cache"))
            .env("MISE_STATE_DIR", root.join("state"))
            .env("MISE_CONFIG_DIR", root.join("config"))
            .args(args);
        mise::run(command).map_err(|_| Error::ReleaseVerificationFailed)
    };
    invoke(&[
        "trust",
        config.to_str().ok_or(Error::ReleaseVerificationFailed)?,
    ])?;
    invoke(&["install"])?;
    // `mise which` returns a shim symlink for packslip tools.
    let request = format!("{}@{version}", mise.tool);
    let directory = release_directory(&invoke(&["where", &request])?)?;
    let binary = directory.join("rhythm");
    if !directory.starts_with(root.join("data"))
        || !regular(&directory.join(".mise-packslip.json"))
        || !regular(&binary)
        || !regular(&directory.join("rhythm-ui"))
        || super::super::window::released_icon(&directory.join(super::super::window::ICON)).is_err()
    {
        return Err(Error::ReleaseVerificationFailed);
    }
    Ok(Verified { scratch, binary })
}

fn release_directory(bytes: &[u8]) -> Result<PathBuf> {
    let path = PathBuf::from(
        std::str::from_utf8(bytes)
            .map_err(|_| Error::ReleaseVerificationFailed)?
            .trim(),
    );
    if !path.is_absolute() || !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(Error::ReleaseVerificationFailed);
    }
    path.canonicalize()
        .map_err(|_| Error::ReleaseVerificationFailed)
}

fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}
