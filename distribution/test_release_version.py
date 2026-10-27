import unittest

from release_version import metadata


class ReleaseVersion(unittest.TestCase):
    def test_prerelease_flag_excludes_build_metadata(self):
        for value, expected in [
            ("1.2.3", "false"),
            ("1.2.3+build-1", "false"),
            ("1.2.3-rc.1+build", "true"),
        ]:
            with self.subTest(value=value):
                self.assertEqual(metadata(value, "v" + value)["prerelease"], expected)

    def test_matching_tag_and_pr_candidate_share_metadata(self):
        self.assertEqual(metadata("1.2.3", "v1.2.3"), metadata("1.2.3"))
        self.assertEqual(metadata("1.2.3")["tag"], "v1.2.3")

    def test_mismatched_or_malformed_tag_is_refused(self):
        for tag in ["v9.9.9", "1.2.3", "v1.2.3-rc.1", "v1.2.3\n"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                metadata("1.2.3", tag)
