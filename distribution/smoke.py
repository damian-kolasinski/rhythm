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
            service = {
                "id": "sample-distributed-service",
                "command": ["/bin/sh", "-c", "printf distributed-service"],
                "working_directory": "/tmp",
                "restart": "never",
            }
            schedule = {
                "id": "sample-distributed-schedule",
                "command": ["/bin/sh", "-c", "printf distributed-schedule"],
                "working_directory": "/tmp",
                "trigger": {"type": "interval", "every_ms": 1000},
                "overlap": "skip",
                "missed": "skip",
            }
            for name, definition in [("service", service), ("schedule", schedule)]:
                file = Path(directory) / (name + ".json")
                file.write_text(json.dumps(definition))
                file.chmod(0o600)
                cli(binary, root, name, "create", "--file", str(file))
            cli(binary, root, "service", "start", service["id"])
            service_history = wait_for(
                lambda: cli(binary, root, "service", "history", service["id"]),
                lambda value: (
                    bool(value["executions"])
                    and value["executions"][0]["outcome"] == {"type": "exited", "code": 0}
                ),
            )
            cli(binary, root, "schedule", "run-now", schedule["id"])
            scheduled = wait_for(
                lambda: cli(binary, root, "schedule", "history", schedule["id"]),
                lambda value: (
                    bool(value["response"]["executions"])
                    and value["response"]["executions"][0]["outcome"]
                    == {"type": "exited", "code": 0}
                ),
            )
            ids = [
                service_history["executions"][0]["id"],
                scheduled["response"]["executions"][0]["id"],
            ]
            if ids[0] == ids[1]:
                raise RuntimeError("execution IDs are not shared")
            for execution, expected in zip(
                ids, ["distributed-service", "distributed-schedule"], strict=True
            ):
                if cli(binary, root, "service", "logs", str(execution))["stdout"] != expected:
                    raise RuntimeError("distributed output mismatch")
            stop()
            start()
            if (
                cli(binary, root, "service", "history", service["id"])["executions"][0]["id"]
                != ids[0]
            ):
                raise RuntimeError("service history did not persist")
            if (
                cli(binary, root, "schedule", "history", schedule["id"])["response"]["executions"][
                    0
                ]["id"]
                != ids[1]
            ):
                raise RuntimeError("schedule history did not persist")
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
                lambda value: len(value["response"]["executions"]) >= 2,
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
        "services": True,
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
