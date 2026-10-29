import hashlib
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package


class Packaging(unittest.TestCase):
    def test_archive_has_stable_metadata_and_root_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "rhythm"
            binary.write_bytes(b"synthetic release executable")
            binary.chmod(0o755)
            with patch("package.subprocess.check_output", return_value="rhythm 1.2.3\n"):
                first = package.archive(binary, root / "one", "1.2.3")
                second = package.archive(binary, root / "two", "1.2.3")
            self.assertEqual(
                hashlib.sha256(first.read_bytes()).digest(),
                hashlib.sha256(second.read_bytes()).digest(),
            )
            with tarfile.open(first) as archive:
                self.assertEqual(
                    archive.getnames(),
                    ["rhythm", "README.txt"],
                )
                for member in archive:
                    self.assertTrue(member.isfile())
                    self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, 0))
                    self.assertEqual((member.uname, member.gname), ("", ""))
                self.assertEqual(archive.getmember("rhythm").mode, 0o755)
                self.assertEqual(archive.extractfile("rhythm").read(), binary.read_bytes())

    def test_invalid_versions_and_targets_cannot_become_paths(self):
        for value in ["../sample", "1.2", "1.2.3\n", "01.2.3", "1.2.3-01"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                package.filename(value)
        with self.assertRaises(ValueError):
            package.filename("1.2.3", "unverified-target")

    def test_mismatched_version_or_private_paths_are_not_packaged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "rhythm"
            binary.write_bytes(b"synthetic")
            binary.chmod(0o755)
            with (
                patch("package.subprocess.check_output", return_value="rhythm 9.9.9\n"),
                self.assertRaises(ValueError),
            ):
                package.archive(binary, root / "out", "1.2.3")
            binary.write_bytes(str(Path.home()).encode())
            with (
                patch("package.subprocess.check_output", return_value="rhythm 1.2.3\n"),
                self.assertRaises(ValueError),
            ):
                package.archive(binary, root / "out", "1.2.3")
            self.assertFalse((root / "out").exists())

    def test_symlink_executable_is_not_packaged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "rhythm"
            link = root / "link"
            binary.write_bytes(b"synthetic")
            binary.chmod(0o755)
            link.symlink_to(binary)
            with self.assertRaises(ValueError):
                package.archive(link, root / "out", "1.2.3")


if __name__ == "__main__":
    unittest.main()
