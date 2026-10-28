"""Exercise an installed executable through its CLI, persisted state and embedded HTTP UI."""

import argparse
import json
import os
import plistlib
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path


def cli(binary, root, *arguments):
    result = subprocess.run(
        [str(binary), "--json", "--data-dir", str(root), *arguments],
        capture_output=True,
        text=True,
        timeout=15,
    )
    if result.returncode:
        raise RuntimeError("installed CLI request failed")
    return json.loads(result.stdout)


def wait_for(read, accept, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = read()
            if accept(value):
                return value
        except (OSError, RuntimeError, json.JSONDecodeError):
            pass
        time.sleep(0.05)
    raise RuntimeError("installed runtime did not reach expected state")


def verify(binary, release_version, supervised=False):
    binary = binary.resolve(strict=True)
    if (
        subprocess.check_output([str(binary), "--version"], text=True).strip()
        != f"rhythm {release_version}"
    ):
        raise RuntimeError("installed executable version mismatch")
    with tempfile.TemporaryDirectory(prefix="rhythm-distributed-") as directory:
        root = (Path(directory) / "state").resolve()
        process = None

        def start():
            nonlocal process
            if supervised:
                cli(binary, root, "runtime", "install")
            else:
                process = subprocess.Popen(
                    [str(binary), "--data-dir", str(root), "runtime", "run"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            wait_for(
                lambda: cli(binary, root, "runtime", "status"),
                lambda value: value["type"] == "runtime",
            )

        def stop():
            nonlocal process
            cli(binary, root, "runtime", "stop")
            if process is not None:
                process.wait(timeout=8)
                process = None

        try:
            start()
            if supervised:
                import hashlib

                label = "dev.rhythm.runtime." + hashlib.sha256(os.fsencode(root)).hexdigest()
                registration = Path.home() / "Library/LaunchAgents" / (label + ".plist")
                with registration.open("rb") as file:
                    definition = plistlib.load(file)
                if definition["ProgramArguments"][0] != str(binary):
                    raise RuntimeError("registration does not use installed executable")
            schedules = [
                {
                    "id": "sample-distributed-report",
                    "command": ["/bin/sh", "-c", "printf distributed-report"],
                    "working_directory": "/tmp",
                    "trigger": {"type": "interval", "every_ms": 1000},
                    "overlap": "skip",
                    "missed": "skip",
                },
                {
                    "id": "sample-distributed-cleanup",
                    "command": ["/bin/sh", "-c", "printf distributed-cleanup"],
                    "working_directory": "/tmp",
                    "trigger": {"type": "interval", "every_ms": 1000},
                    "overlap": "skip",
                    "missed": "skip",
                },
            ]
            ids = []
            for definition, expected in zip(
                schedules, ["distributed-report", "distributed-cleanup"], strict=True
            ):
                file = Path(directory) / (definition["id"] + ".json")
                file.write_text(json.dumps(definition))
                file.chmod(0o600)
                cli(binary, root, "schedule", "create", "--file", str(file))
                cli(binary, root, "schedule", "run-now", definition["id"])
                history = wait_for(
                    lambda schedule_id=definition["id"]: cli(
                        binary, root, "schedule", "history", schedule_id
                    ),
                    lambda value: (
                        bool(value["executions"])
                        and value["executions"][0]["outcome"] == {"type": "exited", "code": 0}
                    ),
                )
                execution = history["executions"][0]
                if (
                    execution["schedule_id"] != definition["id"]
                    or execution["invocation"]["source"] != "manual"
                ):
                    raise RuntimeError("schedule execution ownership mismatch")
                ids.append(execution["id"])
                if (
                    cli(binary, root, "schedule", "logs", str(execution["id"]))["stdout"]
                    != expected
                ):
                    raise RuntimeError("distributed output mismatch")
            if ids[0] == ids[1]:
                raise RuntimeError("schedule execution IDs are not shared")
            stop()
            start()
            for definition, execution in zip(schedules, ids, strict=True):
                if (
                    cli(binary, root, "schedule", "history", definition["id"])["executions"][0][
                        "id"
                    ]
                    != execution
                ):
                    raise RuntimeError("schedule history did not persist")
            schedule = schedules[0]
            launch = cli(binary, root, "ui", "--no-open")
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            for path, expected in [
                ("/", b"<html"),
                ("/assets/app.js", b""),
                ("/assets/app.css", b"DM Sans"),
                ("/assets/dm-sans.woff2", b"wOF2"),
            ]:
                with opener.open(launch["url"].rstrip("/") + path, timeout=5) as response:
                    body = response.read()
                    if (
                        response.status != 200
                        or expected not in body
                        or response.headers["Cache-Control"] != "no-store"
                    ):
                        raise RuntimeError("embedded distributed asset unavailable")
            cli(binary, root, "schedule", "resume", schedule["id"])
            wait_for(
                lambda: cli(binary, root, "schedule", "history", schedule["id"]),
                lambda value: len(value["executions"]) >= 2,
            )
            cli(binary, root, "schedule", "pause", schedule["id"])
        finally:
            try:
                stop()
            finally:
                if process is not None:
                    process.kill()
                    process.wait()
    return {
        "version": release_version,
        "cli": True,
        "schedules": True,
        "persistence": True,
        "embedded_ui": True,
        "supervision": supervised,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--launchd", action="store_true")
    args = parser.parse_args()
    print(json.dumps(verify(args.binary, args.version, args.launchd)))
