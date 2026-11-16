"""Regression tests for the crates.io publish pipeline.

The 0.2.0 tag run failed 18 times with::

    error: failed to select a version for the requirement `pd-edge-abi = "^0.2.0"`
    candidate versions found which didn't match: 0.1.1
    required by package `pd-edge v0.2.0`

because the old workflow rewrote path dependencies into registry requirements
*inside* the workspace: `cargo publish -p pd-edge-abi` then had to resolve the
workspace member `pd-edge`, which already demanded the not-yet-published
`pd-edge-abi 0.2.0`.

These tests run the replacement prepare/publish helpers against verbatim copies
of the 0.2.0 release manifests (``.github/scripts/fixtures/pd-edge-0.2.0``).
Nothing is published and no network is used.
"""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/publish-crates.yml"
SCRIPTS = ROOT / ".github/scripts"
FIXTURE = SCRIPTS / "fixtures/pd-edge-0.2.0"

sys.path.insert(0, str(SCRIPTS))

import prepare_publish  # noqa: E402
import publish_crates  # noqa: E402


RELEASE = "0.2.0"
REGISTRY_DEPS = {"pd-vm": "0.22.2", "pd-host-function": "0.22.2"}
ORDER = ["pd-edge-host-function", "pd-edge-abi", "pd-edge"]


def run_prepare(workspace: Path, out_dir: Path, package_order=None, registry_deps=None):
    return prepare_publish.prepare_publish(
        workspace,
        out_dir,
        RELEASE,
        package_order if package_order is not None else ORDER,
        registry_deps if registry_deps is not None else REGISTRY_DEPS,
    )


def load_staged(out_dir: Path, name: str) -> dict:
    return tomllib.loads((out_dir / name / "Cargo.toml").read_text())


class PreparePublishTests(unittest.TestCase):
    def test_staged_tree_is_resolvable_without_unpublished_siblings(self):
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            ordered = run_prepare(FIXTURE, out_dir)
            self.assertEqual([package.name for package in ordered], ORDER)

            abi = load_staged(out_dir, "pd-edge-abi")
            edge = load_staged(out_dir, "pd-edge")

            # The root cause of the 0.2.0 failure: the tree that publishes
            # pd-edge-abi must not require anything that is published later.
            abi_deps = set(abi.get("dependencies", {})) | set(abi.get("build-dependencies", {}))
            self.assertEqual(abi_deps & set(ORDER), set(),
                             "pd-edge-abi must not depend on a crate released after it")
            self.assertEqual(abi["dependencies"]["pd-host-function"], {"version": "0.22.2"})

            # pd-edge keeps the crates.io-visible names/features but no selectors.
            self.assertEqual(
                edge["dependencies"]["edge_abi"],
                {"package": "pd-edge-abi", "version": RELEASE, "default-features": False},
            )
            self.assertEqual(
                edge["dependencies"]["vm"],
                {"package": "pd-vm", "version": "0.22.2", "default-features": False,
                 "features": ["runtime"]},
            )
            self.assertEqual(edge["dependencies"]["pd-edge-host-function"], {"version": RELEASE})
            self.assertEqual(edge["dependencies"]["tokio"]["features"], ["full"])

            # dev-dependencies never reach crates.io and must not gate a release.
            self.assertNotIn("dev-dependencies", edge)
            self.assertNotIn("reqwest", json.dumps(edge["dependencies"]))

    def test_versions_are_materialised_per_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            run_prepare(FIXTURE, out_dir)
            for name in ORDER:
                manifest = load_staged(out_dir, name)
                self.assertEqual(manifest["package"]["version"], RELEASE)
                self.assertEqual(manifest["package"]["name"], name)
                # Standalone workspace root: no stale workspace inheritance.
                self.assertIn("workspace", manifest)
                self.assertNotIn("package", manifest["workspace"])
            self.assertEqual(load_staged(out_dir, "pd-edge-abi")["package"]["edition"], "2024")

    def test_no_path_or_git_selectors_survive(self):
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            run_prepare(FIXTURE, out_dir)
            for name in ORDER:
                manifest = load_staged(out_dir, name)
                for dependency, spec in prepare_publish._iter_dependency_specs(manifest):
                    if isinstance(spec, dict):
                        self.assertFalse(
                            set(prepare_publish.SOURCE_KEYS) & set(spec),
                            f"{name}: {dependency} kept a path/git selector: {spec}",
                        )
                        self.assertTrue(spec.get("version"), f"{name}: {dependency} has no version")

    def test_unknown_git_dependency_fails_loudly(self):
        # Without a registry version the rewrite must stop instead of silently
        # shipping an unresolvable dependency to crates.io.
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(prepare_publish.PublishError):
                run_prepare(FIXTURE, Path(temporary) / "stage", registry_deps={})

    def test_package_order_is_dependency_sorted(self):
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            ordered = run_prepare(FIXTURE, out_dir, package_order=list(reversed(ORDER)))
            self.assertEqual([package.name for package in ordered], ORDER)

    def test_package_order_must_match_workspace(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(prepare_publish.PublishError):
                run_prepare(FIXTURE, Path(temporary) / "stage",
                            package_order=["pd-edge-abi", "pd-edge"])

    def test_live_manifests_prepare_cleanly(self):
        # Guards against manifest drift: the checked-out 0.2.0 tree must stage
        # with every source selector removed and the release version applied.
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            ordered = run_prepare(ROOT, out_dir)
            self.assertEqual([package.name for package in ordered], ORDER)
            for name in ORDER:
                manifest = load_staged(out_dir, name)
                self.assertEqual(manifest["package"]["version"], RELEASE)
                for dependency, spec in prepare_publish._iter_dependency_specs(manifest):
                    if isinstance(spec, dict):
                        self.assertFalse(set(prepare_publish.SOURCE_KEYS) & set(spec),
                                         f"{name}: {dependency} kept a path/git selector")


class PublishPlanTests(unittest.TestCase):
    def test_cli_writes_plan_and_dry_run_keeps_order(self):
        with tempfile.TemporaryDirectory() as temporary:
            out_dir = Path(temporary) / "stage"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                exit_code = prepare_publish.main([
                    "--workspace-root", str(FIXTURE),
                    "--out", str(out_dir),
                    "--version", RELEASE,
                    "--package-order", *ORDER,
                    "--registry-dep", "pd-vm=0.22.2",
                    "--registry-dep", "pd-host-function=0.22.2",
                ])
            self.assertEqual(exit_code, 0, stdout.getvalue())

            plan_path = out_dir / "publish-plan.json"
            plan = json.loads(plan_path.read_text())
            self.assertEqual([entry["name"] for entry in plan["packages"]], ORDER)
            self.assertEqual(sorted(plan["packages"][1]["dependencies"]),
                             ["pd-host-function", "syn"])

            dry_run = subprocess.run(
                [sys.executable, str(SCRIPTS / "publish_crates.py"),
                 "--plan", str(plan_path), "--dry-run"],
                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=True,
            )
            self.assertEqual(dry_run.returncode, 0)
            lines = [line for line in dry_run.stdout.splitlines() if "cargo publish" in line]
            self.assertEqual(len(lines), 3)
            self.assertIn("pd-edge-host-function", lines[0])
            self.assertIn("pd-edge-abi", lines[1])
            self.assertIn("/pd-edge/Cargo.toml", lines[2])
            self.assertIn("--manifest-path", lines[2])

    def test_index_paths_match_the_sparse_registry_layout(self):
        self.assertEqual(publish_crates.index_path("pd-edge-abi"), "pd/-e/pd-edge-abi")
        self.assertEqual(publish_crates.index_path("pd-vm"), "pd/-v/pd-vm")
        self.assertEqual(publish_crates.index_path("ab"), "2/ab")


class WorkflowWiringTests(unittest.TestCase):
    def test_workflow_uses_the_staged_publishers(self):
        workflow = yaml.safe_load(WORKFLOW.read_text())
        steps = workflow["jobs"]["publish"]["steps"]
        scripts = "\n".join(step.get("run", "") for step in steps)
        self.assertIn(".github/scripts/prepare_publish.py", scripts)
        self.assertIn(".github/scripts/publish_crates.py", scripts)
        # The in-place rewrite that caused the deadlock must be gone.
        self.assertNotIn("rglob('Cargo.toml')", scripts)
        self.assertNotIn("cargo publish -p", scripts)
        self.assertIn("--manifest-path", (SCRIPTS / "publish_crates.py").read_text())

    def test_tag_trigger_still_covers_versions(self):
        workflow = yaml.safe_load(WORKFLOW.read_text())
        # PyYAML maps the bare `on:` key to True.
        triggers = workflow.get("on") or workflow.get(True)
        self.assertIn("v*", triggers["push"]["tags"])
        self.assertIn("[0-9]*", triggers["push"]["tags"])


if __name__ == "__main__":
    unittest.main()
