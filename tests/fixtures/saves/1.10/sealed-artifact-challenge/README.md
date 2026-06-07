# Sealed-abyss-artifact challenge — golden completion + crash repro (1.10)

git-crypt encrypted (real player save data). See `../../README.md` for the
unlock recipe. Full analysis: [`docs/save-loader-length-change-crash.md`](../../../../../docs/save-loader-length-change-crash.md).

Two paired cases, both on Crimson Desert 1.10:

## `engine-natural/` — GOLDEN reference (what a correct completion looks like)

A sealed-abyss-artifact challenge completed **in-game by the engine**, captured
just before vs. just after completion (reward NOT yet claimed).

| Dir | State | Loads? |
|---|---|---|
| `before/` | challenge at 19/20 (in progress) | ✅ |
| `after/`  | challenge engine-completed, reward unclaimed | ✅ |

Challenge: `Challenge_SealedArtifact_Crime_VIII` (catalog key `1001039`),
follow-up `Challenge_SealedArtifact_Crime_VIII_2` (`1003411`).

The `before → after` diff is the ground truth the editor's "complete challenge"
logic is validated against — the engine's natural completion is **exactly**:
- FAR tracker (negative-keyed): `_state` 2 → **5**, `_completedTime` added;
- a NEW `_2` sub-mission appended at `_state=2` (`_uiState=1`, `_branchedTime`,
  `_newAlarm`, no `_completedTime`) — a clone of the fresh FAR-tracker shape;
- the catalog row is **left untouched** (the engine writes it only when the
  player claims the reward).

`_missionStateList` grows by exactly one element; everything else is
session/world housekeeping (the play time between the two saves).

## `editor-edit/` — crash repro (game-side loader bug)

The same player save before vs. after the editor's "Complete Eligible Held
Sealed Abyss Artifact Challenges" (one challenge:
`Challenge_SealedArtifact_Mastery_Spear_V`, catalog `1000692`, X_2 `1003304`).

| Dir | What | Loads in game? |
|---|---|---|
| `before/` | pristine player save | ✅ |
| `after/`  | editor output — one challenge completed (body +64 bytes) | ❌ **crashes** |

`after/` is **byte-correct and round-trips cleanly** through crimson-rs
(verified: encoder fidelity incl. the bool fix, all object-locators / TOC /
HMAC / LZ4 consistent), and its mission edit matches `engine-natural/after`'s
shape exactly. It still crashes the game's loader because the +64-byte growth
shifts the body and trips a **latent fixed-buffer `memcpy` overflow inside the
game's own save loader** (not a data error) — see the doc above. In-place
(non-length-changing) edits do not trigger it.
