"""Tests for the repository-owned Cargo artifact cleanup boundary."""

from pathlib import Path
import tempfile
import unittest

from scripts.prune_build_artifacts import allocated_bytes, clean_plan


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
