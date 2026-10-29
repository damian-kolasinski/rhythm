"""The release archive contract: fixed regular files, with one root executable."""

import tarfile

NAMES = {"rhythm", "README.txt"}


def read(archive):
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        if (
            len(members) != len(NAMES)
            or {entry.name for entry in members} != NAMES
            or any(not entry.isfile() for entry in members)
        ):
            raise ValueError("unexpected release archive contents")
        if any(entry.size > 100 * 1024 * 1024 for entry in members):
            raise ValueError("release archive entry too large")
        return {entry.name: tar.extractfile(entry).read() for entry in members}


def extract(archive, directory):
    contents = read(archive)
    directory.mkdir(parents=True, exist_ok=True)
    for name, data in contents.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(0o755 if name == "rhythm" else 0o644)
    return directory / "rhythm"
