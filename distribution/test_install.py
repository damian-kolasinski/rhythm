import copy
import tomllib
import unittest

from install import mise_configuration, validate_statement


class PublishedIdentity(unittest.TestCase):
    def test_install_policy_pins_version_workflow_and_github_issuer(self):
        identity = "https://github.com/example/rhythm/.github/workflows/release.yml@refs/heads/main"
        config = tomllib.loads(mise_configuration("github.com/example/rhythm", "1.2.3", identity))
        self.assertEqual(
            config["tools"]["packslip:github.com/example/rhythm"],
            {
                "version": "1.2.3",
                "identity": identity,
                "issuer": "https://token.actions.githubusercontent.com",
            },
        )

    def test_expected_verified_statement_is_accepted(self):
        validate_statement(
            {
                "predicate": {
                    "project": "rhythm.invalid",
                    "version": "1.2.3",
                    "source": {"commit": "a" * 40},
                }
            },
            "rhythm.invalid",
            "1.2.3",
            "a" * 40,
        )

    def test_wrong_project_version_or_source_is_refused(self):
        original = {
            "predicate": {
                "project": "rhythm.invalid",
                "version": "1.2.3",
                "source": {"commit": "a" * 40},
            }
        }
        variants = []
        for key, value in [
            ("project", "other.invalid"),
            ("version", "9.9.9"),
            ("source", {"commit": "b" * 40}),
        ]:
            changed = copy.deepcopy(original)
            changed["predicate"][key] = value
            variants.append(changed)
        for statement in [*variants, {}, original["predicate"]]:
            with self.subTest(statement=statement), self.assertRaises(RuntimeError):
                validate_statement(statement, "rhythm.invalid", "1.2.3", "a" * 40)
