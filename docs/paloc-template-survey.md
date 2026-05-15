# PALOC template-density survey

**Question**: do the shipped Mission/Quest/Stage/Knowledge/Character/
GimmickInfo display-name bridges need a template-resolver layer?

**Answer (2026-05-15)**: **no, not for the title namespaces those
bridges actually target**. Yes, if/when a downstream consumer surfaces
**description / dialogue / objective** text — those namespaces are
template-heavy and would need substitution.

The probe at `_probe_paloc_template_density` in
[`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
(`#[ignore]`'d) walks the 1.07 English PALOC (179,571 entries) and
counts template markers per namespace. Findings below.

---

## Template families PA uses

Catalogue is borrowed from CrimsonForge's reference tokenizer
(`D:\Github\crimsonforge\core\translation_tokenizer.py` lines 109–133),
which has the most mature treatment:

| Family | Example | Resolver needs |
|---|---|---|
| `{StaticInfo:Type:Key#fallback_label}` | `Defeat the {StaticInfo:Knowledge:Knowledge_LandSpider_BismuthQueen#Queen Bismuth Oreback Crab}` | Resolve `Type:Key` against the matching gamedata bridge; fall back to label after `#` if lookup misses |
| `{plain:tokens}` | `{enemy_count}` | Substitute from runtime context |
| `<br/>`, `<b>`, `<color>` | Multi-line / styled UI text | Strip or render |
| `[EMPTY]`, `[FULL]`, `[NONE]` | Untranslated / placeholder slots | Render as "(none)" / hide |
| `%0`, `%1`, `%s`, `%d`, `%%` | printf-style args | Substitute from caller |

---

## Per-namespace density (English PALOC, 1.07)

Counts and percentages for every namespace the shipped bridges target,
plus a few neighbours for context. "any_tpl" = entry contains at least
one of the families above. "Static" = entry contains
`{StaticInfo:` / `{Staticinfo:` specifically — the most resolver-
sensitive token family because it cross-references another gamedata
key.

| `lo32` | What it carries | Total | any_tpl | `{Static…}` | `%arg` | `<br>` | `[EMPTY]` |
|---|---|---:|---:|---:|---:|---:|---:|
| **— Shipped bridge targets (titles) —** | | | | | | | |
| `0x30` | CharacterKey display | 7,027 | **0%** | 0 | 0 | 0 | 0 |
| `0x70` | ItemKey title | 6,181 | **0%** | 0 | 0 | 0 | 0 |
| `0x100` | QuestKey arc heading | 673 | **0%** | 0 | 0 | 0 | 0 |
| `0x101` | StageKey title | 13,001 | **1%** | 129 | 0 | 0 | 0 |
| `0x102` | StageKey description (some) | 1,450 | **0.3%** | 0 | 0 | 4 | 0 |
| `0x200` | GimmickInfoKey display | 12,976 | **20%** | 0 | 0 | 0 | **2,604** |
| `0x490` | MissionKey / KnowledgeKey title | 6,049 | **0.1%** | 0 | 0 | 0 | 0 |
| `0x19202` | GimmickInfoKey long description | 191 | **25%** | 0 | 0 | 48 | 0 |
| **— Not yet bridged (descriptions / dialogue) —** | | | | | | | |
| `0x71` | ItemKey description | 6,181 | **6%** | 56 | 0 | 339 | 0 |
| `0x491` | MissionKey description | 6,049 | **19%** | 499 | 0 | 857 | 0 |
| `0x49e` | Knowledge body / lore | 4,460 | **18%** | 393 | 0 | 713 | 0 |
| `0x49f` | (Knowledge variant) | 6,049 | **19%** | 499 | 0 | 864 | 0 |
| `0xb0` | (Various — dialogue-ish) | 5,560 | **7%** | 58 | **184** | 61 | 0 |

The pattern is consistent:

- **Title namespaces (`0x30`, `0x70`, `0x100`, `0x490`)**: essentially
  flat (0–0.1% template-bearing). The shipped bridges return these
  directly with no substitution needed.
- **StageKey title `0x101` (1%)**: a small subset of stage titles
  cross-reference characters / other stages via `{StaticInfo:...}`.
  Negligible for a UI that shows the title verbatim — the fallback
  label after `#` is human-readable.
- **GimmickInfoKey display `0x200` (20% `[EMPTY]`)**: the 2,604
  `[EMPTY]` hits are sentinel values for unnamed / dev gimmicks.
  Not a resolver concern — they ARE the meaningful state ("no
  display name"). The C ABI surface treats them as ordinary strings.
- **Description namespaces (`0x71` / `0x491` / `0x49e` / `0x49f`)**:
  18–19% template-bearing, dominated by `<br/>` (paragraph breaks)
  and `{StaticInfo:...}` cross-references. **A resolver IS needed if
  these are exposed to the user.**
- **`%0`/`%1`/`%s`/`%d` printf-style args**: rare and concentrated in
  `0xb0` (184 entries). If/when the editor exposes that namespace,
  the resolver will need to know about caller-supplied substitutions
  too.

---

## So: do we need a resolver?

### Today: no

The eight shipped bridges (Mission/Quest/Stage/Knowledge/Character/
GimmickInfo/SubLevel/QuestGauge — names only) return flat strings
that an editor can display verbatim. `CrimsonAtomtic`'s current
mercenary-rename / character-name UI works fine without substitution.

The single non-trivial edge case is `StageKey title 0x101`: 129
entries (1%) contain `{StaticInfo:...}` cross-references. The
fallback label after `#` makes them human-readable as-is, but the
editor could special-case them — when the title contains `{`, render
the label-after-`#` only. Three lines of C# postprocessing, no
runtime resolver needed.

### Tomorrow: yes, if scope expands

Add a resolver when a downstream consumer surfaces ANY of:

- **Item descriptions** (`lo32 = 0x71`)
- **Mission descriptions** (`lo32 = 0x491`)
- **Knowledge body / lore** (`lo32 = 0x49e` / `0x49f`)
- **NPC dialogue / quest narrative text** (mostly `0xb0` and adjacent
  namespaces — not yet catalogued in detail)
- **Long gimmick descriptions** (`lo32 = 0x19202`)

The resolver would need to:

1. Tokenise on `{` … `}` (with `\#` inside marking the fallback label
   boundary).
2. For `{StaticInfo:Type:Key…}` tokens:
   - Parse `Type` (`Knowledge` / `Character` / `Mission` / etc.).
   - Look up `Key` against the matching shipped bridge (e.g.
     `crimson_knowledgeinfo_lookup_display_name`).
   - Substitute the resolved display string.
   - If lookup misses, fall back to the `#label` text.
3. Optionally strip `<br/>` / `<b>` / `<color>` tags (or convert to
   newlines / formatting for the UI's rich-text widget).
4. `[EMPTY]` / `[FULL]` / `[NONE]` sentinels stay literal — they're
   meaningful flags, not placeholders.

CrimsonForge's `core/translation_tokenizer.py` is a working Python
reference. A Rust C ABI port would mirror its regex+walk approach
plus call into the shipped `crimson_*_lookup_display_name` chains for
the `{StaticInfo:...}` cases.

---

## Recommended resume order

If the editor expands beyond titles, prioritise by user value:

1. **Item descriptions (`0x71`)** — high value (players want to know
   what an item does), low template complexity (only 6%, dominated
   by `<br/>` which is trivially handled).
2. **Knowledge body (`0x49e` / `0x49f`)** — moderate value (lore
   text), 18% template. The `{StaticInfo:Knowledge:...}` cross-refs
   benefit from the shipped `crimson_knowledgeinfo_*` chain.
3. **Mission descriptions (`0x491`)** — high value (objectives),
   moderate complexity. Same shape as Knowledge body.
4. **Dialogue (`0xb0`+)** — large surface, includes `%N` args. Needs
   the most resolver work; defer until 1–3 are in.

For each, the build-out is roughly the same: pipe the existing PALOC
chain through a new `crimson_paloc_resolve_template` function that
substitutes tokens before returning the string to the caller.
