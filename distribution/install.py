"""Install the published release through packslip-backed mise, then verify the installed bytes and flows."""

import argparse
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

import archive_contents
import package
import smoke

ISSUER = "https://token.actions.githubusercontent.com"


def mise_configuration(project, release_version, identity):
    return (
        "[tools]\n"
        + json.dumps("packslip:" + project)
        + " = { version = "
        + json.dumps(release_version)
        + ", identity = "
        + json.dumps(identity)
        + ", issuer = "
        + json.dumps(ISSUER)
        + " }\n"
    )


def command(arguments, cwd=None, env=None):
    result = subprocess.run(
        arguments, cwd=cwd, env=env, capture_output=True, text=True, timeout=180
    )
    if result.returncode:
        raise RuntimeError(f"{arguments[0]} {arguments[1]} failed (exit {result.returncode})")
    return result.stdout


def validate_statement(statement, project, release_version, commit):
    release = statement.get("predicate", {})
    if (
        release.get("project") != project
        or release.get("version") != release_version
        or release.get("source", {}).get("commit") != commit
    ):
        raise RuntimeError("published release source identity mismatch")


def verify(repository, release_version, commit, browser=False, supervised=False):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) or not re.fullmatch(
        r"[0-9a-f]{40}", commit
    ):
        raise ValueError("invalid release identity")
    archive_name = package.filename(release_version)
    project = "github.com/" + repository
    identity = f"https://github.com/{repository}/.github/workflows/release.yml@refs/tags/v{release_version}"
    with tempfile.TemporaryDirectory(prefix="rhythm-mise-") as directory:
        root = Path(directory).resolve(strict=True)
        assets = root / "assets"
        assets.mkdir()
        command(
            [
                "gh",
                "release",
                "download",
                "v" + release_version,
                "--repo",
                repository,
                "--dir",
                str(assets),
                "--pattern",
                archive_name,
                "--pattern",
                "packslip.sigstore.json",
            ]
        )
        bundle = assets / "packslip.sigstore.json"
        archive = assets / archive_name
        command(
            [
                "packslip",
                "verify",
                str(bundle),
                "--identity",
                identity,
                "--issuer",
                ISSUER,
                "--artifact",
                str(archive),
            ]
        )
        statement = json.loads(command(["packslip", "show", str(bundle)]))
        validate_statement(statement, project, release_version, commit)
        configuration = root / "mise.toml"
        configuration.write_text(mise_configuration(project, release_version, identity))
        configuration.chmod(0o600)
        empty = root / "empty.toml"
        empty.write_text("")
        env = os.environ.copy()
        for key in list(env):
            if key.startswith("MISE_") and key not in {"MISE_GITHUB_TOKEN"}:
                env.pop(key)
        for key, path in {
            "MISE_DATA_DIR": root / "data",
            "MISE_CACHE_DIR": root / "cache",
            "MISE_STATE_DIR": root / "state",
            "MISE_CONFIG_DIR": root / "config",
            "MISE_GLOBAL_CONFIG_FILE": empty,
            "MISE_CONFIG_FILE": configuration,
        }.items():
            env[key] = str(path)
        env["MISE_YES"] = "1"
        command(["mise", "trust", str(configuration)], cwd=root, env=env)
        command(["mise", "install"], cwd=root, env=env)
        binary = Path(command(["mise", "which", "rhythm"], cwd=root, env=env).strip()).resolve(
            strict=True
        )
        if (
            not binary.is_relative_to(root / "data")
            or not (binary.parent / ".mise-packslip.json").is_file()
        ):
            raise RuntimeError("mise did not install a verified release")
        if (
            hashlib.sha256(binary.read_bytes()).digest()
            != hashlib.sha256(archive_contents.read(archive)["rhythm"]).digest()
        ):
            raise RuntimeError("installed bytes differ from verified archive")
        report = smoke.verify(binary, release_version, supervised)
        if browser:
            test_env = os.environ.copy()
            test_env["RHYTHM_BIN"] = str(binary)
            subprocess.run(
                ["pnpm", "--dir", str(package.ROOT / "web"), "test"],
                env=test_env,
                check=True,
            )
        return {
            **report,
            "signature_verified": True,
            "mise_install": True,
            "browser": browser,
            "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--browser", action="store_true")
    parser.add_argument("--launchd", action="store_true")
    args = parser.parse_args()
    print(
        json.dumps(verify(args.repository, args.version, args.commit, args.browser, args.launchd))
    )
