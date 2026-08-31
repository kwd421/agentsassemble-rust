"""Regression tests for architecture and source-growth policy boundaries."""

from pathlib import Path
import tempfile
import unittest

from scripts.check_architecture import (
    architecture_violations,
    desktop_violations,
    override_violations,
    resolve_graph_violations,
)
from scripts.check_source_growth import (
    SourceGrowthPolicy,
    SourceMetric,
    _tracked_source,
    structure_warnings,
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

    def test_transitive_same_name_local_crate_must_use_canonical_manifest(self) -> None:
        expected = Path("/repo/crates/agentsassemble-domain/Cargo.toml").resolve()
        server_id = "path+file:///repo/server#agentsassemble-server@0.1.0"
        fake_id = "path+file:///tmp/fake#agentsassemble-domain@9.9.9"
        packages = {
            server_id: {
                "id": server_id,
                "name": "agentsassemble-server",
                "source": None,
                "manifest_path": "/repo/crates/agentsassemble-server/Cargo.toml",
            },
            fake_id: {
                "id": fake_id,
                "name": "agentsassemble-domain",
                "source": None,
                "manifest_path": "/tmp/fake/Cargo.toml",
            },
        }
        payload = {
            "resolve": {
                "nodes": [
                    {"id": server_id, "dependencies": [fake_id]},
                    {"id": fake_id, "dependencies": []},
                ]
            }
        }
        self.assertIn(
            f"resolved graph reaches agentsassemble-domain from {Path('/tmp/fake/Cargo.toml').resolve()}, expected {expected}",
            resolve_graph_violations(
                payload,
                packages,
                {
                    "agentsassemble-server": Path("/repo/crates/agentsassemble-server/Cargo.toml"),
                    "agentsassemble-domain": expected,
                },
            ),
        )

    def test_parent_cargo_source_override_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository = root / "checkout"
            repository.mkdir()
            cargo = root / ".cargo"
            cargo.mkdir()
            (cargo / "config.toml").write_text(
                '[source.crates-io]\nreplace-with = "mirror"\n', encoding="utf-8"
            )
            found = override_violations(repository, cargo_home=root / "empty-cargo-home")
            self.assertTrue(any("forbidden Cargo source/path overrides" in item for item in found))


class SourceGrowthPolicyTests(unittest.TestCase):
    def test_generated_prefix_does_not_bypass_source_counting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "generatedOversized.rs"
            source.write_text("fn main() {}\n", encoding="utf-8")
            self.assertTrue(_tracked_source(source))

    def test_compressed_one_line_source_hits_logical_line_limit(self) -> None:
        policy = SourceGrowthPolicy(
            warning_line_limit=500,
            strong_warning_line_limit=800,
            default_line_limit=1_000,
            new_file_byte_limit=262_144,
            max_logical_line_bytes=16_384,
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

    def test_line_warnings_do_not_replace_the_absolute_limit(self) -> None:
        policy = SourceGrowthPolicy(
            warning_line_limit=500,
            strong_warning_line_limit=800,
            default_line_limit=1_000,
            new_file_byte_limit=262_144,
            max_logical_line_bytes=16_384,
        )
        metrics = {
            "src/regular.rs": SourceMetric(500, 1_000, 80),
            "src/strong.rs": SourceMetric(800, 1_000, 80),
            "src/cohesive.rs": SourceMetric(1_000, 1_000, 80),
            "src/oversized.rs": SourceMetric(1_001, 1_000, 80),
        }

        warnings = structure_warnings(metrics, policy)
        self.assertTrue(any(item.startswith("warning: src/regular.rs") for item in warnings))
        self.assertTrue(any(item.startswith("strong: src/strong.rs") for item in warnings))
        self.assertTrue(any(item.startswith("strong: src/cohesive.rs") for item in warnings))
        self.assertFalse(any("oversized.rs" in item for item in warnings))
        self.assertIn(
            "src/oversized.rs: 1001 lines exceeds the default limit of 1000",
            violations(metrics, policy),
        )

    def test_executable_lockfile_name_is_still_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "Cargo.lock"
            source.write_text("#!/bin/sh\necho executable\n", encoding="utf-8")
            source.chmod(0o755)
            self.assertTrue(_tracked_source(source))


if __name__ == "__main__":
    unittest.main()
