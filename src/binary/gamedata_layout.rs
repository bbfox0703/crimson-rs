//! Where the static-info gamedata tables live in the live install.
//!
//! Crimson Desert 2.01 renamed the `0008` gamedata directory and every one
//! of its file extensions:
//!
//! ```text
//! 1.05 - 2.00   gamedata/binary__/client/bin/<table>.pabgb / .pabgh
//! 2.01+         gamedata/binarystaticinfo__/bin/<table>.staticinfobody
//!                                              /<table>.staticinfoheader
//! ```
//!
//! The file *contents* did not change — bodies and PABGH indices parse
//! byte-identically either way — so the parsers are untouched and only the
//! lookup path moved. The live-install tests resolve the layout once from
//! the install on disk and fall back to the older one, which keeps them
//! working against a pre-2.01 install (what the cross-version RE workflow
//! in `docs/historical-parser-setup.md` runs against).
//!
//! Test-only: the library itself never hardcodes an archive path — every
//! public entry point takes the directory from its caller.

use std::path::PathBuf;
use std::sync::OnceLock;

struct Layout {
    dir: &'static str,
    body_ext: &'static str,
    header_ext: &'static str,
}

/// Newest layout first.
const LAYOUTS: [Layout; 2] = [
    Layout {
        dir: "gamedata/binarystaticinfo__/bin",
        body_ext: "staticinfobody",
        header_ext: "staticinfoheader",
    },
    Layout {
        dir: "gamedata/binary__/client/bin",
        body_ext: "pabgb",
        header_ext: "pabgh",
    },
];

/// Live game install the tests read from. Overridable so a second install
/// (e.g. a kept previous version) can be pointed at without editing tests.
pub(crate) fn game_root() -> PathBuf {
    std::env::var_os("CRIMSON_GAME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert"))
}

/// Resolve once against `0008/0.pamt`. When the install is missing (CI, a
/// fresh checkout) this reports the newest layout — the callers all bail
/// out on the missing PAMT before the answer matters.
fn resolved() -> &'static Layout {
    static CACHE: OnceLock<usize> = OnceLock::new();
    let idx = *CACHE.get_or_init(|| {
        let Ok(bytes) = std::fs::read(game_root().join("0008").join("0.pamt")) else {
            return 0;
        };
        let Ok(pamt) = crate::binary::pamt::PackMeta::parse(&bytes, None) else {
            return 0;
        };
        LAYOUTS
            .iter()
            .position(|l| pamt.directories.iter().any(|d| d.path == l.dir))
            .unwrap_or(0)
    });
    &LAYOUTS[idx]
}

/// Directory holding the static-info tables inside group `0008`.
pub(crate) fn bin_dir() -> &'static str {
    resolved().dir
}

/// Table body filename: `"skill"` -> `"skill.pabgb"` / `"skill.staticinfobody"`.
pub(crate) fn body(stem: &str) -> String {
    format!("{}.{}", stem, resolved().body_ext)
}

/// Table index filename: `"skill"` -> `"skill.pabgh"` / `"skill.staticinfoheader"`.
pub(crate) fn header(stem: &str) -> String {
    format!("{}.{}", stem, resolved().header_ext)
}

// ── Localization ───────────────────────────────────────────────────────────
//
// 2.01 also split each language's single paloc blob into one file per
// namespace, inside a per-language subdirectory:
//
//     1.05 - 2.00   gamedata/stringtable/binary__/localizationstring_<lang>.paloc
//     2.01+         gamedata/stringtable/binary__/<lang>/<namespace>.paloc
//
// The namespace is already encoded in every entry's `string_key`
// (`(key << 32) | group`), and the container is a flat entry list with a
// trailing count and no header — so the split is presentational only, and
// re-serialising the concatenated entry lists reproduces the pre-2.01 blob.

/// Directory holding the localization files (or, since 2.01, the
/// per-language subdirectories holding them).
const PALOC_ROOT: &str = "gamedata/stringtable/binary__";

/// Where one language's paloc file(s) live: the archive directory plus
/// every filename in it, sorted. One entry pre-2.01, 39 from 2.01 on.
/// `None` when the group's PAMT or the language is absent.
pub(crate) fn paloc_files(group: &str, lang: &str) -> Option<(String, Vec<String>)> {
    let pamt_bytes = std::fs::read(game_root().join(group).join("0.pamt")).ok()?;
    let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).ok()?;

    let lang_dir = format!("{PALOC_ROOT}/{lang}");
    if let Some(d) = pamt.directories.iter().find(|d| d.path == lang_dir) {
        // 2.01+: the directory itself is the language.
        let mut names: Vec<String> = d
            .files
            .iter()
            .filter(|f| f.name.ends_with(".paloc"))
            .map(|f| f.name.clone())
            .collect();
        names.sort();
        return (!names.is_empty()).then_some((d.path.clone(), names));
    }

    let d = pamt.directories.iter().find(|d| d.path == PALOC_ROOT)?;
    let name = format!("localizationstring_{lang}.paloc");
    let f = d.files.iter().find(|f| f.name == name)?;
    Some((d.path.clone(), vec![f.name.clone()]))
}

/// The extracted bytes of each paloc file for one language, as they sit on
/// disk — one blob pre-2.01, 39 from 2.01 on. Paired with the filename so a
/// failure can name it.
///
/// Use this over [`paloc_bytes`] when the test is about the on-disk bytes
/// (roundtrip), not about whole-language lookups.
pub(crate) fn paloc_blobs(group: &str, lang: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let (dir_path, names) = paloc_files(group, lang)?;
    let group_dir = game_root().join(group);
    let pamt_bytes = std::fs::read(group_dir.join("0.pamt")).ok()?;
    let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).ok()?;
    let dir = pamt.directories.iter().find(|d| d.path == dir_path)?;
    let enc = &pamt.header.encrypt_info.encrypt_info;

    names
        .into_iter()
        .map(|n| {
            let f = dir.files.iter().find(|f| f.name == n)?;
            let bytes = crate::binary::paz::extract_file(&group_dir, f, &dir_path, enc).ok()?;
            Some((n, bytes))
        })
        .collect()
}

/// Every paloc byte for one language in `group`, as a single blob.
///
/// Reads whichever layout the install ships and, on 2.01+, merges the
/// per-namespace files so callers keep seeing one whole-language PALOC.
/// The merged blob is a re-serialisation, so it is **not** the on-disk
/// bytes — see [`paloc_blobs`] for those.
pub(crate) fn paloc_bytes(group: &str, lang: &str) -> Option<Vec<u8>> {
    let blobs = paloc_blobs(group, lang)?;
    if let [(_, only)] = blobs.as_slice() {
        return Some(only.clone());
    }
    let mut entries = Vec::new();
    for (_, blob) in &blobs {
        entries.extend(crate::binary::paloc::LocalizationFile::parse(blob).ok()?.entries);
    }
    crate::binary::paloc::LocalizationFile { entries }.to_bytes().ok()
}
