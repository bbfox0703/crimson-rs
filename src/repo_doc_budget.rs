//! Test-only guards on repository documents: every `CLAUDE.md` stays under
//! 40 KB, and `docs/c-abi-surface.md` names every exported C symbol.
//!
//! On the size guard:
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
            // Count the bytes git stores: with `core.autocrlf` a Windows
            // checkout has CRLF line ends, which made the local count one byte
            // per line larger than on the Linux CI runner (audit I-13).
            let bytes = std::fs::read(p).ok()?;
            let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();
            let len = (bytes.len() - crlf) as u64;
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

/// Every exported `crimson_*` function must be named somewhere in
/// `docs/c-abi-surface.md` (its "Symbol index" lists them all). The catalog
/// had drifted to 272 of 311 symbols missing (audit A-18, C-14, D-11, E-14,
/// G-16). Collects direct `extern "C" fn` items, `scalar_getter!` uses and
/// the names passed to `impl_name_only_bridge!`.
#[test]
fn c_abi_surface_lists_every_symbol() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = std::fs::read_to_string(root.join("docs").join("c-abi-surface.md")).expect("c-abi-surface.md");
    let mut symbols = Vec::new();
    for entry in std::fs::read_dir(root.join("src").join("c_abi")).expect("src/c_abi").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut in_macro_call = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("crate::impl_name_only_bridge! {") {
                in_macro_call = true;
            } else if in_macro_call && t == "}" {
                in_macro_call = false;
            }
            let name = if let Some(rest) = t.split("extern \"C\" fn ").nth(1).filter(|_| t.starts_with("pub ")) {
                rest
            } else if let Some(rest) = t.strip_prefix("scalar_getter!(") {
                rest
            } else if in_macro_call {
                match t.split_once(" = ") {
                    Some((_, v)) => v,
                    None => continue,
                }
            } else {
                continue;
            };
            let ident: String = name.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if ident.starts_with("crimson_") {
                symbols.push(ident);
            }
        }
    }
    assert!(symbols.len() > 300, "found only {} symbols; the scan is broken", symbols.len());
    let missing: Vec<&String> = symbols
        .iter()
        .filter(|s| !doc.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| w == s.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{} exported symbols are not named in docs/c-abi-surface.md (add them to its Symbol index):\n{}",
        missing.len(),
        missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
}
