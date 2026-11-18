import unittest

from desktop_install import failure_reason, read_report


class DesktopRehearsalDiagnostics(unittest.TestCase):
    def test_reads_the_final_installer_report_after_mise_progress(self):
        report = b'{\n  "window": "opened",\n  "version": "1.2.3"\n}\n'
        output = b"mise ~/.config/mise/config.toml tools: example@1.2.3\n" + report
        self.assertEqual(read_report(output), {"window": "opened", "version": "1.2.3"})

    def test_reports_the_window_result_and_failure(self):
        stdout = (
            b"mise progress\n"
            b'{"window":"failed","window_error":"window_launch_failed","path":"private"}'
        )
        self.assertEqual(
            failure_reason(stdout, b"unstructured log"), "window=failed, window_launch_failed"
        )

    def test_reports_only_known_error_codes(self):
        stderr = b'{"error":{"code":"installation_conflict","message":"private path"}}'
        self.assertEqual(failure_reason(b"", stderr), "installation_conflict")
        self.assertEqual(
            failure_reason(b'{"window":"private path"}', b'{"error":{"code":"private"}}'), ""
        )

    def test_reports_a_background_session(self):
        self.assertEqual(
            failure_reason(b'{"window":"background_session"}', b""), "window=background_session"
        )
