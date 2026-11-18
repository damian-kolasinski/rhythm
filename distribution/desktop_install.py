"""Open and uninstall the real packaged app in an isolated CI home."""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

import archive_contents


def run(args, env, home, timeout=90):
    result = subprocess.run(args, cwd=home, env=env, capture_output=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"desktop rehearsal command failed with exit {result.returncode}")
    return result.stdout


def running(executable):
    listing = subprocess.check_output(["/bin/ps", "-axww", "-o", "command="], text=True)
    return any(
        line == str(executable) or line.startswith(f"{executable} ")
        for line in listing.splitlines()
    )


def verify(home, cli, env, installed, expected_mise):
    if installed["window"] != "opened":
        raise RuntimeError("desktop window did not acknowledge its first frame")
    bundle = home / "Applications/Rhythm.app"
    executable = bundle / "Contents/MacOS/rhythm-ui"
    if not running(executable):
        raise RuntimeError("the installed window is not running")
    run(["/usr/bin/codesign", "--verify", "--strict", bundle], env, home)
    link = home / ".local/bin/rhythm"
    if run([link, "--version"], env, home).decode().strip() != f"rhythm {installed['version']}":
        raise RuntimeError("the installed CLI does not match the app")
    removed = json.loads(run([cli, "--json", "uninstall"], env, home))
    if os.path.lexists(link) or bundle.exists() or removed["mise"]["state"] != expected_mise:
        raise RuntimeError("uninstall left a desktop installation behind")
    if expected_mise == "removed" and not removed["mise"]["pruned"]:
        raise RuntimeError("uninstall did not prune the mise release")
    deadline = time.monotonic() + 20
    while running(executable) and time.monotonic() < deadline:
        time.sleep(0.25)
    if running(executable):
        raise RuntimeError("the uninstalled window did not close")


def rehearse(archive, installer):
    if os.environ.get("CI") != "true":
        raise RuntimeError("desktop installation rehearsal runs only on CI")
    with tempfile.TemporaryDirectory(prefix="rhythm-desktop-") as scratch:
        root = Path(scratch).resolve()
        home = root / "home"
        home.mkdir(mode=0o700)
        env = {key: value for key, value in os.environ.items() if not key.startswith("MISE_")}
        env.update(
            HOME=str(home),
            XDG_CONFIG_HOME=str(home / ".config"),
            XDG_CACHE_HOME=str(home / ".cache"),
            XDG_STATE_HOME=str(home / ".state"),
            XDG_DATA_HOME=str(home / ".data"),
            RHYTHM_MISE=shutil.which("mise") or "",
            MISE_YES="1",
        )
        if installer:
            installed = json.loads(run(["/bin/sh", installer], env, home, timeout=180))
            cli = home / ".local/bin/rhythm"
            verify(home, cli, env, installed, "removed")
        else:
            cli = archive_contents.extract(archive, root / "archive")
            installed = json.loads(run([cli, "--json", "install", "--open"], env, home))
            verify(home, cli, env, installed, "absent")
    print("Desktop install, first frame, and uninstall verified.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--archive", type=Path)
    group.add_argument("--installer", type=Path)
    args = parser.parse_args()
    rehearse(args.archive, args.installer)
