//! Test-only guards on repository documents: every `CLAUDE.md` stays under
//! 40 KB, `docs/c-abi-surface.md` names every exported C symbol, and no
//! source comment cites a line of this repo or carries an audit tag.
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
            // checkout has CRLF line ends, one byte per line more than the
            // Linux CI runner sees.
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
/// `docs/c-abi-surface.md` (its "Symbol index" lists them all), so the
/// catalog cannot drift from the exports. Collects direct `extern "C" fn` items, `scalar_getter!` uses and
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

/// Every `.rs` file under `src/`, for the comment rule below.
fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => rust_sources(&path, found),
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "rs") => found.push(path),
            _ => {}
        }
    }
}

/// Does `comment` cite a line of this repo (`foo.rs:12`), or carry an audit
/// tag (`audit F-10`)? Both belong in `docs/dev-log.md`, not in the source
/// (the "Code comments" rule in `CLAUDE.md`).
fn breaks_comment_rule(comment: &str) -> Option<&'static str> {
    let bytes = comment.as_bytes();
    for ext in [".rs:", ".py:", ".md:", ".pyi:", ".toml:"] {
        let mut from = 0;
        while let Some(i) = comment[from..].find(ext) {
            let after = from + i + ext.len();
            if bytes.get(after).is_some_and(u8::is_ascii_digit) {
                return Some("cites a line number");
            }
            from = after;
        }
    }
    let mut from = 0;
    while let Some(i) = comment[from..].find("audit ") {
        let tag = &bytes[from + i + 6..];
        if tag.len() >= 3 && (b'A'..=b'I').contains(&tag[0]) && tag[1] == b'-' && tag[2].is_ascii_digit() {
            return Some("carries an audit tag");
        }
        from += i + 6;
    }
    None
}

#[test]
fn comments_cite_no_lines_or_audit_tags() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_sources(&root.join("src"), &mut files);
    let this = root.join("src").join("repo_doc_budget.rs");
    let mut bad = Vec::new();
    for f in files.iter().filter(|f| **f != this) {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for (n, line) in text.lines().enumerate() {
            let Some(at) = line.find("//") else { continue };
            if let Some(why) = breaks_comment_rule(&line[at..]) {
                bad.push(format!("{}:{} {why}: {}", f.strip_prefix(root).unwrap_or(f).display(), n + 1, line.trim()));
            }
        }
    }
    assert!(bad.is_empty(), "{} comment line(s) break the comment rule:\n{}", bad.len(), bad.join("\n"));
}
