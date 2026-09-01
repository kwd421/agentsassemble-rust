"""Bound repository-owned Cargo artifacts before a complete verification run."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import stat
import subprocess
import sys


# A packed-debug full workspace verification currently retains about 20.6 GiB.
# Keep that measured warm cache plus a small amount of deterministic build variance.
MAX_ACTIVE_TARGET_BYTES = 24 * 1024**3
ROOT_TARGET = Path("target")
OBSOLETE_DESKTOP_TARGET = Path("desktop/src-tauri/target")


@dataclass(frozen=True)
class CleanTarget:
    path: Path
    observed_bytes: int
    reason: str


def file_allocation_bytes(metadata: os.stat_result) -> int:
    """Use physical blocks where portable, otherwise the file's logical bytes."""
    blocks = getattr(metadata, "st_blocks", None)
    if isinstance(blocks, int):
        return blocks * 512
    return metadata.st_size


def file_identity(metadata: os.stat_result) -> tuple[int, int] | None:
    """Return a stable hard-link identity when the platform provides one."""
    if metadata.st_ino == 0:
        return None
    return (metadata.st_dev, metadata.st_ino)


def allocated_bytes(path: Path) -> int:
    """Return physical bytes without following links inside the Cargo target."""
    total = 0
    seen_inodes: set[tuple[int, int]] = set()
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
                inode = file_identity(metadata)
                if inode is not None:
                    if inode in seen_inodes:
                        continue
                    seen_inodes.add(inode)
                total += file_allocation_bytes(metadata)
    return total


def clean_plan(
    repository_root: Path, maximum_bytes: int, clean_active: bool = False
) -> tuple[CleanTarget, ...]:
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
    if root_target.exists() and (clean_active or root_bytes > maximum_bytes):
        planned.append(
            CleanTarget(
                root_target,
                root_bytes,
                (
                    "explicit active-target maintenance"
                    if clean_active
                    else f"active Cargo target exceeds {maximum_bytes} bytes"
                ),
            )
        )
    return tuple(planned)


def execute_plan(
    repository_root: Path, plan: tuple[CleanTarget, ...], clean: bool
) -> int:
    if not plan:
        print("Cargo artifacts retained for the next build; no maintenance required")
        return 0

    if not clean:
        for target in plan:
            print(
                f"Cargo artifact maintenance required for {target.path}: "
                f"{target.observed_bytes} bytes; {target.reason}",
                file=sys.stderr,
            )
        print(
            "Stop repository Cargo and Tauri work, then run `make artifact-prune`.",
            file=sys.stderr,
        )
        return 1

    for target in plan:
        print(
            f"Cleaning {target.path}: {target.observed_bytes} bytes; {target.reason}"
        )
        subprocess.run(
            ["cargo", "clean", "--target-dir", str(target.path)],
            cwd=repository_root,
            check=True,
        )
    return 0


def main(arguments: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--clean",
        action="store_true",
        help="clean exact project-owned targets during an explicit maintenance window",
    )
    options = parser.parse_args(arguments)
    repository_root = Path(__file__).resolve().parents[1]
    plan = clean_plan(
        repository_root,
        MAX_ACTIVE_TARGET_BYTES,
        clean_active=options.clean,
    )
    return execute_plan(repository_root, plan, options.clean)


if __name__ == "__main__":
    raise SystemExit(main())
