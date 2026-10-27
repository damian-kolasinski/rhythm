"""Verify a packaged candidate with disposable signing keys and real installed flows."""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import archive_contents
import package
import smoke


def run(arguments, ok=True):
    result = subprocess.run(arguments, capture_output=True, text=True, timeout=120)
    if (result.returncode == 0) != ok:
        raise RuntimeError("candidate signature verification did not match expected outcome")
    return result


def verify(archive, release_version, browser=False, supervised=False):
    with tempfile.TemporaryDirectory(prefix="rhythm-package-") as directory:
        root = Path(directory)
        key = root / "release.key"
        run(["packslip", "keygen", "--out", str(key)])
        run(
            [
                "packslip",
                "create",
                "--project",
                "rhythm.invalid",
                "--version",
                release_version,
                "--key",
                str(key),
                "--no-log",
                "--out",
                str(root),
                "--url-base",
                f"https://rhythm.invalid/v{release_version}",
                "--bin",
                "rhythm",
                str(archive),
            ]
        )
        bundle = root / "packslip.sigstore.json"
        check = [
            "packslip",
            "verify",
            str(bundle),
            "--pubkey",
            str(root / "release.pub"),
            "--allow-unlogged",
        ]
        run([*check, "--artifact", str(archive)])
        corrupt = root / "corrupt" / archive.name
        corrupt.parent.mkdir()
        shutil.copyfile(archive, corrupt)
        with corrupt.open("ab") as file:
            file.write(b"tampered")
        run([*check, "--artifact", str(corrupt)], ok=False)
        wrong = root / "wrong.key"
        run(["packslip", "keygen", "--out", str(wrong)])
        run(
            [
                "packslip",
                "verify",
                str(bundle),
                "--pubkey",
                str(root / "wrong.pub"),
                "--allow-unlogged",
                "--artifact",
                str(archive),
            ],
            ok=False,
        )
        binary = archive_contents.extract(archive, root / "installed")
        report = smoke.verify(binary, release_version, supervised)
        if browser:
            env = os.environ.copy()
            env["RHYTHM_BIN"] = str(binary)
            subprocess.run(
                ["pnpm", "--dir", str(package.ROOT / "web"), "test"],
                env=env,
                check=True,
            )
        return {
            **report,
            "test_key_signature": True,
            "tamper_rejected": True,
            "wrong_signer_rejected": True,
            "browser": browser,
        }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--version", default=package.version())
    parser.add_argument("--browser", action="store_true")
    parser.add_argument("--launchd", action="store_true")
    args = parser.parse_args()
    print(
        json.dumps(
            verify(
                (args.archive or package.ROOT / "dist" / package.filename(args.version)).resolve(
                    strict=True
                ),
                args.version,
                args.browser,
                args.launchd,
            )
        )
    )
