"""Release once per reviewed version; refuse partial releases or conflicting tags."""

import argparse
import json
import re
import subprocess

import package


def github(path):
    result = subprocess.run(
        ["gh", "api", path, "--include"], capture_output=True, text=True, timeout=30
    )
    response = result.stdout.split("\n\n", 1)
    status = re.match(r"HTTP/\S+ (\d+)", response[0])
    if status and status[1] == "404":
        return None
    if result.returncode or not status or status[1] != "200" or len(response) != 2:
        raise RuntimeError("cannot determine release state")
    return json.loads(response[1])


def decision(release, tag_commit, commit):
    if release is not None:
        if release.get("draft"):
            raise RuntimeError("incomplete release requires investigation")
        return False
    if tag_commit is not None and tag_commit != commit:
        raise RuntimeError("release tag points to another commit")
    return True


def metadata(release_version):
    package.filename(release_version)
    return {
        "version": release_version,
        "tag": "v" + release_version,
        "prerelease": str("-" in release_version.split("+", 1)[0]).lower(),
    }


def plan(repository, commit):
    release_version = package.version()
    tag = "v" + release_version
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) or not re.fullmatch(
        r"[0-9a-f]{40}", commit
    ):
        raise ValueError("invalid release identity")
    release = github(f"repos/{repository}/releases/tags/{tag}")
    tag_ref = None if release is not None else github(f"repos/{repository}/git/ref/tags/{tag}")
    tag_commit = None
    if tag_ref is not None:
        obj = tag_ref["object"]
        for _ in range(8):
            if obj["type"] == "commit":
                tag_commit = obj["sha"]
                break
            if obj["type"] != "tag":
                raise RuntimeError("invalid release tag")
            obj = github(f"repos/{repository}/git/tags/{obj['sha']}")["object"]
        else:
            raise RuntimeError("release tag nesting is unsupported")
    return {
        **metadata(release_version),
        "publish": str(decision(release, tag_commit, commit)).lower(),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output")
    args = parser.parse_args()
    result = plan(args.repository, args.commit)
    if args.output:
        with open(args.output, "a") as file:
            file.writelines(f"{key}={value}\n" for key, value in result.items())
    print(json.dumps(result))
