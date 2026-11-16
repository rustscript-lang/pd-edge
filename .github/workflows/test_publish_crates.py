"""Prepare-only checks for the Python manifest rewrite in publish-crates.yml."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/publish-crates.yml"


class PublishRewriteTests(unittest.TestCase):
    def test_tag_preparation_rewrites_registry_dependencies_and_versions(self):
        # Run the exact publish preparation block, never the publishing block.
        workflow = WORKFLOW.read_text()
        script = textwrap.dedent(
            workflow.split("          python3 - <<'PY'\n", 1)[1]
            .split("          PY\n", 1)[0]
        )
        with tempfile.TemporaryDirectory() as temporary:
            checkout = Path(temporary)
            edge = checkout / "pd-edge"
            edge.mkdir()
            for relative in ("Cargo.toml", "pd-edge-abi/Cargo.toml", "pd-edge-host-function/Cargo.toml"):
                destination = edge / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes((ROOT / relative).read_bytes())
            rustscript = checkout / "rustscript"
            rustscript.mkdir()
            (rustscript / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["."]\n'
                '[workspace.package]\nversion = "0.1.0"\n'
                '[package]\nname = "rustscript"\nversion.workspace = true\n'
                '[dependencies]\npd-vm = { path = "../pd-vm", version = "0.1.0", optional = true }\n'
                'host = { package = "pd-host-function", git = "https://example.invalid/host", '
                'branch = "main", default-features = false, features = ["std"] }\n'
            )
            environment = dict(os.environ, PUBLISH_VERSION="0.2.0",
                               PD_VM_VERSION="0.22.2", PD_HOST_FUNCTION_VERSION="0.22.2",
                               WORKSPACE_VERSION_OVERRIDES="pd-edge= rustscript=0.22.2")
            subprocess.run([sys.executable, "-c", script], cwd=edge, env=environment, check=True)

            edge_manifest = tomllib.loads((edge / "Cargo.toml").read_text())
            abi_manifest = tomllib.loads((edge / "pd-edge-abi/Cargo.toml").read_text())
            host_manifest = tomllib.loads((edge / "pd-edge-host-function/Cargo.toml").read_text())
            vm = edge_manifest["dependencies"]["vm"]
            self.assertEqual(vm, {"package": "pd-vm", "version": "0.22.2",
                                  "default-features": False, "features": ["runtime"]})
            self.assertEqual(abi_manifest["dependencies"]["pd-host-function"],
                             {"version": "0.22.2"})
            self.assertEqual(edge_manifest["dependencies"]["edge_abi"],
                             {"package": "pd-edge-abi", "version": "0.2.0",
                              "default-features": False})
            self.assertEqual(edge_manifest["dependencies"]["pd-edge-host-function"],
                             {"version": "0.2.0"})
            self.assertEqual(edge_manifest["dependencies"]["tokio"]["features"], ["full"])
            self.assertEqual(edge_manifest["workspace"]["package"]["version"], "0.2.0")
            self.assertEqual(abi_manifest["package"]["version"], {"workspace": True})
            self.assertEqual(host_manifest["package"]["version"], {"workspace": True})
            rustscript_manifest = tomllib.loads((rustscript / "Cargo.toml").read_text())
            self.assertEqual(rustscript_manifest["workspace"]["package"]["version"], "0.22.2")
            self.assertEqual(rustscript_manifest["dependencies"]["pd-vm"],
                             {"version": "0.22.2", "optional": True})
            self.assertEqual(rustscript_manifest["dependencies"]["host"],
                             {"package": "pd-host-function", "version": "0.22.2",
                              "default-features": False, "features": ["std"]})


if __name__ == "__main__":
    unittest.main()
