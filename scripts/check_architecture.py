"""Enforce the Rust workspace's product dependency direction."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess


OWNED_CRATES = {
    "agentsassemble-domain": frozenset(),
    "agentsassemble-persistence": frozenset({"agentsassemble-domain"}),
    "agentsassemble-protocol": frozenset({"agentsassemble-domain"}),
    "agentsassemble-server": frozenset(
        {
            "agentsassemble-domain",
            "agentsassemble-persistence",
            "agentsassemble-protocol",
        }
    ),
}
DOMAIN_FORBIDDEN = frozenset(
    {"axum", "reqwest", "sqlx", "tokio", "tokio-util", "tower", "tower-http"}
)


def metadata(repository_root: Path) -> dict[str, object]:
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=repository_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def architecture_violations(payload: dict[str, object]) -> tuple[str, ...]:
    packages = payload.get("packages")
    if not isinstance(packages, list):
        return ("cargo metadata did not contain packages",)
    names = {
        str(package.get("name"))
        for package in packages
        if isinstance(package, dict)
    }
    found: list[str] = []
    unexpected = sorted(names - set(OWNED_CRATES))
    missing = sorted(set(OWNED_CRATES) - names)
    found.extend(f"unowned workspace crate: {name}" for name in unexpected)
    found.extend(f"owned workspace crate is missing: {name}" for name in missing)

    for package in packages:
        if not isinstance(package, dict):
            continue
        name = str(package.get("name"))
        if name not in OWNED_CRATES:
            continue
        dependencies = package.get("dependencies")
        if not isinstance(dependencies, list):
            continue
        dependency_names = {
            str(dependency.get("name"))
            for dependency in dependencies
            if isinstance(dependency, dict)
        }
        workspace_dependencies = dependency_names & set(OWNED_CRATES)
        disallowed = sorted(workspace_dependencies - OWNED_CRATES[name])
        found.extend(f"{name} imports disallowed workspace crate {item}" for item in disallowed)
        if name == "agentsassemble-domain":
            forbidden = sorted(dependency_names & DOMAIN_FORBIDDEN)
            found.extend(f"domain imports infrastructure dependency {item}" for item in forbidden)
    return tuple(found)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = architecture_violations(metadata(root))
    if not found:
        return 0
    print("Architecture violations:")
    for violation in found:
        print(f"- {violation}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())

