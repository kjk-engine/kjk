# Bumps: numbered at merge

A branch never edits a format's version number (KJKS, KJKV, KJKR, …) or
the sim hash's pin (`KJK_SIM_HASH` in `ci.yml` and `manual.yml`). Two
branches doing so took the same numbers and fought over the same lines:
111 re-pins, about a tenth of all commits, and 17 snapshot versions taken
twice. Instead a branch says what it changes in a file of its own here,
and the merge numbers it.

## On a branch

Add `bumps/<branch-name>.toml` (any name ending in `.toml`; one per
branch, so no two branches touch the same file):

```toml
# The sim's hash moves on purpose (beyond a snapshot bump, which moves it
# anyway): why.
sim = "blasters fire KOTOR's bolt models"

# Each format whose encoding changes, with the line its version's history
# gets (the comment above its `VERSION`), said as the others are.
[formats]
snapshot = "a fighter carries its blaster's bolt model (`Fighter::bolt`)."
save = "a queued shot carries its bolt (`Action::Shoot::bolt`)."
```

The formats: `snapshot`, `save`, `replay`, `pack`, `gathered`, `install`
(`crates/bumps/src/lib.rs`). Change a file's text as the branch goes on;
remove the file if the branch no longer changes anything.

While the file is there:

- Each format's version is one above `main`'s (the sim's `build.rs` adds
  the bumps pending), so the branch refuses `main`'s saves and snapshots
  and `main` the branch's, as a bump always did.
- `sim_determinism` still checks every way agrees, but leaves the final
  hash to the merge when the file has `sim` or bumps `snapshot`. Without
  a bump file the hash must stay `main`'s: an unexpected move fails, as
  before.

## Reading old saves

Players keep their saves across updates, so a bump of a format saves hold
(`save`, `snapshot`, `pack`) says how a save from before it is read, under
`[migrations]`, or the file says why such saves are refused:

```toml
[formats]
snapshot = "a swing's contact keeps whether its chance to be turned was drawn (`Contact::drawn`)."

[migrations]
snapshot = "old saves read `drawn = false`: no swing in a save is mid-contact"
```

```toml
breaking = "why old saves can't be read, and what the story restore keeps"

[formats]
snapshot = "…"
```

The build refuses a stored format bumped with neither, and a migration of a
format not bumped or not stored. `breaking` takes the owner's approval in
the pull request. While it is pending, that format's `OLDEST` is already
its version, so the branch refuses older saves as the merge will.

The decoder keeps the old read beside the new one, gated on the version
the bump gives, named after its file (`bumps/saber-duel-fixes.toml` is
`SABER_DUEL_FIXES`; `crates/sim/src/since.rs`). The name must start with a
letter and be new: one a pinned bump took, or another pending file has, is
refused. The gate reads that format's version wherever the bytes lie in a
save (a sheet in the snapshot, an item's facts in the scripts' state):

```rust
let drawn = if input.since(crate::since::snapshot::SABER_DUEL_FIXES) {
    input.bool()?
} else {
    false
};
```

- A field added: read from new versions; old ones get what an old save
  means (empty, `None`, the default). Most bumps are this.
- A field removed: old versions read it and drop it.
- A field whose meaning changed: read the old form, then convert it in
  `crates/sim/src/migrate.rs`, after the whole save is decoded.

`encode` writes only the current layout. The approved test
`old_saves_load` loads the fixture of every earlier version
(`crates/approved_tests/saves`), so a pull request whose old saves no
longer read fails there.

Report the change in the pull request as before ("KJKS +1"); its number
is the merge's.

## At merge

`.github/workflows/pin.yml` runs on `main` after a pull request with a bump
file merges (a merge or push to `main` starts it by its paths). It runs `cargo xtask pin`: each bump, in the files' name order
(as the build numbers them on a branch), takes the next version with its history
line; the files are removed; the pinned determinism run's hash is written
into both workflows. Then the approved tests run on the result, and it is
committed to `main` as "Pinned at merge: …", naming every version and
the hash old → new. `git log --grep "Pinned at merge"` is the history.

`cargo xtask pin` runs locally too (it edits the tree, commits nothing),
e.g. to see the numbers a merge would give.
