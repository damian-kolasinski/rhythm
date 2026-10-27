"""Require a release tag to match the packaged Cargo version."""

import argparse
import json

import package


def metadata(release_version, tag=""):
    package.filename(release_version)
    expected = "v" + release_version
    if tag and tag != expected:
        raise ValueError("release tag does not match Cargo version")
    return {
        "version": release_version,
        "tag": expected,
        "prerelease": str("-" in release_version.split("+", 1)[0]).lower(),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", default="")
    parser.add_argument("--output")
    args = parser.parse_args()
    result = metadata(package.version(), args.tag)
    if args.output:
        with open(args.output, "a") as file:
            file.writelines(f"{key}={value}\n" for key, value in result.items())
    print(json.dumps(result))
