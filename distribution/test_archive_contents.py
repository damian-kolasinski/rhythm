import io
import tarfile
import tempfile
import unittest
from pathlib import Path

import archive_contents


class ArchiveBoundary(unittest.TestCase):
    def test_extra_files_links_and_traversal_are_rejected(self):
        for name, kind in [
            ("../private", tarfile.REGTYPE),
            ("rhythm", tarfile.SYMTYPE),
            ("unexpected", tarfile.REGTYPE),
        ]:
            with (
                self.subTest(name=name, kind=kind),
                tempfile.TemporaryDirectory() as directory,
            ):
                path = Path(directory) / "archive.tar.gz"
                with tarfile.open(path, "w:gz") as archive:
                    item = tarfile.TarInfo(name)
                    item.type = kind
                    item.linkname = "elsewhere"
                    archive.addfile(item, io.BytesIO())
                with self.assertRaises(ValueError):
                    archive_contents.read(path)
