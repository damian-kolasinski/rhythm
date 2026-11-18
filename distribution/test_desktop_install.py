import unittest

from desktop_install import failure_reason


class DesktopRehearsalDiagnostics(unittest.TestCase):
    def test_reports_the_window_result_and_failure(self):
        stdout = b'{"window":"failed","window_error":"window_launch_failed","path":"private"}'
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
