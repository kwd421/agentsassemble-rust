"""Regression tests for architecture and source-growth policy boundaries."""

from pathlib import Path
import tempfile
import unittest

from scripts.check_architecture import architecture_violations, desktop_violations
from scripts.check_source_growth import _tracked_source


def dependency(name: str, path: str | None = None) -> dict[str, object]:
    return {"name": name, "path": path}


class ArchitecturePolicyTests(unittest.TestCase):
    def test_workspace_rejects_unapproved_local_path_shim(self) -> None:
        packages = [
            {"name": name, "dependencies": []}
            for name in (
                "agentsassemble-domain",
                "agentsassemble-persistence",
                "agentsassemble-protocol",
            )
        ]
        packages.append(
            {
                "name": "agentsassemble-server",
                "dependencies": [dependency("local-path-shim", "/tmp/shim")],
            }
        )

        self.assertIn(
            "agentsassemble-server imports unapproved local path dependency local-path-shim",
            architecture_violations({"packages": packages}),
        )

    def test_desktop_rejects_unapproved_local_path_shim(self) -> None:
        payload = {
            "packages": [
                {
                    "name": "agentsassemble-desktop",
                    "dependencies": [dependency("local-path-shim", "/tmp/shim")],
                }
            ]
        }

        self.assertIn(
            "desktop imports unapproved local path dependency local-path-shim",
            desktop_violations(payload),
        )


class SourceGrowthPolicyTests(unittest.TestCase):
    def test_generated_prefix_does_not_bypass_source_counting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "generatedOversized.rs"
            source.write_text("fn main() {}\n", encoding="utf-8")
            self.assertTrue(_tracked_source(source))


if __name__ == "__main__":
    unittest.main()
