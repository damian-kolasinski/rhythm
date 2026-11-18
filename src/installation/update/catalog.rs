use std::{process::Command, time::Duration};

use semver::Version;

use crate::{
    bounded_command,
    operation_error::{Error, Result},
};

/// Reading the catalog only discovers a candidate. The download boundary
/// authenticates it before any of its contents are executed.
pub trait Catalog {
    fn latest(&self) -> Result<Option<Version>>;
}

pub(super) struct Github<'a>(pub &'a str);

impl Catalog for Github<'_> {
    fn latest(&self) -> Result<Option<Version>> {
        let releases = format!("https://github.com/{}/releases", self.0);
        let redirect = curl(&[
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code} %{redirect_url}",
            &format!("{releases}/latest"),
        ])?;
        let (status, redirect) = redirect.split_once(' ').ok_or(Error::ReleaseCheckFailed)?;
        if !matches!(status, "301" | "302" | "303" | "307" | "308") {
            return Err(Error::ReleaseCheckFailed);
        }
        if redirect == releases {
            return Ok(None);
        }
        let tag = redirect
            .strip_prefix(&format!("{releases}/tag/v"))
            .ok_or(Error::ReleaseCheckFailed)?;
        let Some(version) = stable(tag) else {
            return Ok(None);
        };
        let archive = format!("rhythm-v{version}-aarch64-apple-darwin.tar.gz");
        for asset in [archive.as_str(), "packslip.sigstore.json", "install.sh"] {
            let status = curl(&[
                "--location",
                "--output",
                "/dev/null",
                "--write-out",
                "%{http_code}",
                &format!("{releases}/download/v{version}/{asset}"),
            ])?;
            match status.as_str() {
                "200" => {}
                "404" => return Ok(None),
                _ => return Err(Error::ReleaseCheckFailed),
            }
        }
        Ok(Some(version))
    }
}

pub(super) fn stable(text: &str) -> Option<Version> {
    let version: Version = text.parse().ok()?;
    (version.pre.is_empty() && version.build.is_empty()).then_some(version)
}

fn curl(args: &[&str]) -> Result<String> {
    let mut command = Command::new("/usr/bin/curl");
    command
        .args([
            "-q",
            "--silent",
            "--head",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "4",
            "--max-time",
            "8",
            "--max-redirs",
            "5",
        ])
        .args(args);
    let output = bounded_command::run(command, Duration::from_secs(10), 4096)
        .ok()
        .filter(|out| out.status.success())
        .ok_or(Error::ReleaseCheckFailed)?;
    String::from_utf8(output.stdout).map_err(|_| Error::ReleaseCheckFailed)
}
