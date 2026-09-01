"""Bound repository-owned Cargo artifacts before a complete verification run."""

from __future__ import annotations

from dataclasses import dataclass
import os
from pathlib import Path
import stat
import subprocess


MAX_ACTIVE_TARGET_BYTES = 20 * 1024**3
ROOT_TARGET = Path("target")
OBSOLETE_DESKTOP_TARGET = Path("desktop/src-tauri/target")


@dataclass(frozen=True)
class CleanTarget:
    path: Path
    observed_bytes: int
    reason: str


def allocated_bytes(path: Path) -> int:
    """Return physical bytes without following links inside the Cargo target."""
    total = 0
    for directory, child_directories, files in os.walk(path, followlinks=False):
        current = Path(directory)
        child_directories[:] = [
            name for name in child_directories if not (current / name).is_symlink()
        ]
        for name in files:
            try:
                metadata = os.lstat(current / name)
            except FileNotFoundError:
                continue
            if stat.S_ISREG(metadata.st_mode):
                total += metadata.st_blocks * 512
    return total


def clean_plan(repository_root: Path, maximum_bytes: int) -> tuple[CleanTarget, ...]:
    if maximum_bytes < 0:
        raise ValueError("maximum_bytes must not be negative")

    root_target = repository_root / ROOT_TARGET
    obsolete_target = repository_root / OBSOLETE_DESKTOP_TARGET
    for path in (root_target, obsolete_target):
        if path.is_symlink():
            raise RuntimeError(f"refusing symlinked Cargo target: {path}")
        if path.exists() and not path.is_dir():
            raise RuntimeError(f"Cargo target is not a directory: {path}")

    planned: list[CleanTarget] = []
    if obsolete_target.exists():
        planned.append(
            CleanTarget(
                obsolete_target,
                allocated_bytes(obsolete_target),
                "desktop now shares the repository Cargo target",
            )
        )

    root_bytes = allocated_bytes(root_target) if root_target.exists() else 0
    if root_bytes > maximum_bytes:
        planned.append(
            CleanTarget(
                root_target,
                root_bytes,
                f"active Cargo target exceeds {maximum_bytes} allocated bytes",
            )
        )
    return tuple(planned)


def main() -> int:
    repository_root = Path(__file__).resolve().parents[1]
    plan = clean_plan(repository_root, MAX_ACTIVE_TARGET_BYTES)
    if not plan:
        current = allocated_bytes(repository_root / ROOT_TARGET)
        print(f"Cargo artifacts retained for the next build: {current} allocated bytes")
        return 0

    for target in plan:
        print(
            f"Cleaning {target.path}: {target.observed_bytes} allocated bytes; "
            f"{target.reason}"
        )
        subprocess.run(
            ["cargo", "clean", "--target-dir", str(target.path)],
            cwd=repository_root,
            check=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
