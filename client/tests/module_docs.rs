//! Every module says what it is for.
//!
//! A convention rather than something the compiler checks: `#![warn(missing_docs)]` does nothing
//! here — it lints the items a *library* exports, and a window exports nothing — so a test that
//! reads the files is the only thing that can hold this. It walks `src` rather than naming files,
//! so a module added later is covered by having been added.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_module_says_what_it_is_for() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut missing = Vec::new();

    collect(&root, &mut files);

    for file in files {
        let source = fs::read_to_string(&file).expect("a source file");

        // `//!` at the top of the file: the comment that belongs to the module rather than to
        // whatever item comes first in it.
        if !source.starts_with("//!") {
            let name = file.strip_prefix(&root).unwrap_or(&file);
            missing.push(name.display().to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "no module comment at the top of: {missing:?}"
    );
}

/// Every `.rs` file under `dir`, however deep.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("a directory entry").path();

        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            out.push(path);
        }
    }
}
