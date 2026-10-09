import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import generate_strings


class GenerateStringsTests(unittest.TestCase):
    def test_partial_external_catalog_keeps_checked_in_rust_keys(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tools = root / "tools"
            rust_catalogs = root / "crates/sse-catalog/i18n"
            external_catalogs = root / "csharp/i18n"
            output_directory = root / "crates/sse-ui/src"
            tools.mkdir(parents=True)
            rust_catalogs.mkdir(parents=True)
            external_catalogs.mkdir(parents=True)
            output_directory.mkdir(parents=True)
            shutil.copyfile(TOOLS / "generate_strings.py", tools / "generate_strings.py")
            (tools / "i18n-extra.json").write_text("{}", encoding="utf-8")

            for language in generate_strings.LANGS:
                (rust_catalogs / f"{language}.json").write_text(
                    json.dumps(
                        {
                            "fallback-only": f"fallback {language}",
                            "shared": f"Rust {language}",
                        }
                    ),
                    encoding="utf-8",
                )
                (external_catalogs / f"{language}.json").write_text(
                    json.dumps(
                        {
                            "external-only": f"external {language}",
                            "shared": f"CSharp {language}",
                        }
                    ),
                    encoding="utf-8",
                )

            result = subprocess.run(
                [sys.executable, str(tools / "generate_strings.py"), str(root / "csharp")],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            )
            generated = (root / "crates/sse-ui/src/strings.rs").read_text(encoding="utf-8")

        self.assertIn('("fallback-only", [', generated)
        self.assertIn('"fallback uk"', generated)
        self.assertIn('("external-only", [', generated)
        self.assertIn('"external uk"', generated)
        shared_row = next(line for line in generated.splitlines() if line.startswith('    ("shared",'))
        self.assertIn('"CSharp uk"', shared_row)
        self.assertNotIn('"Rust uk"', shared_row)
        self.assertIn("generated", result.stdout)


if __name__ == "__main__":
    unittest.main()
