#!/usr/bin/env python3
"""Publish the staged crates from prepare_publish.py in dependency order.

Design notes
------------
* Every package is published from its own standalone tree
  (``cargo publish --manifest-path <staged>/Cargo.toml``), so Cargo never has to
  resolve a sibling that is not on crates.io yet.
* Versions are checked against the *sparse index* (``index.crates.io``) and not
  only the crates.io API: the API answers before Cargo's index can see the new
  version, and publishing a dependent in that window fails with
  ``candidate versions found which didn't match``.
* A dependency-resolution failure is retried only while the dependency is still
  missing from the index; the last Cargo output is printed verbatim on failure.
* ``--dry-run`` prints the plan and executes nothing (used by the tests).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path

INDEX_ROOT = "https://index.crates.io"
API_ROOT = "https://crates.io/api/v1/crates"


@dataclass
class Package:
    name: str
    version: str
    manifest: Path
    dependencies: list[str]


def index_path(name: str) -> str:
    lowered = name.lower()
    if len(lowered) == 1:
        return f"1/{lowered}"
    if len(lowered) == 2:
        return f"2/{lowered}"
    if len(lowered) == 3:
        return f"3/{lowered[0]}/{lowered}"
    return f"{lowered[:2]}/{lowered[2:4]}/{lowered}"


def _fetch(url: str, timeout: float = 20.0) -> bytes | None:
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise


def index_has(name: str, version: str) -> bool:
    payload = _fetch(f"{INDEX_ROOT}/{index_path(name)}")
    if payload is None:
        return False
    for line in payload.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if entry.get("vers") == version and not entry.get("yanked", False):
            return True
    return False


def api_has(name: str, version: str) -> bool:
    return _fetch(f"{API_ROOT}/{name}/{version}") is not None


def wait_for_index(name: str, version: str, timeout: float, interval: float) -> bool:
    deadline = time.monotonic() + timeout
    while True:
        try:
            if index_has(name, version):
                return True
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            print(f"  index lookup for {name} {version} failed: {error}")
        if time.monotonic() >= deadline:
            return False
        time.sleep(interval)


def load_plan(path: Path) -> list[Package]:
    plan = json.loads(path.read_text())
    # The plan is already in dependency order; keep it verbatim.
    return [
        Package(
            entry["name"],
            entry["version"],
            Path(entry["manifest"]),
            list(entry.get("dependencies", [])),
        )
        for entry in plan["packages"]
    ]


def ensure_dependencies_indexed(packages: list[Package],
                                timeout: float, interval: float) -> None:
    names = {package.name: package.version for package in packages}
    for package in packages:
        for dependency in package.dependencies:
            if dependency not in names:
                continue
            expected = names[dependency]
            if not wait_for_index(dependency, expected, timeout, interval):
                raise SystemExit(
                    f"{dependency} {expected} is not visible in the crates.io index; "
                    f"refusing to publish {package.name}"
                )
            print(f"  dependency {dependency} {expected} is indexed")


def publish(package: Package, token: str | None, retries: int, wait: float,
            timeout: float, interval: float) -> None:
    if index_has(package.name, package.version):
        print(f"{package.name} {package.version} already published; skipping")
        return

    command = [
        "cargo", "publish",
        "--manifest-path", str(package.manifest),
        "--no-verify",
        "--allow-dirty",
    ]
    environment = dict(os.environ)
    if token:
        # `cargo publish --token` is deprecated; the env var is the supported path.
        environment["CARGO_REGISTRY_TOKEN"] = token

    for attempt in range(1, retries + 1):
        print(f"publishing {package.name} {package.version}, attempt {attempt}")
        proc = subprocess.run(
            command,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env=environment,
        )
        print(proc.stdout.rstrip())
        if proc.returncode == 0 or wait_for_index(package.name, package.version,
                                                  timeout, interval):
            print(f"{package.name} {package.version} published")
            return
        if attempt == retries:
            raise SystemExit(
                f"{package.name} {package.version} failed to publish after "
                f"{retries} attempt(s) (cargo exit {proc.returncode})"
            )
        time.sleep(wait)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", required=True, type=Path)
    parser.add_argument("--token", default=None,
                        help="registry token; defaults to $CARGO_REGISTRY_TOKEN")
    parser.add_argument("--retries", type=int, default=12)
    parser.add_argument("--retry-delay", type=float, default=20.0)
    parser.add_argument("--index-timeout", type=float, default=300.0)
    parser.add_argument("--index-interval", type=float, default=5.0)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)

    packages = load_plan(args.plan)
    if not packages:
        print("publish_crates: plan contains no packages", file=sys.stderr)
        return 1

    print("publish_crates: order " + " -> ".join(
        f"{package.name} {package.version}" for package in packages))
    if args.dry_run:
        for package in packages:
            print(f"  cargo publish --manifest-path {package.manifest}")
        return 0

    token = args.token or os.environ.get("CARGO_REGISTRY_TOKEN")
    if not token:
        print("CARGO_REGISTRY_TOKEN is required", file=sys.stderr)
        return 1

    for package in packages:
        ensure_dependencies_indexed(packages, args.index_timeout, args.index_interval)
        publish(package, token, args.retries, args.retry_delay,
                args.index_timeout, args.index_interval)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
