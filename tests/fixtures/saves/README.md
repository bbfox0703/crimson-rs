# Reference save files (git-crypt encrypted)

Real Crimson Desert save slots, kept as parser/round-trip reference data:

| Dir | Game version | `ContentsMiscSaveData` | Notes |
|---|---|---|---|
| `1.09/` | 1.09 | 14 fields (no `_miniGameLeaderboardSaveDataList`) | pre-reconstruction-phase schema |
| `1.10/` | 1.10 | 15 fields | the object-list leading-pad widened 3→4 bytes (see `src/save/body/decoder.rs`) |
| `1.10/sealed-artifact-challenge/` | 1.10 | — | before/after pairs (engine-natural vs editor-edit) for the length-change loader crash |
| `1.10/broken_save_after_length_change/` | 1.10 | — | "add sugar" A/B/C set: `base_sample` (pristine), `add_2_sugar_in_game` (game-written, loads), `use_editor_add_sugar` (editor-written, CTDs). Controlled pair that pins the editor's item-fidelity drift — see [`docs/save-loader-length-change-crash.md`](../../../docs/save-loader-length-change-crash.md) |

Each dir holds the full slot: `save.save` (main body — ChaCha20 + HMAC + LZ4)
and `lobby.save` (lobby/character-select metadata).

## Encryption

The `*.save` files are **transparently encrypted with [git-crypt](https://github.com/AGWA/git-crypt)**
(see the rule in the repo-root `.gitattributes`). They contain a real player's
save data, so only the encrypted blobs are pushed to the public remote — the
working-tree copies are decrypted on checkout and re-encrypted on commit,
automatically, once the repo is unlocked.

**The key is NOT in the repo.** It lives in `.git/git-crypt/` (which git never
commits) and is exported to a file kept outside the working tree.

### Unlocking a fresh clone (to get transparent read/write access)

1. Get the exported key file (kept off-repo by the maintainer; default export
   location on the original machine: `%USERPROFILE%\.git-crypt\crimson-rs.key`).
2. From the repo root, with `git-crypt` available:

   ```powershell
   # Windows, using the vendored binary:
   tools\git-crypt.exe unlock C:\path\to\crimson-rs.key
   ```

   This installs the smudge/clean filters into `.git/config` and decrypts every
   `*.save` in the working tree. From then on the files are transparent — read
   and edit them like normal; `git add`/`commit` re-encrypts automatically.

### Without the key

A clone without the key (or without git-crypt configured — e.g. CI) checks the
`*.save` files out as their **encrypted blobs**. Git treats the `git-crypt`
filter as a pass-through when it isn't configured, so nothing breaks; the files
just aren't readable as saves. CI doesn't use them.

### Verifying state

```powershell
tools\git-crypt.exe status          # lists which files are encrypted
tools\git-crypt.exe status -e       # only the encrypted ones
```
