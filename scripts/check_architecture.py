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
CRATE_FORBIDDEN = {
    "agentsassemble-domain": DOMAIN_FORBIDDEN,
    "agentsassemble-protocol": frozenset({"axum", "sqlx", "tokio", "tower", "tower-http"}),
    "agentsassemble-persistence": frozenset({"axum", "tower", "tower-http"}),
    "agentsassemble-server": frozenset({"sqlx"}),
}
DESKTOP_MANIFEST = Path("desktop/src-tauri/Cargo.toml")


def metadata(repository_root: Path) -> dict[str, object]:
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=repository_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def desktop_metadata(repository_root: Path) -> dict[str, object]:
    completed = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(DESKTOP_MANIFEST),
        ],
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
        local_path_names = {
            str(dependency.get("name"))
            for dependency in dependencies
            if isinstance(dependency, dict) and dependency.get("path") is not None
        }
        unapproved_paths = sorted(local_path_names - set(OWNED_CRATES))
        found.extend(
            f"{name} imports unapproved local path dependency {item}"
            for item in unapproved_paths
        )
        workspace_dependencies = dependency_names & set(OWNED_CRATES)
        disallowed = sorted(workspace_dependencies - OWNED_CRATES[name])
        found.extend(f"{name} imports disallowed workspace crate {item}" for item in disallowed)
        forbidden = sorted(dependency_names & CRATE_FORBIDDEN[name])
        found.extend(
            f"{name} imports forbidden infrastructure dependency {item}"
            for item in forbidden
        )
    return tuple(found)


def desktop_violations(payload: dict[str, object]) -> tuple[str, ...]:
    packages = payload.get("packages")
    if not isinstance(packages, list) or len(packages) != 1:
        return ("desktop metadata must contain exactly one package",)
    package = packages[0]
    if not isinstance(package, dict) or package.get("name") != "agentsassemble-desktop":
        return ("desktop package must be agentsassemble-desktop",)
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        return ("desktop metadata did not contain dependencies",)
    names = {
        str(dependency.get("name"))
        for dependency in dependencies
        if isinstance(dependency, dict)
    }
    allowed_owned = {"agentsassemble-domain"}
    local_path_names = {
        str(dependency.get("name"))
        for dependency in dependencies
        if isinstance(dependency, dict) and dependency.get("path") is not None
    }
    found = [
        f"desktop imports disallowed workspace crate {name}"
        for name in sorted((names & set(OWNED_CRATES)) - allowed_owned)
    ]
    found.extend(
        f"desktop imports forbidden server infrastructure dependency {name}"
        for name in sorted(names & {"axum", "sqlx", "tower", "tower-http"})
    )
    found.extend(
        f"desktop imports unapproved local path dependency {name}"
        for name in sorted(local_path_names - allowed_owned)
    )
    return tuple(found)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = architecture_violations(metadata(root)) + desktop_violations(desktop_metadata(root))
    if not found:
        return 0
    print("Architecture violations:")
    for violation in found:
        print(f"- {violation}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
