use std::{path::Path, process::Command};

pub fn write(path: &Path, script: &str) {
    // Keep writable executable descriptors out of the multithreaded test process:
    // another thread's child can inherit one and make exec fail with ETXTBSY.
    let status = Command::new("/bin/sh")
        .args([
            "-c",
            "set -eu; umask 077; printf '%s' \"$2\" > \"$1\"; chmod 700 \"$1\"",
            "fixture-writer",
        ])
        .arg(path)
        .arg(script)
        .status()
        .unwrap();
    assert!(status.success());
}
