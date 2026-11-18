import subprocess
import tempfile
import unittest
from pathlib import Path

import stamp_installer


class Installer(unittest.TestCase):
    def test_stamp_is_exact_and_truncation_does_not_install(self):
        template = Path(stamp_installer.__file__).with_name("install.sh").read_text()
        text = stamp_installer.stamp(template, "example/rhythm", "1.2.3")
        self.assertIn("\nrepository=example/rhythm\n", text)
        self.assertIn("\nrelease=1.2.3\n", text)
        self.assertEqual(text.splitlines()[-1], 'main "$@"')
        with self.assertRaises(ValueError):
            stamp_installer.stamp(text, "example/rhythm", "1.2.4")
        with tempfile.TemporaryDirectory() as scratch:
            result = subprocess.run(
                ["/bin/sh"],
                input=text.removesuffix('main "$@"\n'),
                cwd=scratch,
                env={"HOME": scratch, "PATH": "/usr/bin:/bin"},
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.assertEqual(result.returncode, 0)
            self.assertEqual(list(Path(scratch).iterdir()), [])
