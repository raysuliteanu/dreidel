// SPDX-License-Identifier: GPL-3.0-only

fn main() {
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();

    // Embed the jj change ID at build time for display in the help dialog and --version output.
    // Silently skipped when jj is not available or the repo has no working copy.
    let change_id = std::process::Command::new("jj")
        .args(["log", "-r", "@", "--no-graph", "-T", "change_id.short(8)"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if id.is_empty() { None } else { Some(id) }
        });

    if let Some(ref id) = change_id {
        println!("cargo:rustc-env=JJ_CHANGE_ID={id}");
        println!("cargo:rustc-env=TOPPERS_VERSION={pkg_version} ({id})");
    } else {
        println!("cargo:rustc-env=TOPPERS_VERSION={pkg_version}");
    }

    // Rebuild when jj state changes (best-effort — .jj/working_copy/checkout is updated on commit)
    println!("cargo:rerun-if-changed=.jj/working_copy/checkout");
    println!("cargo:rerun-if-changed=build.rs");
}
