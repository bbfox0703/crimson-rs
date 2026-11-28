# tools/

## git-crypt.exe

| | |
|---|---|
| Version | `git-crypt 0.8.0` (its `--version` output) |
| SHA256 | `f797fe01183fc06324e66c267aa94e00b003c769b1ee279904b7bff281ae6379` |
| Size | 456,192 bytes |
| Added | 2026-06-05 (`a351b27`) |
| Upstream | git-crypt by Andrew Ayer, <https://github.com/AGWA/git-crypt>, GPL-3.0; source code at that repository |

**Why it is here.** Until 2026-11-27 the reference saves sat in this repo under
`tests/fixtures/saves/`, encrypted with git-crypt. They now live in the private
`crimson-rs-private` repo (see `src/test_fixtures.rs`), but the commits before that
still hold the encrypted blobs, and this binary decrypts them on Windows when you check
out such a commit with the key.

**Not established:** that this file is byte-identical to an official upstream release
asset; compare the SHA256 above with the release you trust before using it.
