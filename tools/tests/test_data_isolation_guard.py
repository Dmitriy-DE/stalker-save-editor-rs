from __future__ import annotations

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_data_isolation import main, snapshot_tree


class DataIsolationGuardTests(unittest.TestCase):
    def test_command_that_writes_to_real_data_root_fails_the_guard(self) -> None:
        with tempfile.TemporaryDirectory(prefix="sse-data-isolation-guard-") as temporary:
            root = Path(temporary)
            home = root / "home"
            local_app_data = root / "local-app-data"
            real_data_root = (
                local_app_data / "StalkerSaveEditor"
                if os.name == "nt"
                else home / ".local" / "share" / "StalkerSaveEditor"
            )
            real_data_root.mkdir(parents=True)

            def write_to_real_data(command: list[str], *, env: dict[str, str], check: bool) -> subprocess.CompletedProcess[str]:
                del env, check
                (real_data_root / "settings.json").write_text("{}", encoding="utf-8")
                return subprocess.CompletedProcess(command, 0)

            error_output = io.StringIO()
            environment = {
                "HOME": str(home),
                "XDG_DATA_HOME": "",
                "LOCALAPPDATA": str(local_app_data),
                "USERPROFILE": str(home),
            }
            with (
                mock.patch.dict(os.environ, environment, clear=True),
                mock.patch.object(sys, "argv", ["check_data_isolation.py", "--", "fake-command"]),
                mock.patch("check_data_isolation.subprocess.run", side_effect=write_to_real_data),
                contextlib.redirect_stderr(error_output),
            ):
                result = main()

            self.assertEqual(result, 1)
            self.assertIn("settings.json", error_output.getvalue())

    def test_snapshot_detects_new_and_modified_files(self) -> None:
        with tempfile.TemporaryDirectory(prefix="sse-data-isolation-test-") as temporary:
            root = Path(temporary) / "app-data"
            root.mkdir()
            before = snapshot_tree(root)

            settings = root / "settings.json"
            settings.write_text("first", encoding="utf-8")
            after_create = snapshot_tree(root)
            self.assertNotEqual(before, after_create)

            settings.write_text("second value", encoding="utf-8")
            after_update = snapshot_tree(root)
            self.assertNotEqual(after_create, after_update)


if __name__ == "__main__":
    unittest.main()
