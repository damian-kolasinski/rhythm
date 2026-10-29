"""Exercise an installed executable through its CLI against disposable launchd definitions."""

import argparse
import json
import os
import secrets
import subprocess
import tempfile
from pathlib import Path


def cli(binary, home, *arguments, ok=True):
    result = subprocess.run(
        [str(binary), "--json", *arguments],
        capture_output=True,
        text=True,
        timeout=30,
        env={**os.environ, "HOME": str(home)},
    )
    if (result.returncode == 0) != ok:
        raise RuntimeError("installed CLI request did not match the expected outcome")
    return json.loads(result.stdout or result.stderr)


def definition(label, body):
    return (
        '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>'
        f"<key>Label</key><string>{label}</string>{body}</dict></plist>"
    )


def launchctl(*arguments):
    return subprocess.run(["/bin/launchctl", *arguments], capture_output=True).returncode == 0


def verify(binary, release_version, supervised=False):
    binary = binary.resolve(strict=True)
    if (
        subprocess.check_output([str(binary), "--version"], text=True).strip()
        != f"rhythm {release_version}"
    ):
        raise RuntimeError("installed executable version mismatch")
    with tempfile.TemporaryDirectory(prefix="rhythm-distributed-") as directory:
        home = Path(directory).resolve()
        agents = home / "Library/LaunchAgents"
        agents.mkdir(parents=True)
        suffix = secrets.token_hex(6)
        unloaded = f"dev.rhythm.smoke.unloaded.{suffix}"
        (agents / f"{unloaded}.plist").write_text(
            definition(
                unloaded,
                "<key>Program</key><string>/usr/bin/true</string>"
                "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>9</integer>"
                "<key>Minute</key><integer>15</integer></dict>",
            )
        )
        (agents / "broken.plist").write_text("{not a plist")
        jobs = {job["label"]: job for job in cli(binary, home, "status")["hosts"][0]["jobs"]}
        if (
            jobs[unloaded]["health"] != "unloaded"
            or jobs[unloaded]["activation"]["triggers"][0]["hour"] != 9
            or jobs["broken"]["health"] != "invalid"
        ):
            raise RuntimeError("installed status did not read definitions")
        state = home / "state"
        result = subprocess.run(
            [
                str(binary),
                "--data-dir",
                str(state),
                "run",
                unloaded,
                "--",
                "/bin/sh",
                "-c",
                "echo distributed-run; exit 2",
            ],
            capture_output=True,
            timeout=30,
        )
        if result.returncode != 2 or result.stdout or result.stderr:
            raise RuntimeError("installed recorder did not exit like its command")
        runs = cli(binary, home, "--data-dir", str(state), "history", unloaded)["runs"]
        logs = cli(binary, home, "--data-dir", str(state), "logs", unloaded)
        if (
            len(runs) != 1
            or runs[0]["outcome"] != {"type": "code", "code": 2}
            or logs["output"] != "distributed-run\n"
        ):
            raise RuntimeError("installed recorder did not record the run")
        if (
            cli(binary, home, "status", "--host", "-oProxyCommand=x", ok=False)["error"]["code"]
            != "invalid_request"
        ):
            raise RuntimeError("installed status accepted an ssh option as a host")
        if supervised:
            user = os.getuid()
            domain = f"gui/{user}" if launchctl("print", f"gui/{user}") else f"user/{user}"
            session = "Aqua" if domain.startswith("gui") else "Background"
            failing = f"dev.rhythm.smoke.failing.{suffix}"
            plist = agents / f"{failing}.plist"
            plist.write_text(
                definition(
                    failing,
                    "<key>ProgramArguments</key><array><string>/bin/sh</string>"
                    "<string>-c</string><string>exit 3</string></array>"
                    f"<key>RunAtLoad</key><true/><key>LimitLoadToSessionType</key>"
                    f"<string>{session}</string>",
                )
            )
            if not launchctl("bootstrap", domain, str(plist)):
                raise RuntimeError("disposable agent could not be registered")
            try:
                for _ in range(100):
                    job = next(
                        j
                        for j in cli(binary, home, "status")["hosts"][0]["jobs"]
                        if j["label"] == failing
                    )
                    if job["health"] == "failing":
                        break
                    subprocess.run(["/bin/sleep", "0.05"], check=True)
                if job["observation"].get("last_exit") != {"type": "code", "code": 3}:
                    raise RuntimeError("installed status did not observe a launchd failure")
            finally:
                launchctl("bootout", f"{domain}/{failing}")
            name = f"smoke-{suffix}"
            cli(
                binary,
                home,
                "--data-dir",
                str(state),
                "add",
                name,
                "--",
                "/bin/sh",
                "-c",
                "echo distributed-add; exit 6",
            )
            try:
                kicked = cli(
                    binary,
                    home,
                    "--data-dir",
                    str(state),
                    "kick",
                    name,
                    "--wait",
                    "--timeout",
                    "30",
                    ok=False,
                )
            finally:
                cli(binary, home, "--data-dir", str(state), "remove", name)
            if (
                kicked.get("finished", {}).get("exit") != {"type": "code", "code": 6}
                or kicked["finished"].get("output") != "distributed-add\n"
            ):
                raise RuntimeError("installed add and kick did not record a real launchd run")
    return {
        "version": release_version,
        "cli": True,
        "status": True,
        "recording": True,
        "authoring": supervised,
        "supervision": supervised,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--launchd", action="store_true")
    args = parser.parse_args()
    print(json.dumps(verify(args.binary, args.version, args.launchd)))
