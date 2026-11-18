"""Write the exact release's one-command installer."""

import argparse
import re
from pathlib import Path

import package


def stamp(text, repository, version):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid release repository")
    if not package.SEMVER.fullmatch(version):
        raise ValueError("invalid release version")
    for field, value in [("repository", repository), ("release", version)]:
        marker = f"{field}=unstamped"
        if text.splitlines().count(marker) != 1:
            raise ValueError(f"expected one installer marker for {field}")
        text = text.replace(f"\n{marker}\n", f"\n{field}={value}\n")
    return text


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    source = Path(__file__).with_name("install.sh").read_text()
    args.out.write_text(stamp(source, args.repository, args.version))
