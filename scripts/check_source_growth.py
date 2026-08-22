"""Reject source files that cross the repository's ownership pressure limit."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import stat
import subprocess
import tomllib
from typing import Mapping


POLICY_PATH = Path("docs/architecture/SOURCE_GROWTH_LIMITS.toml")
SOURCE_SUFFIXES = frozenset(
    {
        ".bat", ".cjs", ".cmd", ".css", ".html", ".js", ".json", ".jsx",
        ".mjs", ".mts", ".ps1", ".py", ".rs", ".sh", ".toml", ".ts",
        ".tsx", ".yaml", ".yml",
    }
)
SPECIAL_SOURCE_NAMES = frozenset({"Makefile"})
GENERATED_LOCKFILES = frozenset({"Cargo.lock", "package-lock.json"})


@dataclass(frozen=True)
class FileLimit:
    limit: int
    reason: str


@dataclass(frozen=True)
class SourceGrowthPolicy:
    new_file_line_limit: int
    new_file_byte_limit: int
    max_logical_line_bytes: int
    file_limits: Mapping[str, FileLimit]


@dataclass(frozen=True)
class SourceMetric:
    lines: int
    bytes: int
    max_logical_line_bytes: int


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
    byte_limit = policy.get("new_file_byte_limit")
    logical_line_limit = policy.get("max_logical_line_bytes")
    if isinstance(byte_limit, bool) or not isinstance(byte_limit, int) or byte_limit < 1:
        raise ValueError("new_file_byte_limit must be a positive integer.")
    if (
        isinstance(logical_line_limit, bool)
        or not isinstance(logical_line_limit, int)
        or logical_line_limit < 1
    ):
        raise ValueError("max_logical_line_bytes must be a positive integer.")
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
    return SourceGrowthPolicy(default_limit, byte_limit, logical_line_limit, limits)


def tracked_paths(repository_root: Path) -> tuple[Path, ...]:
    completed = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=repository_root,
        check=True,
        capture_output=True,
    )
    return tuple(
        repository_root / raw.decode("utf-8")
        for raw in completed.stdout.split(b"\0")
        if raw
    )


def collect_source_metrics(repository_root: Path) -> dict[str, SourceMetric]:
    metrics: dict[str, SourceMetric] = {}
    for path in tracked_paths(repository_root):
        if not _tracked_source(path):
            continue
        content = path.read_bytes()
        content.decode("utf-8")
        logical_lines = content.splitlines()
        relative = path.relative_to(repository_root).as_posix()
        metrics[relative] = SourceMetric(
            lines=len(logical_lines),
            bytes=len(content),
            max_logical_line_bytes=max(map(len, logical_lines), default=0),
        )
    return dict(sorted(metrics.items()))


def collect_line_counts(repository_root: Path) -> dict[str, int]:
    return {
        relative: metric.lines
        for relative, metric in collect_source_metrics(repository_root).items()
    }


def violations(metrics: Mapping[str, SourceMetric], policy: SourceGrowthPolicy) -> tuple[str, ...]:
    found: list[str] = []
    for relative, metric in sorted(metrics.items()):
        exception = policy.file_limits.get(relative)
        limit = exception.limit if exception else policy.new_file_line_limit
        if metric.lines > limit:
            ownership = "cohesive-owner exception" if exception else "unowned-file"
            found.append(
                f"{relative}: {metric.lines} lines exceeds its {ownership} limit of {limit}"
            )
        if metric.bytes > policy.new_file_byte_limit:
            found.append(
                f"{relative}: {metric.bytes} bytes exceeds the source byte limit of {policy.new_file_byte_limit}"
            )
        if metric.max_logical_line_bytes > policy.max_logical_line_bytes:
            found.append(
                f"{relative}: {metric.max_logical_line_bytes}-byte logical line exceeds the limit of {policy.max_logical_line_bytes}"
            )
    for relative in sorted(set(policy.file_limits) - set(metrics)):
        found.append(f"{relative}: recorded source file is missing")
    return tuple(found)


def _tracked_source(path: Path) -> bool:
    if not path.is_file() or path.name in GENERATED_LOCKFILES:
        return False
    if path.suffix.lower() in SOURCE_SUFFIXES or path.name in SPECIAL_SOURCE_NAMES:
        return True
    mode = path.stat().st_mode
    if mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
        return True
    with path.open("rb") as source:
        return source.read(2) == b"#!"


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = violations(collect_source_metrics(root), load_policy(root))
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
