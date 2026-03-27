fn main() {
    // Embed the jj change ID at build time for display in the help dialog.
    // Silently skipped when jj is not available or the repo has no working copy.
    if let Ok(output) = std::process::Command::new("jj")
        .args(["log", "-r", "@", "--no-graph", "-T", "change_id.short(8)"])
        .output()
        && output.status.success()
    {
        let id = String::from_utf8_lossy(&output.stdout);
        let id = id.trim();
        if !id.is_empty() {
            println!("cargo:rustc-env=JJ_CHANGE_ID={id}");
        }
    }
    // Rebuild when jj state changes (best-effort — .jj/working_copy/checkout is updated on commit)
    println!("cargo:rerun-if-changed=.jj/working_copy/checkout");
    println!("cargo:rerun-if-changed=build.rs");
}
