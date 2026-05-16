# Deferred-redecode batch — suspending the per-call re-decode

> Status (2026-05-16): shipped. Lifecycle entry points
> `crimson_save_begin_deferred_redecode` /
> `crimson_save_end_deferred_redecode` /
> `crimson_save_abort_deferred_redecode` /
> `crimson_save_is_deferred_redecode_open`. Four `#[cfg(test)]`
> roundtrip + abort tests live in
> [`src/c_abi/mod.rs`](../src/c_abi/mod.rs).

## The problem this solves

Every length-changing mutation on a save handle pays an
encode + parse + decode_blocks cycle that runs roughly **25 ms** on
the 1.07 baseline save. Every scalar mutation pays a
decode_blocks-only cycle (no encode + parse) — same ~25 ms hot loop.

Workflows that fire many length-changing edits in succession quickly
accumulate. The motivating example from CrimsonAtomtic's
**"Complete All held sealed abyss artifact challenges"**:

| Per-challenge step | Shape | Time (normal mode) |
|---|---|---|
| `list_clone_element` | length-change → encode + parse + decode | ~25 ms |
| `set_scalar_field` (state) | decode only | ~25 ms |
| `set_scalar_field_present` (`_completedTime`) | length-change → full cycle | ~25 ms |
| `dynamic_array_set_u32_elements` (tags) | length-change → full cycle | ~25 ms |
| `set_scalar_field` (clone `_key`) | decode only | ~25 ms |
| `set_scalar_field` (clone `_branchedTime`) | decode only | ~25 ms |

141 challenges × 3 length-changing decodes ≈ **10 seconds** of pure
`decode_blocks` time. (The scalar-only decodes amortise away via
`crimson_save_set_scalar_fields_batch`, but the three length-changing
calls each force their own decode and can't be folded into the
scalar batch.)

## The fix — a deferred-redecode batch

A "transactional" wrapper that suspends the encode + decode tail on
every mutation entry point. The matching `end_*` call runs **one**
encode + parse + decode_blocks pass for the whole batch.

```c
int32_t crimson_save_begin_deferred_redecode(handle);
int32_t crimson_save_end_deferred_redecode(handle);      // commits + bumps version once
int32_t crimson_save_abort_deferred_redecode(handle);    // rolls back
int32_t crimson_save_is_deferred_redecode_open(handle, *out_open);
```

While a batch is open:

- **Length-changing entry points** (`list_clone_element`,
  `list_insert_element`, `list_remove_element` + batch variants,
  `set_scalar_field_present` + batch variant,
  `set_inline_bytes_field`, `dynamic_array_set_u32_elements`):
  mutate `blocks` in place, skip the encode + parse + decode_blocks
  tail.
- **Scalar entry points** (`set_scalar_field`,
  `set_scalar_field_path`, `set_scalar_fields_batch`): update the
  in-memory `ScalarValue` via [`scalar_from_bytes`](../src/save/body/object.rs);
  no body patch, no decode_blocks. The encoder at `end_*` time reads
  the typed value back, so the change persists.
- **Read entry points** (`get_block_json`, `list_inventory_items`,
  `get_block_info`, …): all work as normal — `blocks` is always the
  in-progress tree, so reads see the latest state.
- **`write_to_file`**: rejected with `BATCH_IN_PROGRESS`. The cached
  `save.body` is still at its pre-batch byte image; writing would
  silently drop every mutation. Caller must end / abort first.
- **`mutation_version`**: bumps exactly once on a successful `end_*`
  (regardless of how many mutations ran inside). `abort_*` restores
  the pre-begin value. Snapshot readers (see
  [`save-mutation-version.md`](./save-mutation-version.md)) only
  observe the post-batch state, never an intermediate.

## Error codes

| Code | When |
|---|---|
| `BATCH_IN_PROGRESS` (-21) | `begin_*` called while a batch is already open; `write_to_file` called inside an open batch |
| `BATCH_NOT_OPEN` (-22) | `end_*` / `abort_*` called with no batch open |
| `MUTATION_INVALID` (-19) | `end_*` couldn't encode or re-parse the accumulated tree; the handle is rolled back to its pre-batch state |

`begin_*` does **not** nest. Open at most one batch at a time; end or
abort the outer batch before starting a new one.

## Failure semantics

- **Per-op failure inside the batch** (a mutation entry point returns
  e.g. `OUT_OF_RANGE` or `NOT_SCALAR`): the tree is left partially
  mutated. The caller decides whether to keep going or call `abort_*`.
  This mirrors how the non-deferred `*_batch` entry points behave —
  they too leave earlier ops applied on a mid-batch failure.
- **Commit failure** (`end_*` returns `MUTATION_INVALID`): the
  accumulated tree did not round-trip through `Body::write` +
  `Body::parse`. The handle's `blocks` is restored from the
  snapshot captured by `begin_*`; `save.body` and `body` were
  untouched throughout the batch, so the handle is left in its
  exact pre-begin state. `mutation_version` is not bumped.
- **Caller drops the handle mid-batch**: harmless. The
  `DeferredState` snapshot is freed along with the handle. No leak,
  no corruption — the on-disk file is never written without an
  explicit `write_to_file` call.

## The wall-clock win

Same "Complete All held sealed abyss artifact challenges" workflow:

| Phase | Normal mode | Deferred batch |
|---|---|---|
| 141 × `list_clone_element` | 141 × encode + parse + decode | 141 × in-place mutate |
| 141 × `set_scalar_field` state | 141 × decode_blocks | 141 × in-place mutate |
| 141 × `set_scalar_field_present` | 141 × encode + parse + decode | 141 × in-place mutate |
| 141 × `dynamic_array_set_u32_elements` | 141 × encode + parse + decode | 141 × in-place mutate |
| 282 × `set_scalar_field` clone fields | (folded into scalar `*_batch`) | 282 × in-place mutate |
| Final commit | — | **1** × encode + parse + decode |
| **Total `decode_blocks` calls** | **~423** | **1** |
| **Wall-time** | ~10 s | ~0.1 s |

The cost is the cost of one `decode_blocks` plus the per-op tree-walk
work (cheap). For the abyss-artifact workflow this brings the loop
under a single frame.

## C# usage pattern

```csharp
// One-shot transactional helper around begin/end/abort.
public static void RunDeferred(NativeSaveHandle h, Action body)
{
    int rc = Native.crimson_save_begin_deferred_redecode(h);
    if (rc != Native.OK) throw new CrimsonRsException(rc, "begin_deferred");

    try
    {
        body();
    }
    catch
    {
        Native.crimson_save_abort_deferred_redecode(h);
        throw;
    }

    int end_rc = Native.crimson_save_end_deferred_redecode(h);
    if (end_rc != Native.OK)
    {
        // end already rolled `blocks` back on MUTATION_INVALID;
        // surface the error to the caller.
        throw new CrimsonRsException(end_rc, "end_deferred");
    }
}

// Call site — "Complete All held sealed abyss artifact challenges":
RunDeferred(_save, () =>
{
    foreach (var challenge in heldChallenges)
    {
        CloneChallengeRow(challenge);          // list_clone_element
        SetCloneState(challenge, Completed);   // set_scalar_field
        TouchCompletedTime(challenge);         // set_scalar_field_present
        WriteCloneTags(challenge);             // dynamic_array_set_u32_elements
        SetCloneKey(challenge);                // set_scalar_field
        SetCloneBranchedTime(challenge);       // set_scalar_field
    }
});
// One bump on `mutation_version`, one decode_blocks pass total.
```

After the batch, the snapshot reader pattern from
[`save-mutation-version.md`](./save-mutation-version.md) just works
— there's exactly one version bump to invalidate against.

## Round-trip contract

The committed body bytes from a deferred batch are byte-identical to
running the same mutations in normal mode. This is pinned by
`c_abi_deferred_redecode_commits_and_matches_normal_mode` (scalar-only
sequence) and `c_abi_deferred_redecode_mixed_length_change_matches_normal_mode`
(`list_clone_element` + scalar) in
[`src/c_abi/mod.rs`](../src/c_abi/mod.rs). If either test ever
diverges, the deferred path is mishandling some preservation field
the normal path captures.

## When NOT to use a deferred batch

- **Single-mutation workflows**: the per-op `decode_blocks` is
  already the canonical state-refresh; opening a batch just adds
  begin / end overhead.
- **Workflows that need to write the save mid-stream**:
  `write_to_file` is rejected during a batch. End first.
- **Heterogeneous handles**: a deferred batch on one handle does not
  affect another. Each handle has its own `Option<DeferredState>`.

## Cross-references

- [`save-mutation-version.md`](./save-mutation-version.md) — the
  staleness contract that snapshot readers should pair with this
  ABI.
- [`save-editor-keys-plan.md`](./save-editor-keys-plan.md) — the
  Save Editor key-resolver workstream; the C# editor's bulk
  challenge-completion flow is the canonical consumer.
- [`socket-editor-scope.md`](./socket-editor-scope.md),
  [`dye-editor-scope.md`](./dye-editor-scope.md) — sibling
  editor docs; both expose multi-mutation workflows that benefit
  from the deferred batch when run in bulk.
