use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=PD_BUILD_GIT_TAG");
    println!("cargo:rerun-if-env-changed=PD_BUILD_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=PD_BUILD_GIT_DIRTY");

    let git_tag = env::var("PD_BUILD_GIT_TAG").unwrap_or_else(|_| {
        run_git(["describe", "--tags", "--exact-match"]).unwrap_or_else(|| "untagged".to_string())
    });
    let git_commit = env::var("PD_BUILD_GIT_COMMIT").unwrap_or_else(|_| {
        run_git(["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".to_string())
    });
    let git_dirty = env::var("PD_BUILD_GIT_DIRTY").unwrap_or_else(|_| {
        match run_git(["status", "--porcelain", "--untracked-files=no"]) {
            Some(output) if !output.trim().is_empty() => "true".to_string(),
            _ => "false".to_string(),
        }
    });

    println!("cargo:rustc-env=PD_BUILD_GIT_TAG={git_tag}");
    println!("cargo:rustc-env=PD_BUILD_GIT_COMMIT={git_commit}");
    println!("cargo:rustc-env=PD_BUILD_GIT_DIRTY={git_dirty}");
}

fn run_git<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}
