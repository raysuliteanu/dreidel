// SPDX-License-Identifier: GPL-3.0-only

fn main() {
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();

    // Embed the git commit hash at build time for display in the help dialog and --version output.
    // Silently skipped when git is not available or there is no commit yet.
    let commit_id = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if id.is_empty() { None } else { Some(id) }
        });

    if let Some(ref id) = commit_id {
        println!("cargo:rustc-env=GIT_COMMIT_ID={id}");
        println!("cargo:rustc-env=DREIDEL_VERSION={pkg_version} ({id})");
    } else {
        println!("cargo:rustc-env=DREIDEL_VERSION={pkg_version}");
    }

    // Rebuild when HEAD changes (covers commits and branch switches)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
}
