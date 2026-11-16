#!/usr/bin/env python3
"""Materialise standalone crates.io-ready publish trees for a workspace release.

Why this exists
---------------
`cargo publish -p <member>` resolves the *whole* workspace before packaging, so a
path dependency that has been rewritten into a registry requirement inside the
workspace deadlocks the release: publishing `pd-edge-abi` fails because
`crates.io` does not have `pd-edge-abi 0.2.0` yet, which is exactly what the
member `pd-edge` (already rewritten to `pd-edge-abi = "0.2.0"`) demands.

So instead of mutating the checkout in place, every released package is copied
into its own standalone tree whose manifest only contains dependencies that
crates.io can resolve:

* `path` / `git` / `rev` / `branch` / `tag` selectors are removed and replaced
  by the version of the crate being released (or an explicit registry version
  for external crates such as `pd-vm` / `pd-host-function`);
* `dev-dependencies` are dropped (a consumer never builds them, and they must
  not block a release);
* `[workspace]` / `[workspace.package]` inheritance is materialised so the tree
  is its own workspace root;
* the publish order is derived from the actual normal + build dependency graph,
  so dependencies are always published first.

Usage
-----
    python3 .github/scripts/prepare_publish.py \
        --workspace-root . \
        --out "$RUNNER_TEMP/publish-crates" \
        --version 0.2.0 \
        --package-order pd-edge-host-function pd-edge-abi pd-edge \
        --registry-dep pd-vm=0.22.2 \
        --registry-dep pd-host-function=0.22.2
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

SOURCE_KEYS = ("path", "git", "rev", "branch", "tag")
DEPENDENCY_SUFFIXES = ("dependencies", "build-dependencies")
DEV_SUFFIXES = ("dev-dependencies",)
IGNORED_DIR_NAMES = {
    ".git",
    ".github",
    ".ci-tmp",
    "target",
    "dist",
    "node_modules",
}

_HEADER_RE = re.compile(r"^\[(?P<name>\[[^\[\]]+\]|[^\[\]]+)\]$")


class PublishError(RuntimeError):
    """Raised when a manifest cannot be made crates.io-resolvable."""


@dataclass
class StagedPackage:
    name: str
    version: str
    manifest: Path
    dependencies: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# tiny TOML value helpers (only the shapes used by inline dependency tables)


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def emit_value(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return _toml_string(value)
    if isinstance(value, list):
        return "[" + ", ".join(emit_value(item) for item in value) + "]"
    if isinstance(value, dict):
        return emit_inline_table(value)
    raise PublishError(f"unsupported TOML value in a dependency table: {value!r}")


def emit_inline_table(table: dict[str, Any]) -> str:
    if not table:
        return "{}"
    inner = ", ".join(f"{key} = {emit_value(value)}" for key, value in table.items())
    return "{ " + inner + " }"


# ---------------------------------------------------------------------------
# manifest text surgery


def section_kind(header_name: str) -> str:
    name = header_name.strip()
    if name == "workspace" or name.startswith("workspace."):
        return "workspace"
    if name == "package":
        return "package"
    for suffix in DEV_SUFFIXES:
        if name == suffix or name.endswith("." + suffix):
            return "dev-dependencies"
    for suffix in DEPENDENCY_SUFFIXES:
        if name == suffix or name.endswith("." + suffix):
            return "dependencies"
    return "other"


def split_sections(lines: list[str]) -> list[tuple[str, str | None, list[str]]]:
    """Split a manifest into (kind, header-line, body-lines) triples."""
    sections: list[tuple[str, str | None, list[str]]] = [("other", None, [])]
    for line in lines:
        match = _HEADER_RE.match(line.strip())
        if match:
            sections.append((section_kind(match.group("name")), line, []))
        else:
            sections[-1][2].append(line)
    return sections


def _balanced(text: str) -> bool:
    depth = 0
    in_string = False
    escaped = False
    for char in text:
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
    return depth <= 0


def rewrite_dependency(alias: str, spec: Any, versions: dict[str, str]) -> Any:
    """Return a crates.io-resolvable dependency spec (or a plain version string)."""
    if not isinstance(spec, dict):
        name = alias
        override = versions.get(name)
        if override is None:
            return spec
        return override

    if spec.get("workspace") is True:
        raise PublishError(
            f"dependency {alias!r} uses `workspace = true`; declare an explicit "
            "version/path so the publish rewrite can resolve it"
        )

    name = spec.get("package") or alias
    has_source = any(key in spec for key in SOURCE_KEYS)
    target = versions.get(name)

    if has_source and target is None:
        selectors = ", ".join(f"{key} = {spec[key]!r}" for key in SOURCE_KEYS if key in spec)
        raise PublishError(
            f"dependency {alias!r} ({name}) is not on crates.io and has no known "
            f"registry version ({selectors}); pass --registry-dep {name}=<version>"
        )

    if not has_source and target is None:
        return spec

    rewritten: dict[str, Any] = {}
    if "package" in spec:
        rewritten["package"] = spec["package"]
    rewritten["version"] = target
    for key, value in spec.items():
        if key in ("package", "version") or key in SOURCE_KEYS:
            continue
        rewritten[key] = value
    return rewritten


def rewrite_dependency_section(
    lines: list[str], versions: dict[str, str]
) -> tuple[list[str], list[str]]:
    """Rewrite every dependency entry in a section; return (lines, dep names)."""
    out: list[str] = []
    names: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in stripped:
            out.append(line)
            index += 1
            continue

        chunk = [line]
        raw = line.split("=", 1)[1]
        while not _balanced(raw) and index + len(chunk) < len(lines):
            chunk.append(lines[index + len(chunk)])
            raw += "\n" + chunk[-1]
        index += len(chunk)

        alias = line.split("=", 1)[0].strip()
        try:
            parsed = tomllib.loads(f"dep = {raw.strip()}")
        except tomllib.TOMLDecodeError:
            # Not an inline table we can reason about; keep verbatim.
            out.extend(chunk)
            continue

        spec = parsed["dep"]
        name = spec.get("package") or alias if isinstance(spec, dict) else alias
        names.append(name)
        rewritten = rewrite_dependency(alias, spec, versions)
        if isinstance(rewritten, str):
            if rewritten == spec:
                out.extend(chunk)
            else:
                out.append(f'{alias} = "{rewritten}"')
        else:
            out.append(f"{alias} = {emit_inline_table(rewritten)}")
    return out, names


def materialise_package_section(
    lines: list[str], inherited: dict[str, Any], name: str, version: str
) -> list[str]:
    out: list[str] = []
    for line in lines:
        stripped = line.strip()
        match = re.match(r"^([A-Za-z0-9_-]+)\.workspace\s*=\s*true$", stripped)
        if match:
            key = match.group(1)
            if key == "version":
                # The release version always wins over the inherited one.
                out.append(f'version = "{version}"')
                continue
            if key not in inherited:
                raise PublishError(
                    f"[package] {key}.workspace = true has no [workspace.package].{key}"
                )
            out.append(f"{key} = {emit_value(inherited[key])}")
            continue
        if re.match(r"^version\s*=", stripped):
            out.append(f'version = "{version}"')
            continue
        out.append(line)
    if not any(re.match(r"^version\s*=", line.strip()) for line in out):
        raise PublishError(f"[package] {name}: no version found after rewrite")
    return out


def prepare_manifest(
    text: str, name: str, version: str, versions: dict[str, str], inherited: dict[str, Any]
) -> tuple[str, list[str]]:
    lines = text.splitlines()
    dependencies: list[str] = []
    out: list[str] = []
    has_workspace_table = False
    for kind, header, body in split_sections(lines):
        if kind == "workspace":
            continue
        if kind == "dev-dependencies":
            continue
        if kind == "package":
            out.append(header or "")
            out.extend(materialise_package_section(body, inherited, name, version))
            continue
        if kind == "dependencies":
            rewritten, names = rewrite_dependency_section(body, versions)
            out.append(header or "")
            out.extend(rewritten)
            dependencies.extend(names)
            continue
        if header is not None:
            out.append(header)
        out.extend(body)

    # Make the staged tree its own workspace root so Cargo never tries to join
    # the parent workspace (the tree lives inside the checkout).
    if not has_workspace_table:
        while out and not out[-1].strip():
            out.pop()
        out.extend(["", "[workspace]"])

    return "\n".join(out).rstrip("\n") + "\n", dependencies


# ---------------------------------------------------------------------------
# workspace discovery / staging


def _read_manifest(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def workspace_packages(root: Path) -> tuple[dict[str, Path], dict[str, Any]]:
    manifest = _read_manifest(root / "Cargo.toml")
    workspace = manifest.get("workspace")
    if workspace is None:
        raise PublishError(f"{root / 'Cargo.toml'} has no [workspace] table")
    inherited = workspace.get("package", {}) or {}
    packages: dict[str, Path] = {}
    for member in workspace.get("members", []):
        if any(char in member for char in "*?["):
            for candidate in sorted(root.glob(member)):
                if (candidate / "Cargo.toml").is_file():
                    data = _read_manifest(candidate / "Cargo.toml")
                    packages[data["package"]["name"]] = candidate / "Cargo.toml"
            continue
        candidate = (root / member).resolve()
        data = _read_manifest(candidate / "Cargo.toml")
        packages[data["package"]["name"]] = candidate / "Cargo.toml"
    return packages, inherited


def _copy_ignore(nested: set[Path]):
    def ignore(directory: str, names: list[str]) -> set[str]:
        skipped: set[str] = set()
        current = Path(directory)
        for name in names:
            if name in IGNORED_DIR_NAMES:
                skipped.add(name)
                continue
            if name == "Cargo.lock":
                skipped.add(name)
                continue
            candidate = (current / name).resolve()
            if candidate in nested:
                skipped.add(name)
        return skipped

    return ignore


def stage_package(
    name: str,
    manifest_path: Path,
    version: str,
    versions: dict[str, str],
    inherited: dict[str, Any],
    nested: set[Path],
    out_dir: Path,
) -> StagedPackage:
    package_dir = manifest_path.parent
    destination = out_dir / name
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(package_dir, destination, ignore=_copy_ignore(nested))

    text = manifest_path.read_text()
    rewritten, dependencies = prepare_manifest(text, name, version, versions, inherited)
    manifest = destination / "Cargo.toml"
    manifest.write_text(rewritten)

    parsed = _read_manifest(manifest)
    package = parsed.get("package", {})
    if package.get("name") != name:
        raise PublishError(f"staged {manifest} is not package {name}")
    if package.get("version") != version:
        raise PublishError(
            f"staged {manifest} version is {package.get('version')!r}, expected {version!r}"
        )
    for section in ("dev-dependencies",):
        if section in parsed:
            raise PublishError(f"staged {manifest} still contains [{section}]")
    for target in (parsed.get("target") or {}).values():
        if isinstance(target, dict) and "dev-dependencies" in target:
            raise PublishError(f"staged {manifest} still contains target dev-dependencies")
    if "workspace" not in parsed:
        raise PublishError(f"staged {manifest} is not its own workspace root")
    for dependency_name, spec in _iter_dependency_specs(parsed):
        if isinstance(spec, dict) and any(key in spec for key in SOURCE_KEYS):
            raise PublishError(
                f"staged {manifest}: dependency {dependency_name!r} still has a "
                "path/git selector"
            )
    return StagedPackage(name, version, manifest, dependencies)


def _iter_dependency_specs(manifest: dict[str, Any]) -> Iterable[tuple[str, Any]]:
    tables: list[dict[str, Any]] = []
    for key in ("dependencies", "build-dependencies"):
        value = manifest.get(key)
        if isinstance(value, dict):
            tables.append(value)
    for target in (manifest.get("target") or {}).values():
        if not isinstance(target, dict):
            continue
        for key in ("dependencies", "build-dependencies"):
            value = target.get(key)
            if isinstance(value, dict):
                tables.append(value)
    for table in tables:
        for dependency_name, spec in table.items():
            yield dependency_name, spec


def publish_order(packages: dict[str, StagedPackage]) -> list[StagedPackage]:
    """Topologically order packages so dependencies are published first."""
    order: list[StagedPackage] = []
    visiting: set[str] = set()
    done: set[str] = set()

    def visit(name: str) -> None:
        if name in done:
            return
        if name in visiting:
            raise PublishError(f"dependency cycle among crates.io packages at {name}")
        visiting.add(name)
        for dependency in packages[name].dependencies:
            if dependency in packages:
                visit(dependency)
        visiting.discard(name)
        done.add(name)
        order.append(packages[name])

    for name in packages:
        visit(name)
    return order


def prepare_publish(
    workspace_root: Path,
    out_dir: Path,
    version: str,
    package_order: list[str] | None = None,
    registry_deps: dict[str, str] | None = None,
) -> list[StagedPackage]:
    workspace_root = workspace_root.resolve()
    out_dir = out_dir.resolve()
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?", version or ""):
        raise PublishError(f"invalid publish version {version!r}")
    manifests, inherited = workspace_packages(workspace_root)
    if not manifests:
        raise PublishError(f"no workspace packages found in {workspace_root}")

    if package_order:
        expected = set(package_order)
        actual = set(manifests)
        if expected != actual:
            raise PublishError(
                "PACKAGE_ORDER does not match the workspace packages: "
                f"missing={sorted(actual - expected)} unknown={sorted(expected - actual)}"
            )

    versions = dict(registry_deps or {})
    for name in manifests:
        versions[name] = version

    nested = {path.parent.resolve() for path in manifests.values()}
    out_dir.mkdir(parents=True, exist_ok=True)
    staged: dict[str, StagedPackage] = {}
    for name, manifest_path in manifests.items():
        staged[name] = stage_package(
            name, manifest_path, version, versions, inherited, nested, out_dir
        )

    ordered = publish_order(staged)
    if package_order:
        allowed = {name: index for index, name in enumerate(package_order)}
        declared = [package.name for package in ordered]
        if declared != sorted(declared, key=allowed.__getitem__):
            print(
                "prepare_publish: PACKAGE_ORDER is not dependency safe; using "
                f"computed order {declared}",
                file=sys.stderr,
            )
    return ordered


def write_plan(ordered: list[StagedPackage], version: str, path: Path) -> None:
    plan = {
        "version": version,
        "packages": [
            {
                "name": package.name,
                "version": package.version,
                "manifest": str(package.manifest),
                "dependencies": package.dependencies,
            }
            for package in ordered
        ],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(plan, indent=2) + "\n")


def parse_named_versions(values: list[str] | None, flag: str) -> dict[str, str]:
    parsed: dict[str, str] = {}
    for value in values or []:
        if "=" not in value:
            raise PublishError(f"{flag} expects NAME=VERSION, got {value!r}")
        name, version = value.split("=", 1)
        if not name.strip() or not version.strip():
            raise PublishError(f"{flag} expects NAME=VERSION, got {value!r}")
        parsed[name.strip()] = version.strip()
    return parsed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace-root", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--package-order", nargs="*", default=None)
    parser.add_argument("--registry-dep", action="append", default=None,
                        metavar="NAME=VERSION")
    parser.add_argument("--summary", type=Path, default=None)
    args = parser.parse_args(argv)

    try:
        ordered = prepare_publish(
            args.workspace_root,
            args.out,
            args.version,
            args.package_order,
            parse_named_versions(args.registry_dep, "--registry-dep"),
        )
    except PublishError as error:
        print(f"prepare_publish: {error}", file=sys.stderr)
        return 1

    summary = args.summary or (args.out / "publish-plan.json")
    write_plan(ordered, args.version, summary)
    print(f"prepare_publish: staged {len(ordered)} package(s) for {args.version}")
    for package in ordered:
        print(f"  {package.name} {package.version} -> {package.manifest}")
    print(f"prepare_publish: plan written to {summary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
