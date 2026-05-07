# Building a previous-version parser as a sibling install

When a new game patch lands and the cross-version diff workflow is needed, you'll want both versions of the `crimson_rs` Python wheel loadable in the same Python process — the in-tree wheel (current parser) plus the previous-version wheel (last patch's parser, used as ground truth for tracked spans).

This recipe is what was used for the 1.04 → 1.05 RE.

## Recipe (one-time, per historical version)

Replace `<commit>` with the commit hash of the last-known-good parser for the previous version, and `<ver>` with a short identifier (e.g. `104`, `105`).

```powershell
# 1. Check out the historical parser in a sibling worktree.
git worktree add ../crimson-rs-<ver> <commit>

# 2. Build the wheel from that worktree.
cd ../crimson-rs-<ver>
maturin build --release

# 3. Install it into a target dir inside the main repo, NOT into site-packages.
cd ../crimson-rs
pip install --target=.crimson_rs_<ver> --force-reinstall --no-deps `
    ../crimson-rs-<ver>/target/wheels/crimson_rs-0.1.0-cp312-abi3-win_amd64.whl
```

For the 1.04 parser specifically, `<commit>` is `56a57da` and `<ver>` is `104`.

## How it's used

`.crimson_rs_<ver>/` is gitignored. Diagnostic scripts that need the historical parser do their own `sys.path.insert` so the in-tree wheel keeps working in the same process. Example from [`../scripts/archive/dump_104_spans.py`](../scripts/archive/dump_104_spans.py):

```python
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / ".crimson_rs_104"))
import crimson_rs as crimson_rs_104     # historical parser
sys.path.pop(0)
import crimson_rs as crimson_rs_105     # in-tree (current) parser
```

`crimson_rs_104.parse_iteminfo_tracked(...)` then parses the 1.04 binary with the 1.04 parser, returning spans you can use as ground truth.

## When to use this

Only when reverse-engineering a new structure-changing patch. For routine work the current in-tree parser is enough.

The cross-version diff scripts that consume the sibling install live in [`../scripts/archive/`](../scripts/archive/) — they're templates for the next-patch RE.
