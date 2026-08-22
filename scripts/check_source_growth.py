"""Reject source files that cross the repository's ownership pressure limit."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import tomllib
from typing import Mapping


POLICY_PATH = Path("docs/architecture/SOURCE_GROWTH_LIMITS.toml")
SOURCE_ROOTS = (
    Path("crates"),
    Path("frontend/src"),
    Path("desktop/src"),
    Path("desktop/scripts"),
    Path("desktop/src-tauri/src"),
    Path("scripts"),
    Path("tests"),
)
SOURCE_SUFFIXES = frozenset(
    {".css", ".js", ".jsx", ".py", ".rs", ".ts", ".tsx"}
)


@dataclass(frozen=True)
class FileLimit:
    limit: int
    reason: str


@dataclass(frozen=True)
class SourceGrowthPolicy:
    new_file_line_limit: int
    file_limits: Mapping[str, FileLimit]


def load_policy(repository_root: Path) -> SourceGrowthPolicy:
    payload = tomllib.loads(
        (repository_root / POLICY_PATH).read_text(encoding="utf-8")
    )
    policy = payload.get("policy")
    raw_limits = payload.get("file_limits")
    if not isinstance(policy, dict) or not isinstance(raw_limits, dict):
        raise ValueError("Source growth policy needs [policy] and [file_limits].")
    default_limit = policy.get("new_file_line_limit")
    if isinstance(default_limit, bool) or not isinstance(default_limit, int) or default_limit < 1:
        raise ValueError("new_file_line_limit must be a positive integer.")
    if list(raw_limits) != sorted(raw_limits):
        raise ValueError("Source growth exceptions must be sorted by path.")

    limits: dict[str, FileLimit] = {}
    for relative, raw in raw_limits.items():
        path = Path(relative)
        if path.is_absolute() or ".." in path.parts or path.as_posix() != relative:
            raise ValueError(f"Invalid source growth path: {relative!r}")
        if not isinstance(raw, dict):
            raise ValueError(f"Exception {relative!r} needs limit and reason.")
        limit = raw.get("limit")
        reason = raw.get("reason")
        if isinstance(limit, bool) or not isinstance(limit, int) or limit <= default_limit:
            raise ValueError(
                f"Exception {relative!r} must have a limit above {default_limit}."
            )
        if not isinstance(reason, str) or len(reason.strip()) < 40:
            raise ValueError(
                f"Exception {relative!r} needs a specific cohesive-owner reason."
            )
        limits[relative] = FileLimit(limit=limit, reason=reason.strip())
    return SourceGrowthPolicy(default_limit, limits)


def collect_line_counts(repository_root: Path) -> dict[str, int]:
    counts: dict[str, int] = {}
    for source_root in SOURCE_ROOTS:
        absolute_root = repository_root / source_root
        if not absolute_root.exists():
            continue
        for path in absolute_root.rglob("*"):
            if not _tracked_source(path):
                continue
            relative = path.relative_to(repository_root).as_posix()
            counts[relative] = len(path.read_text(encoding="utf-8").splitlines())
    return dict(sorted(counts.items()))


def violations(
    line_counts: Mapping[str, int], policy: SourceGrowthPolicy
) -> tuple[str, ...]:
    found: list[str] = []
    for relative, count in sorted(line_counts.items()):
        exception = policy.file_limits.get(relative)
        limit = exception.limit if exception else policy.new_file_line_limit
        if count > limit:
            ownership = "cohesive-owner exception" if exception else "unowned-file"
            found.append(
                f"{relative}: {count} lines exceeds its {ownership} limit of {limit}"
            )
    for relative in sorted(set(policy.file_limits) - set(line_counts)):
        found.append(f"{relative}: recorded source file is missing")
    return tuple(found)


def _tracked_source(path: Path) -> bool:
    return (
        path.is_file()
        and path.suffix.lower() in SOURCE_SUFFIXES
        and not path.name.lower().startswith("generated")
        and "node_modules" not in path.parts
        and "target" not in path.parts
        and "__pycache__" not in path.parts
    )


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = violations(collect_line_counts(root), load_policy(root))
    if not found:
        return 0
    print("Source growth violations:")
    for violation in found:
        print(f"- {violation}")
    print(
        "Split at an owning boundary. Do not add or raise an exception without "
        "an explicit decision that the responsibility must remain cohesive."
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
