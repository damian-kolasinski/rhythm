"""Build and package the supported release target without local path metadata."""

import argparse
import gzip
import io
import os
import re
import subprocess
import tarfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = "aarch64-apple-darwin"
NUMBER = r"(?:0|[1-9]\d*)"
PRERELEASE = rf"(?:{NUMBER}|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    rf"{NUMBER}\.{NUMBER}\.{NUMBER}(?:-{PRERELEASE}(?:\.{PRERELEASE})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def version(root=ROOT):
    value = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if not isinstance(value, str) or not SEMVER.fullmatch(value):
        raise ValueError("invalid release version")
    return value


def filename(release_version, target=TARGET):
    if not SEMVER.fullmatch(release_version) or target != TARGET:
        raise ValueError("unsupported release identity")
    return f"rhythm-v{release_version}-{target}.tar.gz"


def build(root=ROOT, target=TARGET):
    if target != TARGET:
        raise ValueError("unsupported release target")
    if (
        subprocess.check_output(["rustc", "-vV"], text=True).split("host: ")[1].splitlines()[0]
        != target
    ):
        raise ValueError("release verification requires a native target")
    env = os.environ.copy()
    # Path remapping covers this checkout, dependencies, and toolchain sources.
    mappings = [
        (Path.home(), "/build/home"),
        (root, "/build/rhythm"),
        (Path(env.get("CARGO_HOME", Path.home() / ".cargo")), "/build/cargo"),
        (Path(env.get("RUSTUP_HOME", Path.home() / ".rustup")), "/build/rustup"),
    ]
    env.pop("RUSTFLAGS", None)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
        f"--remap-path-prefix={a}={b}" for a, b in mappings
    )
    env["MACOSX_DEPLOYMENT_TARGET"] = "13.0"
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "--target", target],
        cwd=root,
        env=env,
        check=True,
    )
    return root / "target" / target / "release" / "rhythm"


def archive(binary, output, release_version, target=TARGET):
    name = filename(release_version, target)
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("release executable unavailable")
    binary = binary.resolve(strict=True)
    if (
        subprocess.check_output([str(binary), "--version"], text=True).strip()
        != f"rhythm {release_version}"
    ):
        raise ValueError("release version mismatch")
    executable = binary.read_bytes()
    if str(Path.home()).encode() in executable:
        raise ValueError("release executable contains a private build path")
    output.mkdir(parents=True, exist_ok=True)
    members = {
        "rhythm": (executable, 0o755),
        "README.txt": (
            b"Rhythm\n\nRun rhythm --help or rhythm status.\nInstall versioned, signed releases with mise's packslip backend.\n",
            0o644,
        ),
    }
    path = output / name
    with (
        path.open("wb") as raw,
        gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed,
    ):
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for member_name, (data, mode) in members.items():
                item = tarfile.TarInfo(member_name)
                item.size = len(data)
                item.mode = mode
                item.mtime = item.uid = item.gid = 0
                item.uname = item.gname = ""
                tar.addfile(item, io.BytesIO(data))
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    binary = build() if args.build else args.binary
    if binary is None:
        parser.error("choose --build or --binary")
    result = archive(binary, args.out, version())
    print(result.name)


if __name__ == "__main__":
    main()
