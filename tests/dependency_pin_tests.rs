//! The pd-edge workspace must stay pinned to the frozen RustScript descriptor
//! core by exact revision, in every manifest and in the lockfile.
//!
//! Every assertion here fails on a stale pin, an abbreviated revision, a
//! sibling path pin, or a moving branch pin, so a dependency refresh cannot
//! silently retarget the migration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The frozen core candidate this workspace migrates against.
const FROZEN_CORE_REV: &str = "b1d6cffede77f49410bf63525f30b9a46b02dc01";
const FROZEN_CORE_URL: &str = "https://github.com/rustscript-lang/rustscript.git";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// Returns the manifest line that declares `dependency`.
fn dependency_line(manifest: &str, dependency: &str) -> String {
    manifest
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{dependency} = ")))
        .unwrap_or_else(|| panic!("{dependency} dependency is missing from the manifest"))
        .to_string()
}

#[test]
fn pd_vm_is_pinned_to_the_frozen_core_revision() {
    let manifest = read(&manifest_dir().join("Cargo.toml"));
    let line = dependency_line(&manifest, "vm");
    assert!(
        line.contains("package = \"pd-vm\""),
        "the `vm` dependency must name the pd-vm package: {line}"
    );
    assert!(
        line.contains(&format!("git = \"{FROZEN_CORE_URL}\"")),
        "the `vm` dependency must come from the frozen core repository: {line}"
    );
    assert!(
        line.contains(&format!("rev = \"{FROZEN_CORE_REV}\"")),
        "the `vm` dependency must carry the exact frozen revision: {line}"
    );
    assert!(
        !line.contains("path = \""),
        "the `vm` dependency must not use a sibling path pin: {line}"
    );
    assert!(
        !line.contains("branch = "),
        "the `vm` dependency must not follow a moving branch: {line}"
    );
}

#[test]
fn pd_host_function_is_pinned_to_the_frozen_core_revision() {
    let manifest = read(&manifest_dir().join("pd-edge-abi/Cargo.toml"));
    let line = dependency_line(&manifest, "pd-host-function");
    assert!(
        line.contains(&format!("git = \"{FROZEN_CORE_URL}\"")),
        "pd-host-function must come from the frozen core repository: {line}"
    );
    assert!(
        line.contains(&format!("rev = \"{FROZEN_CORE_REV}\"")),
        "pd-host-function must carry the exact frozen revision: {line}"
    );
    assert!(
        !line.contains("path = \""),
        "pd-host-function must not use a sibling path pin: {line}"
    );
}

#[test]
fn no_workspace_manifest_uses_a_sibling_core_path_pin() {
    let mut manifests = vec![manifest_dir().join("Cargo.toml")];
    for entry in std::fs::read_dir(manifest_dir()).expect("workspace root should be readable") {
        let path = entry.expect("workspace entry should be readable").path();
        if path.is_dir() {
            let candidate = path.join("Cargo.toml");
            if candidate.is_file() {
                manifests.push(candidate);
            }
        }
    }
    let mut offenders = Vec::new();
    for manifest in manifests {
        for (index, line) in read(&manifest).lines().enumerate() {
            if line.contains("path = \"../rustscript") || line.contains("path = \"../../rustscript")
            {
                offenders.push(format!("{}:{}: {line}", manifest.display(), index + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "sibling core path pins must not exist: {offenders:?}"
    );
}

#[test]
fn the_lockfile_resolves_both_core_packages_to_the_frozen_revision() {
    let lock = read(&manifest_dir().join("Cargo.lock"));
    let expected_source = format!("git+{FROZEN_CORE_URL}?rev={FROZEN_CORE_REV}#{FROZEN_CORE_REV}");
    for package in ["pd-vm", "pd-host-function"] {
        let mut resolved = None;
        let mut lines = lock.lines();
        while let Some(line) = lines.next() {
            if line.trim() == format!("name = \"{package}\"") {
                let version = lines
                    .next()
                    .expect("a locked package has a version line")
                    .trim()
                    .to_string();
                let source = lines.next().unwrap_or_default().trim().to_string();
                if version == "version = \"0.1.0\"" && source.contains("rustscript.git") {
                    resolved = Some(source);
                    break;
                }
            }
        }
        let source = resolved
            .unwrap_or_else(|| panic!("Cargo.lock must resolve {package} from the frozen core"));
        assert_eq!(
            source,
            format!("source = \"{expected_source}\""),
            "{package} must resolve to the exact frozen revision"
        );
    }
}

#[test]
fn every_locked_core_revision_is_the_frozen_one() {
    let lock = read(&manifest_dir().join("Cargo.lock"));
    let revisions = lock
        .lines()
        .filter_map(|line| line.trim().strip_prefix("source = \"git+"))
        .filter(|source| source.contains("rustscript.git"))
        .collect::<BTreeSet<_>>();
    assert!(
        !revisions.is_empty(),
        "the lockfile must resolve at least one core git dependency"
    );
    for source in revisions {
        assert!(
            source.contains(&format!("?rev={FROZEN_CORE_REV}#{FROZEN_CORE_REV}")),
            "a stale core revision is locked: {source}"
        );
    }
}
