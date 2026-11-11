//! Guard rails: every workspace crate must inherit the shared lints and forbid unsafe code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn crate_dirs() -> Vec<PathBuf> {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut dirs: Vec<PathBuf> = fs::read_dir(crates_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_crate_inherits_workspace_lints() {
    for dir in crate_dirs() {
        let manifest = fs::read_to_string(dir.join("Cargo.toml"))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            manifest.contains("[lints]\nworkspace = true"),
            "{} must contain `[lints]` followed by `workspace = true`",
            dir.display()
        );
    }
}

#[test]
fn every_crate_root_forbids_unsafe_code() {
    for dir in crate_dirs() {
        let roots: Vec<PathBuf> = ["src/lib.rs", "src/main.rs"]
            .iter()
            .map(|file| dir.join(file))
            .filter(|path| path.exists())
            .collect();
        assert!(
            !roots.is_empty(),
            "{} has no src/lib.rs or src/main.rs",
            dir.display()
        );
        for root in roots {
            let source = fs::read_to_string(&root).unwrap();
            assert!(
                source.contains("#![forbid(unsafe_code)]"),
                "{} must contain `#![forbid(unsafe_code)]`",
                root.display()
            );
        }
    }
}
