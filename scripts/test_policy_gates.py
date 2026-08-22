"""Regression tests for architecture and source-growth policy boundaries."""

from pathlib import Path
import tempfile
import unittest

from scripts.check_architecture import (
    architecture_violations,
    desktop_violations,
    override_violations,
)
from scripts.check_source_growth import (
    SourceGrowthPolicy,
    SourceMetric,
    _tracked_source,
    violations,
)


def dependency(name: str, path: str | None = None) -> dict[str, object]:
    return {"name": name, "path": path}


def metadata_payload(packages: list[dict[str, object]]) -> dict[str, object]:
    normalized = []
    for package in packages:
        item = dict(package)
        item.setdefault("id", f"path+file:///fixture#{item['name']}@0.1.0")
        normalized.append(item)
    members = [str(package["id"]) for package in normalized]
    return {
        "packages": normalized,
        "workspace_members": members,
        "resolve": {
            "nodes": [
                {"id": package["id"], "dependencies": []}
                for package in normalized
            ]
        },
    }


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
            architecture_violations(metadata_payload(packages)),
        )

    def test_desktop_rejects_unapproved_local_path_shim(self) -> None:
        payload = metadata_payload([
            {
                "name": "agentsassemble-desktop",
                "dependencies": [dependency("local-path-shim", "/tmp/shim")],
            }
        ])

        self.assertIn(
            "desktop imports unapproved local path dependency local-path-shim",
            desktop_violations(payload),
        )

    def test_repository_patch_override_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = []\n[patch.crates-io]\nserde = { path = "shim" }\n',
                encoding="utf-8",
            )
            self.assertIn(
                "Cargo.toml contains forbidden [patch] source overrides",
                override_violations(root),
            )


class SourceGrowthPolicyTests(unittest.TestCase):
    def test_generated_prefix_does_not_bypass_source_counting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "generatedOversized.rs"
            source.write_text("fn main() {}\n", encoding="utf-8")
            self.assertTrue(_tracked_source(source))

    def test_compressed_one_line_source_hits_logical_line_limit(self) -> None:
        policy = SourceGrowthPolicy(
            new_file_line_limit=800,
            new_file_byte_limit=262_144,
            max_logical_line_bytes=16_384,
            file_limits={},
        )
        found = violations(
            {
                "frontend/src/compressed.ts": SourceMetric(
                    lines=1,
                    bytes=20_000,
                    max_logical_line_bytes=20_000,
                )
            },
            policy,
        )
        self.assertIn(
            "frontend/src/compressed.ts: 20000-byte logical line exceeds the limit of 16384",
            found,
        )


if __name__ == "__main__":
    unittest.main()
