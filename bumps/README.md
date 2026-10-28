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

Report the change in the pull request as before ("KJKS +1"); its number
is the merge's.

## At merge

`.github/workflows/pin.yml` runs on `main` after a pull request with a bump
file merges (a merge or push to `main` starts it by its paths). It runs `cargo xtask pin`: each bump, in the
order the files reached `main`, takes the next version with its history
line; the files are removed; the pinned determinism run's hash is written
into both workflows. Then the approved tests run on the result, and it is
committed to `main` as "Pinned at merge: …", naming every version and
the hash old → new. `git log --grep "Pinned at merge"` is the history.

`cargo xtask pin` runs locally too (it edits the tree, commits nothing),
e.g. to see the numbers a merge would give.
