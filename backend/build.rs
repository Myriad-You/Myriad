//! Stamps the myriad-core commit the site is built with, its `core/`
//! pointer, for what it says it runs on her link (`link.versions`):
//! `MYRIAD_CORE_COMMIT` when given, else `HEAD:core` of the repository it
//! is built in. Empty when neither gives 40 hex digits; the site then says
//! no versions of its own.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn main() {
    println!("cargo:rerun-if-env-changed=MYRIAD_CORE_COMMIT");
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("..");
    // Read again when HEAD moves (a commit, another checkout): its file,
    // the branch it is on, and the packed refs. Only files that are there,
    // or every build would read it again.
    let branch = git(&root, &["symbolic-ref", "-q", "HEAD"]);
    for name in ["HEAD", "packed-refs"]
        .into_iter()
        .map(str::to_string)
        .chain(branch)
    {
        if let Some(file) = git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", &name],
        ) && Path::new(&file).exists()
        {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    let commit = std::env::var("MYRIAD_CORE_COMMIT")
        .ok()
        .or_else(|| git(&root, &["rev-parse", "HEAD:core"]))
        .map(|commit| commit.trim().to_lowercase())
        .filter(|commit| commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_default();
    println!("cargo:rustc-env=MYRIAD_SITE_CORE_COMMIT={commit}");
}
