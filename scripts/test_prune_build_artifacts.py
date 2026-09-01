"""Tests for the repository-owned Cargo artifact cleanup boundary."""

from contextlib import redirect_stderr
import io
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from scripts.prune_build_artifacts import (
    CleanTarget,
    allocated_bytes,
    clean_plan,
    execute_plan,
    file_allocation_bytes,
    file_identity,
)


class BuildArtifactCleanupTests(unittest.TestCase):
    def test_active_target_below_limit_is_retained(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()
            (target / "current").write_bytes(b"current build")

            self.assertEqual(clean_plan(root, 1024**2), ())

    def test_active_target_above_limit_is_cleaned_as_one_cargo_owner(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()
            (target / "stale").write_bytes(b"stale build")

            plan = clean_plan(root, 0)

            self.assertEqual([item.path for item in plan], [target])
            self.assertGreater(plan[0].observed_bytes, 0)

    def test_explicit_maintenance_includes_active_target_below_limit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()

            plan = clean_plan(root, 1024**2, clean_active=True)

            self.assertEqual([item.path for item in plan], [target])

    def test_obsolete_desktop_target_is_always_cleaned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            obsolete = root / "desktop/src-tauri/target"
            obsolete.mkdir(parents=True)

            plan = clean_plan(root, 1024**2)

            self.assertEqual([item.path for item in plan], [obsolete])

    def test_hard_links_count_one_physical_inode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.mkdir()
            original = target / "original"
            original.write_bytes(b"one physical allocation")
            (target / "hard-link").hardlink_to(original)

            self.assertEqual(
                allocated_bytes(target),
                original.stat().st_blocks * 512,
            )

    def test_platform_without_block_counts_uses_logical_bytes(self) -> None:
        metadata = SimpleNamespace(st_size=123)

        self.assertEqual(file_allocation_bytes(metadata), 123)

    def test_unavailable_file_index_never_collapses_distinct_files(self) -> None:
        metadata = SimpleNamespace(st_dev=0, st_ino=0)

        self.assertIsNone(file_identity(metadata))

    def test_oversized_check_never_invokes_cargo_clean(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            plan = (CleanTarget(root / "target", 21, "over limit"),)

            with (
                patch("scripts.prune_build_artifacts.subprocess.run") as run,
                redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(execute_plan(root, plan, clean=False), 1)

            run.assert_not_called()

    def test_symlinked_target_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside"
            outside.mkdir()
            (root / "target").symlink_to(outside, target_is_directory=True)

            with self.assertRaisesRegex(RuntimeError, "refusing symlinked"):
                clean_plan(root, 1024**2)


if __name__ == "__main__":
    unittest.main()
