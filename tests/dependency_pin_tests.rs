//! The pd-edge workspace must stay pinned to the frozen RustScript descriptor
//! core by exact revision, in every manifest and in the lockfile.
//!
//! Every assertion here fails on a stale pin, an abbreviated revision, a
//! sibling path pin, or a moving branch pin, so a dependency refresh cannot
//! silently retarget the migration.
//!
//! Architecture guards also fail if pd-edge re-enables `vm/edge-abi` (the
//! crates.io ABI24 universe) or if any registry `pd-edge*`, `pd-host-*`, or
//! `pd-vm*` family crate appears in Cargo.lock or feature-matrix metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

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

fn is_family_package(name: &str) -> bool {
    name == "pd-vm"
        || name.starts_with("pd-vm-")
        || name.starts_with("pd-edge")
        || name.starts_with("pd-host-")
}

fn parse_quoted_strings(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            break;
        };
        items.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    items
}

fn pd_edge_feature_table(manifest: &str) -> BTreeMap<String, Vec<String>> {
    let mut table = BTreeMap::new();
    let mut in_features = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        table.insert(name.trim().to_string(), parse_quoted_strings(value));
    }
    table
}

fn enables_vm_edge_abi(item: &str) -> bool {
    item == "vm/edge-abi" || item == "edge-abi" || item.starts_with("vm/edge-abi")
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    resolve: MetadataResolve,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
    id: String,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataResolve {
    nodes: Vec<MetadataNode>,
}

#[derive(Debug, Deserialize)]
struct MetadataNode {
    id: String,
    features: Vec<String>,
}

fn host_target_triple() -> String {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap_or_else(|error| panic!("failed to run rustc -vV: {error}"));
    assert!(
        output.status.success(),
        "rustc -vV failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("rustc -vV must be utf-8")
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .expect("rustc -vV must report the host triple")
}

fn cargo_metadata(extra: &[&str]) -> CargoMetadata {
    let host = host_target_triple();
    let output = Command::new(env!("CARGO"))
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--offline")
        .arg("--locked")
        .arg("--filter-platform")
        .arg(&host)
        .arg("--manifest-path")
        .arg(manifest_dir().join("Cargo.toml"))
        .args(extra)
        .current_dir(manifest_dir())
        .output()
        .unwrap_or_else(|error| panic!("failed to run cargo metadata: {error}"));
    assert!(
        output.status.success(),
        "cargo metadata {} failed:\n{}",
        extra.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "cargo metadata {} produced invalid JSON: {error}",
            extra.join(" ")
        )
    })
}

fn family_registry_offenders(metadata: &CargoMetadata) -> Vec<String> {
    metadata
        .packages
        .iter()
        .filter(|package| is_family_package(&package.name))
        .filter_map(|package| {
            let source = package.source.as_deref()?;
            source
                .starts_with("registry+")
                .then(|| format!("{} {} source={source}", package.name, package.version))
        })
        .collect()
}

fn pd_vm_enabled_features(metadata: &CargoMetadata) -> BTreeSet<String> {
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "pd-vm")
        .unwrap_or_else(|| panic!("cargo metadata must include the pd-vm package"));
    metadata
        .resolve
        .nodes
        .iter()
        .find(|node| node.id == package.id)
        .unwrap_or_else(|| panic!("cargo metadata must resolve pd-vm in the feature graph"))
        .features
        .iter()
        .cloned()
        .collect()
}

fn in_tree_abi25_present(metadata: &CargoMetadata) -> bool {
    metadata.packages.iter().any(|package| {
        package.name == "pd-edge-abi"
            && package.version == "0.1.0"
            && package.source.is_none()
            && !package.id.contains("registry+")
    })
}

fn feature_matrices() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("default", &[]),
        ("no-default", &["--no-default-features"]),
        ("http", &["--no-default-features", "--features", "http"]),
        ("mqtt", &["--features", "mqtt"]),
        ("all-features", &["--all-features"]),
    ]
}

#[test]
fn pd_edge_features_never_enable_vm_edge_abi() {
    let manifest = read(&manifest_dir().join("Cargo.toml"));
    let vm_line = dependency_line(&manifest, "vm");
    assert!(
        !vm_line.contains("edge-abi"),
        "the pd-vm dependency must not enable edge-abi: {vm_line}"
    );

    let table = pd_edge_feature_table(&manifest);
    assert!(
        !table.contains_key("edge-abi"),
        "pd-edge must not expose a legacy edge-abi feature: {table:?}"
    );
    for (name, enables) in &table {
        assert!(
            !enables.iter().any(|item| enables_vm_edge_abi(item)),
            "feature `{name}` re-enables the legacy pd-vm ABI24 universe: {enables:?}"
        );
    }

    let http = table
        .get("http")
        .expect("the http feature must remain declared");
    assert!(
        http.iter().any(|item| item == "edge_abi/http"),
        "http must keep enabling the in-tree ABI25 http surface: {http:?}"
    );
    assert!(
        !http.iter().any(|item| enables_vm_edge_abi(item)),
        "http must not enable vm/edge-abi: {http:?}"
    );
}

#[test]
fn feature_matrix_metadata_keeps_git_abi25_and_drops_registry_family() {
    for (label, extra) in feature_matrices() {
        let metadata = cargo_metadata(extra);
        let offenders = family_registry_offenders(&metadata);
        assert!(
            offenders.is_empty(),
            "{label} cargo metadata pulled registry family crates: {offenders:?}"
        );
        let features = pd_vm_enabled_features(&metadata);
        assert!(
            features.contains("runtime"),
            "{label} cargo metadata must keep pd-vm runtime: {features:?}"
        );
        assert!(
            !features.contains("edge-abi"),
            "{label} cargo metadata enabled pd-vm edge-abi: {features:?}"
        );
        assert!(
            in_tree_abi25_present(&metadata),
            "{label} cargo metadata must keep the in-tree ABI25 pd-edge-abi crate"
        );
    }
}
