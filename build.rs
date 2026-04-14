// SPDX-License-Identifier: GPL-3.0-only

//! Build script — embeds the git commit SHA via `git rev-parse` for the
//! `--version` output. Falls back to "unknown" if git is unavailable.

fn main() {
    // GITHUB_SHA is set by GitHub Actions; use it when git is unavailable (e.g.
    // musl containers used by cargo-dist don't have git installed).
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .map(|s| s[..7.min(s.len())].to_string())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_SHA={sha}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    // Re-run only when HEAD moves.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}
