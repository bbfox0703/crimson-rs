//! Test-only guard on the size of every `CLAUDE.md` in the repository.
//!
//! Each `CLAUDE.md` is loaded into every Claude Code turn that touches its
//! directory, so the root `CLAUDE.md` sets a hard limit of 40 KB per file
//! (history belongs in `docs/dev-log.md`, per-patch detail in
//! `docs/validated-patches.md`). This test is what makes CI enforce it.

use std::path::{Path, PathBuf};

const LIMIT_BYTES: u64 = 40 * 1024;

/// Directories that hold no tracked `CLAUDE.md` of this repo (build output,
/// virtualenvs, Claude worktrees that are other checkouts).
const SKIP_DIRS: &[&str] = &[".git", ".claude", ".venv", "target", "out", "node_modules"];

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with(".crimson_rs_") {
                    collect(&path, found);
                }
            }
            Ok(t) if t.is_file() && name == "CLAUDE.md" => found.push(path),
            _ => {}
        }
    }
}

#[test]
fn claude_md_files_stay_under_40_kb() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    collect(root, &mut found);
    assert!(
        found.iter().any(|p| p == &root.join("CLAUDE.md")),
        "root CLAUDE.md not found under {}",
        root.display()
    );

    let over: Vec<String> = found
        .iter()
        .filter_map(|p| {
            let len = std::fs::metadata(p).ok()?.len();
            (len > LIMIT_BYTES).then(|| format!("{} is {len} bytes", p.display()))
        })
        .collect();
    assert!(
        over.is_empty(),
        "CLAUDE.md over the {LIMIT_BYTES}-byte limit (move history to docs/dev-log.md or \
         docs/validated-patches.md, trim, never grow):\n{}",
        over.join("\n")
    );
}
