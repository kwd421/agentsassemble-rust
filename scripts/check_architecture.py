"""Enforce the Rust workspace's product dependency direction."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tomllib


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
OWNED_MANIFESTS = {
    "agentsassemble-domain": Path("crates/agentsassemble-domain/Cargo.toml"),
    "agentsassemble-persistence": Path("crates/agentsassemble-persistence/Cargo.toml"),
    "agentsassemble-protocol": Path("crates/agentsassemble-protocol/Cargo.toml"),
    "agentsassemble-server": Path("crates/agentsassemble-server/Cargo.toml"),
}


def metadata(repository_root: Path) -> dict[str, object]:
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
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
            "--locked",
            "--manifest-path",
            str(DESKTOP_MANIFEST),
        ],
        cwd=repository_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def workspace_packages(payload: dict[str, object]) -> list[dict[str, object]]:
    packages = payload.get("packages")
    members = payload.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(members, list):
        return []
    member_ids = {str(member) for member in members}
    return [
        package
        for package in packages
        if isinstance(package, dict) and str(package.get("id")) in member_ids
    ]


def architecture_violations(
    payload: dict[str, object], repository_root: Path | None = None
) -> tuple[str, ...]:
    packages = workspace_packages(payload)
    if not packages:
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

    package_by_id = {
        str(package.get("id")): package
        for package in payload.get("packages", [])
        if isinstance(package, dict)
    }
    if repository_root is not None:
        for package in packages:
            name = str(package.get("name"))
            if name not in OWNED_MANIFESTS:
                continue
            actual = Path(str(package.get("manifest_path"))).resolve()
            expected = (repository_root / OWNED_MANIFESTS[name]).resolve()
            if actual != expected:
                found.append(f"{name} manifest is {actual}, expected {expected}")

    for package in packages:
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
        if repository_root is not None:
            for dependency in dependencies:
                if not isinstance(dependency, dict):
                    continue
                dependency_name = str(dependency.get("name"))
                expected_relative = OWNED_MANIFESTS.get(dependency_name)
                if expected_relative is None:
                    continue
                if dependency.get("path") is None:
                    found.append(f"{name} resolves owned crate {dependency_name} from a non-path source")
                    continue
                actual_manifest = (Path(str(dependency["path"])) / "Cargo.toml").resolve()
                expected_manifest = (repository_root / expected_relative).resolve()
                if actual_manifest != expected_manifest:
                    found.append(
                        f"{name} resolves {dependency_name} from {actual_manifest}, expected {expected_manifest}"
                    )
        workspace_dependencies = dependency_names & set(OWNED_CRATES)
        disallowed = sorted(workspace_dependencies - OWNED_CRATES[name])
        found.extend(f"{name} imports disallowed workspace crate {item}" for item in disallowed)
        forbidden = sorted(dependency_names & CRATE_FORBIDDEN[name])
        found.extend(
            f"{name} imports forbidden infrastructure dependency {item}"
            for item in forbidden
        )
    found.extend(resolve_graph_violations(payload, package_by_id, set(OWNED_CRATES)))
    return tuple(found)


def desktop_violations(
    payload: dict[str, object], repository_root: Path | None = None
) -> tuple[str, ...]:
    packages = workspace_packages(payload)
    if len(packages) != 1:
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
    allowed_owned = {"agentsassemble-domain", "agentsassemble-protocol"}
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
    if repository_root is not None:
        expected_desktop = (repository_root / DESKTOP_MANIFEST).resolve()
        actual_desktop = Path(str(package.get("manifest_path"))).resolve()
        if actual_desktop != expected_desktop:
            found.append(
                f"desktop manifest is {actual_desktop}, expected {expected_desktop}"
            )
        for dependency in dependencies:
            if not isinstance(dependency, dict):
                continue
            name = str(dependency.get("name"))
            expected_relative = OWNED_MANIFESTS.get(name)
            if expected_relative is None:
                continue
            if dependency.get("path") is None:
                found.append(f"desktop resolves owned crate {name} from a non-path source")
                continue
            actual = (Path(str(dependency["path"])) / "Cargo.toml").resolve()
            expected = (repository_root / expected_relative).resolve()
            if actual != expected:
                found.append(f"desktop resolves {name} from {actual}, expected {expected}")
    package_by_id = {
        str(item.get("id")): item
        for item in payload.get("packages", [])
        if isinstance(item, dict)
    }
    found.extend(resolve_graph_violations(payload, package_by_id, {"agentsassemble-desktop"}))
    return tuple(found)


def resolve_graph_violations(
    payload: dict[str, object],
    package_by_id: dict[str, dict[str, object]],
    roots: set[str],
) -> list[str]:
    resolve = payload.get("resolve")
    if not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
        return ["cargo metadata did not contain a resolved dependency graph"]
    nodes = {
        str(node.get("id")): node
        for node in resolve["nodes"]
        if isinstance(node, dict)
    }
    root_ids = {
        package_id
        for package_id, package in package_by_id.items()
        if str(package.get("name")) in roots
    }
    reachable = set(root_ids)
    pending = list(root_ids)
    while pending:
        package_id = pending.pop()
        node = nodes.get(package_id, {})
        for dependency_id in node.get("dependencies", []):
            dependency_id = str(dependency_id)
            if dependency_id not in reachable:
                reachable.add(dependency_id)
                pending.append(dependency_id)
    found: list[str] = []
    for package_id in sorted(reachable):
        package = package_by_id.get(package_id)
        if not package or package.get("source") is not None:
            continue
        name = str(package.get("name"))
        if name not in OWNED_MANIFESTS and name != "agentsassemble-desktop":
            found.append(f"resolved graph reaches unapproved local package {name}")
    return found


def override_violations(repository_root: Path) -> tuple[str, ...]:
    found: list[str] = []
    ignored = {"target", "node_modules", ".git"}
    for manifest in repository_root.rglob("Cargo.toml"):
        if ignored.intersection(manifest.parts):
            continue
        payload = tomllib.loads(manifest.read_text(encoding="utf-8"))
        relative = manifest.relative_to(repository_root).as_posix()
        if "patch" in payload:
            found.append(f"{relative} contains forbidden [patch] source overrides")
        if "replace" in payload:
            found.append(f"{relative} contains forbidden [replace] source overrides")
    for config_name in (".cargo/config", ".cargo/config.toml"):
        for config in repository_root.rglob(config_name):
            if ignored.intersection(config.parts):
                continue
            payload = tomllib.loads(config.read_text(encoding="utf-8"))
            relative = config.relative_to(repository_root).as_posix()
            if "source" in payload or "paths" in payload:
                found.append(f"{relative} contains forbidden Cargo source/path overrides")
    return tuple(found)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = (
        architecture_violations(metadata(root), root)
        + desktop_violations(desktop_metadata(root), root)
        + override_violations(root)
    )
    if not found:
        return 0
    print("Architecture violations:")
    for violation in found:
        print(f"- {violation}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
