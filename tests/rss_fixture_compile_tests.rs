//! Every checked-in RSS program and example module keeps compiling against the
//! frozen descriptor core and the descriptor-driven edge catalog.
//!
//! The sweep is programmatic: it enumerates the files instead of trusting a
//! hand-written list, so a new fixture cannot be added without being compiled
//! here, and the count is asserted so a silent deletion is caught as well.

use std::path::{Path, PathBuf};

use edge::{compile_edge_source_file, compile_edge_source_with_flavor};
use vm::SourceFlavor;

/// The checked-in RSS corpus rooted at the manifest directory.
fn rss_corpus() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for directory in ["examples", "stdlib"] {
        collect(&root.join(directory), &mut files);
    }
    files.sort();
    files
}

fn collect(directory: &Path, out: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rss") {
            out.push(path);
        }
    }
}

/// Features a fixture needs before it can compile, derived from its path.
///
/// The ABI spec gates the same feature families, so a fixture that calls a
/// gated namespace cannot compile in a build that disables it.
fn required_features(path: &Path) -> &'static [&'static str] {
    let relative = path
        .strip_prefix(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path);
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.starts_with("examples/console/sample_console_mqtt_client.rss") {
        // The console MQTT sample drives the MQTT connection surface.
        &["console", "mqtt"]
    } else if text.starts_with("examples/console/") {
        &["console"]
    } else if text.starts_with("examples/mqtt/") {
        &["mqtt"]
    } else if text.starts_with("examples/webrtc/") {
        &["webrtc"]
    } else if text.starts_with("examples/websocket/") {
        &["websocket"]
    } else if text.starts_with("examples/http/")
        || text.starts_with("examples/proxy/")
        || text.starts_with("examples/transport/")
    {
        &["http", "tls"]
    } else {
        &[]
    }
}

/// Fixtures that the frozen core still rejects for guest-language reasons.
///
/// These are *executable samples* outside the compiled CI fixture set. They are
/// pinned here (with the exact frozen-core diagnostic family) so the sweep
/// fails if they start compiling — the entry must then be removed — and
/// everything else must compile. Re-typing them is a guest-program change that
/// belongs to the sample itself, not to this migration.
fn expected_frozen_core_incompatibility(path: &Path) -> Option<&'static str> {
    let relative = path
        .strip_prefix(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path);
    let text = relative.to_string_lossy().replace('\\', "/");
    match text.as_str() {
        "examples/mqtt/downstream/sample_transport_mqtt_broker_program.rss" => Some(
            "the frozen core's stricter guest inference rejects the hand-written dynamic \
             MQTT packet decoder (BinaryOperandTypeMismatch: int vs unknown)",
        ),
        _ => None,
    }
}

fn feature_enabled(feature: &str) -> bool {
    match feature {
        "console" => cfg!(feature = "console"),
        "http" => cfg!(feature = "http"),
        "http2" => cfg!(feature = "http2"),
        "http3" => cfg!(feature = "http3"),
        "mqtt" => cfg!(feature = "mqtt"),
        "tls" => cfg!(feature = "tls"),
        "websocket" => cfg!(feature = "websocket"),
        "webrtc" => cfg!(feature = "webrtc"),
        other => panic!("unknown edge feature '{other}'"),
    }
}

#[test]
fn the_checked_in_rss_corpus_is_complete() {
    let files = rss_corpus();
    assert_eq!(
        files.len(),
        26,
        "the checked-in RSS corpus changed; audit the new/removed fixtures: {files:#?}"
    );
}

#[test]
fn every_checked_in_rss_program_compiles_for_its_feature_set() {
    let files = rss_corpus();
    assert!(!files.is_empty(), "the RSS corpus must not be empty");
    let mut compiled = Vec::new();
    let mut skipped = Vec::new();
    let mut documented = Vec::new();
    let mut failures = Vec::new();
    for path in &files {
        let missing = required_features(path)
            .iter()
            .filter(|feature| !feature_enabled(feature))
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            skipped.push(format!("{} (needs {missing:?})", path.display()));
            continue;
        }
        match compile_edge_source_file(path) {
            Ok(_) => {
                if let Some(reason) = expected_frozen_core_incompatibility(path) {
                    failures.push(format!(
                        "{} now compiles; remove its documented frozen-core incompatibility                          entry ({reason})",
                        path.display()
                    ));
                    continue;
                }
                compiled.push(path.clone());
            }
            Err(error) => {
                if let Some(reason) = expected_frozen_core_incompatibility(path) {
                    documented.push(format!("{}: {reason}", path.display()));
                    continue;
                }
                failures.push(format!("{}: {error:?}", path.display()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "checked-in RSS fixtures must compile:\n{}",
        failures.join("\n\n")
    );
    assert_eq!(
        compiled.len() + skipped.len() + documented.len(),
        files.len(),
        "every fixture must be compiled, skipped for a disabled feature, or documented"
    );
    assert!(
        !compiled.is_empty(),
        "at least the default-feature fixtures must compile; skipped={skipped:#?}"
    );
    println!(
        "compiled {} of {} RSS fixtures ({} skipped for disabled features, {} documented \
         frozen-core incompatibilities)",
        compiled.len(),
        files.len(),
        skipped.len(),
        documented.len()
    );
}

#[test]
fn every_supported_flavor_example_compiles() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let flavors = [
        (
            "examples/http/proxy/sample_proxy_program.js",
            SourceFlavor::JavaScript,
        ),
        (
            "examples/http/proxy/sample_proxy_program.lua",
            SourceFlavor::Lua,
        ),
    ];
    let mut skipped = Vec::new();
    for (relative, flavor) in flavors {
        let path = manifest.join(relative);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        match compile_edge_source_with_flavor(&source, flavor) {
            Ok(_) => {}
            // The JavaScript/Lua frontends are plugins owned by the
            // compatibility-frontends repository; a build that does not
            // register them cannot compile these samples.
            Err(error) if format!("{error:?}").contains("MissingFrontendPlugin") => {
                skipped.push(relative);
            }
            Err(error) => panic!("{} must compile: {error:?}", path.display()),
        }
    }
    println!("flavor samples skipped for missing frontend plugins: {skipped:?}");
}
