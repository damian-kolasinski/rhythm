import unittest

from plan import decision, metadata


class ReleaseDecisions(unittest.TestCase):
    def test_prerelease_flag_excludes_build_metadata(self):
        for value, expected in [
            ("1.2.3", "false"),
            ("1.2.3+build-1", "false"),
            ("1.2.3-rc.1+build", "true"),
        ]:
            with self.subTest(value=value):
                self.assertEqual(metadata(value)["prerelease"], expected)

    def test_new_version_can_publish(self):
        self.assertTrue(decision(None, None, "a" * 40))
        self.assertTrue(decision(None, "a" * 40, "a" * 40))

    def test_published_version_is_immutable(self):
        self.assertFalse(decision({"draft": False}, None, "a" * 40))

    def test_partial_release_and_conflicting_tag_need_investigation(self):
        with self.assertRaises(RuntimeError):
            decision({"draft": True}, None, "a" * 40)
        with self.assertRaises(RuntimeError):
            decision(None, "b" * 40, "a" * 40)
