# Save handle mutation version + snapshot reads

A short note on the cache-coherency contract every C ABI consumer of
`CrimsonSaveHandle` should follow when it holds a snapshot of save
state across mutations.

## The problem

All "read" entry points on the save handle —
[`crimson_save_get_block_json`](../src/c_abi/mod.rs),
[`crimson_save_list_inventory_items`](../src/c_abi/mod.rs), and
friends — return **positional snapshots**. A returned `(block_idx,
inventory_element_idx, item_element_idx)` tuple is valid against the
handle's state **at the moment the read happened**. Any
length-changing mutation between the read and a subsequent use can
invalidate the positions:

| C ABI mutation | What it invalidates |
|---|---|
| [`crimson_save_set_scalar_field`](../src/c_abi/mod.rs) / `_path` | scalar value at one slot — neighbour positions unchanged, but cached `count` / `item_key` values for that exact slot are stale |
| [`crimson_save_list_remove_element`](../src/c_abi/mod.rs) / `_remove_elements_batch` | every element index in the same list ≥ the removed index shifts down |
| [`crimson_save_list_insert_element`](../src/c_abi/mod.rs) / `_list_clone_element` | every element index in the same list ≥ the inserted index shifts up |
| [`crimson_save_set_scalar_field_present`](../src/c_abi/mod.rs) / `_batch` | the field's `present` bit + `kind` for one slot — neighbour positions unchanged |
| [`crimson_save_set_inline_bytes_field`](../src/c_abi/mod.rs) | inline-bytes payload of one slot — neighbour positions unchanged |
| [`crimson_save_dynamic_array_set_u32_elements`](../src/c_abi/mod.rs) | array contents for one slot — neighbour positions unchanged |

## The protocol

Every successful mutation through any of those entry points bumps
the save handle's internal `mutation_version: u64` counter by exactly
1. Pure read entry points do not bump it. Failed mutations do not
bump it (the handle is rolled back to its pre-call state, including
the version).

Snapshot readers stamp the version at read time and compare against
the live version before each reuse. This is O(1) regardless of save
size.

### Reading the version

```c
uint64_t v = 0;
if (crimson_save_get_mutation_version(handle, &v) != CRIMSON_OK) {
    // null pointer
}
```

`crimson_save_get_mutation_version` is a single pointer-deref + u64
read. Free to call.

### Pairing a snapshot with a version

The snapshot APIs that take an `out_version` parameter populate it
with the value of `mutation_version` AT READ TIME, atomically with
the snapshot itself. Hold the `(snapshot, version)` pair together
and refresh when they disagree with the live version.

[`crimson_save_list_inventory_items`](../src/c_abi/mod.rs):

```c
// First call: query size + grab the version stamp.
size_t count = 0;
uint64_t snapshot_version = 0;
crimson_save_list_inventory_items(
    handle, NULL, 0, &count, &snapshot_version);

// Allocate, second call: fills records and re-confirms the version.
CrimsonInventoryItemRecord* records =
    malloc(count * sizeof(*records));
crimson_save_list_inventory_items(
    handle, records, count, &count, &snapshot_version);

// ... time passes, possibly with crimson_save_set_*/list_* calls ...

uint64_t live;
crimson_save_get_mutation_version(handle, &live);
if (live != snapshot_version) {
    // Snapshot is stale. Free + re-list.
    free(records);
    // … repeat the two-call dance …
}
```

### C# idiomatic wrapper

```csharp
public sealed class InventorySnapshot : IDisposable
{
    private readonly NativeSaveHandle _handle;
    private CrimsonInventoryItemRecord[] _records;
    private ulong _version;

    public IReadOnlyList<CrimsonInventoryItemRecord> Records
    {
        get
        {
            if (Native.GetMutationVersion(_handle) != _version)
                Refresh();
            return _records;
        }
    }

    private void Refresh()
    {
        // Two-call dance, stamp _version from out parameter
    }
}
```

This pattern is **the** correct way to consume a snapshot. Hardcoding
"invalidate the cache on every save mutation we know about" is
fragile — easy to miss an FFI path. Version check is the
ground-truth: if the version is unchanged, the snapshot IS still
correct; if it bumped, the snapshot MAY be stale and a refresh is
the safe move.

## What the version does NOT do

- It is **not** a transaction id. There is no rollback API that
  reverts to a prior version.
- It is **not** persistent. A fresh handle from
  [`crimson_save_load_from_file`](../src/c_abi/mod.rs) starts at 0
  regardless of the on-disk save's history.
- It is **not** thread-safe. The C ABI surface is single-threaded by
  contract — `CrimsonSaveHandle` is not `Send` / `Sync`.
- It does **not** auto-refresh snapshots. The caller is responsible
  for re-issuing the read when the version mismatches.

## Adding new mutation entry points

If you add a new `crimson_save_*` C ABI function that mutates the
handle:

1. Call `h.bump_version()` exactly once after the mutation is
   committed (after `h.blocks = h.body.decode_blocks(...)` or the
   `apply_length_changing_mutation` helper, both of which already
   bump on the success path).
2. Do NOT bump on the failure path. The handle must be rolled back
   to pre-call state, including the version.
3. Add a regression test that the new mutation function bumps the
   version exactly once, mirroring
   `c_abi_mutation_version_bumps_on_mutation_only` in
   `src/c_abi/mod.rs`.

The `apply_length_changing_mutation` helper already bumps internally
on the success path — any new function using it inherits the bump
automatically.

## Interaction with the deferred-redecode batch ABI

[`save-deferred-redecode.md`](./save-deferred-redecode.md) introduces a
transactional batch (`crimson_save_begin_deferred_redecode` →
`_end_*` / `_abort_*`) that suspends the per-call `decode_blocks` on
every mutation entry point. From the mutation-version reader's
perspective:

- **Mid-batch mutations DO NOT bump `mutation_version`**. The version
  reflects "the on-disk-equivalent state changed"; while a batch is
  open the disk-equivalent state is still the pre-begin image. The
  in-memory tree is ahead of disk, but no observer outside the
  current FFI call has seen it.
- **A successful `end_*` bumps `mutation_version` exactly once** —
  regardless of how many mutations ran inside. Snapshot readers
  invalidate against this single bump, not against each in-batch
  call.
- **An aborted batch (`abort_*`) does not bump** — observationally
  the state hasn't changed since `begin_*`. Snapshot readers see
  the same version they did before the batch opened.
- **A failed `end_*` (`MUTATION_INVALID`)** does not bump either —
  the handle is rolled back to its pre-begin state, equivalent to
  an abort.

The snapshot-reader pattern from this doc keeps working unchanged
under deferred batches. Each commit produces one bump; snapshots
re-walk once per commit, not once per mutation.
