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
class SourceGrowthPolicy:
    warning_line_limit: int
    strong_warning_line_limit: int
    default_line_limit: int
    new_file_byte_limit: int
    max_logical_line_bytes: int


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
    if not isinstance(policy, dict):
        raise ValueError("Source growth policy needs [policy].")
    warning_limit = policy.get("warning_line_limit")
    strong_warning_limit = policy.get("strong_warning_line_limit")
    default_limit = policy.get("default_line_limit")
    if any(
        isinstance(limit, bool) or not isinstance(limit, int) or limit < 1
        for limit in (warning_limit, strong_warning_limit, default_limit)
    ):
        raise ValueError("Source line thresholds must be positive integers.")
    if not warning_limit < strong_warning_limit < default_limit:
        raise ValueError("Source line thresholds must increase from warning to default limit.")
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
    return SourceGrowthPolicy(
        warning_limit,
        strong_warning_limit,
        default_limit,
        byte_limit,
        logical_line_limit,
    )


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
        if metric.lines > policy.default_line_limit:
            found.append(
                f"{relative}: {metric.lines} lines exceeds the default limit of "
                f"{policy.default_line_limit}"
            )
        if metric.bytes > policy.new_file_byte_limit:
            found.append(
                f"{relative}: {metric.bytes} bytes exceeds the source byte limit of {policy.new_file_byte_limit}"
            )
        if metric.max_logical_line_bytes > policy.max_logical_line_bytes:
            found.append(
                f"{relative}: {metric.max_logical_line_bytes}-byte logical line exceeds the limit of {policy.max_logical_line_bytes}"
            )
    return tuple(found)


def structure_warnings(
    metrics: Mapping[str, SourceMetric], policy: SourceGrowthPolicy
) -> tuple[str, ...]:
    found: list[str] = []
    for relative, metric in sorted(metrics.items()):
        if metric.lines > policy.default_line_limit:
            continue
        if metric.lines >= policy.strong_warning_line_limit:
            found.append(
                f"strong: {relative}: {metric.lines} lines is a strong split candidate; "
                "keep it only when one state and invariant owner remains cohesive"
            )
        elif metric.lines >= policy.warning_line_limit:
            found.append(
                f"warning: {relative}: {metric.lines} lines requires a responsibility review"
            )
    return tuple(found)


def _tracked_source(path: Path) -> bool:
    if not path.is_file():
        return False
    mode = path.stat().st_mode
    with path.open("rb") as source:
        has_shebang = source.read(2) == b"#!"
    if mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH) or has_shebang:
        return True
    if path.name in GENERATED_LOCKFILES:
        return False
    if path.suffix.lower() in SOURCE_SUFFIXES or path.name in SPECIAL_SOURCE_NAMES:
        return True
    return False


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    policy = load_policy(root)
    metrics = collect_source_metrics(root)
    found = violations(metrics, policy)
    warnings = structure_warnings(metrics, policy)
    if warnings:
        print(
            f"Source structure warnings: {len(warnings)} file(s) are at least "
            f"{policy.warning_line_limit} lines; responsibility boundaries, not line count, "
            "decide whether to split."
        )
        for item in warnings:
            print(f"- {item}")
    if not found:
        return 0
    print("Source growth violations:")
    for violation in found:
        print(f"- {violation}")
    print(
        "Split at state and invariant ownership boundaries before the default limit. A small "
        "file that mixes authorities still requires separation."
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
