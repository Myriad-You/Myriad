//! Stamps the upstream commit the core was built from, for `meta.version`.
//! A platform building from an archive of one commit passes it in
//! `MYRIAD_CORE_UPSTREAM_COMMIT`; a workspace build says `unknown`.

fn main() {
    println!("cargo:rerun-if-env-changed=MYRIAD_CORE_UPSTREAM_COMMIT");
    let commit = std::env::var("MYRIAD_CORE_UPSTREAM_COMMIT")
        .ok()
        .filter(|commit| !commit.trim().is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!(
        "cargo:rustc-env=MYRIAD_CORE_UPSTREAM_COMMIT={}",
        commit.trim()
    );
}
