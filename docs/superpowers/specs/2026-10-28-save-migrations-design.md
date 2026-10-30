# Save migrations — design

Date: 2026-10-28. Status: draft for the owner's approval.

Builds on `2026-10-28-safety-net-recorder-design.md` (the safety net): its
backups on a new package version, rotating autosaves, the `meta.json` fields
`sha256`, `build`, `version`, `game` and `formats`, and its refusal text
naming the save's build. This design uses those as they are, and adds one
`meta.json` field (`story`, section 4), to be agreed with that design's
author.

## Why

kjk is public, and players will keep their saves across updates. Today a save
loads only on the exact format versions and the exact level digests it was made
with. Every format bump or data change breaks every save, and only the owner's
saves get fixed, by hand, through the save-compat skill
(`.claude/skills/save-compat/`). A bump has happened about every 15-20
commits so far (KJKV is at 59, KJKS at 72). Players can't run that skill.

The goal is that **a player's save from any release since v0.1.0 loads on any
later release**, and that when it truly can't, they keep their story progress
instead of starting over.

## Goals

- Every format bump that touches what a save holds ships its migration in the
  same PR. CI refuses a bump that doesn't say how old saves are read.
- Level digest changes (module data, collision, nav, anims, scripts) never
  refuse a save by themselves. The save is re-stamped at load with a warning
  in the log, including the modules left behind.
- Real saves from every release (Endar Spire, Taris, mid-Korriban, TSL
  Peragus) load on main, checked before every release.
- When a save truly can't be read or matched, the game offers to restore the
  story from a small, stable record written next to it.
- Later: a snapshot layout where most bumps need no migration code at all.

## Non-goals

- Old *builds* reading new saves (downgrades). A save written by 0.5 need not
  load on 0.4.
- Replays, flight recordings and bug reports on another build. They replay
  only on their own build (safety net, "Replay builds").
- The network formats. Host and joiners run the same build (see section 1d).
- Saves from before v0.1.0 (none exist outside the owner's machine, and those
  are at the v0.1.0 versions already).
- Importing the original games' own `.sav` files.

## Anti-goals (a failure even if it works)

- **A save that loads but plays wrong without telling anyone.** Every
  migration, re-stamp and fallback says what it did in the log, and a fallback
  also tells the player in the game.
- **A migration that writes over the player's save.** Loading never rewrites
  the slot. The next save the player makes is in the new format; the safety
  net's backup holds the old bytes.
- **Per-module or per-save patches.** Migrations are written per format
  change, engine-wide, never "fix Taris saves".
- **A slower load for saves that need nothing.** A current-format save with
  matching digests takes exactly today's path.
- **Copyrighted game data in the public repo.** Real saves carry bytes derived
  from the games' files, so they are not committed there (section 2).
- A sim hash change from parts 1, 2 and 4. Part 3 adds snapshot fields, and
  any snapshot bump moves the hash (it is `blake3(encode_state())`,
  `snapshot.rs:1202`), so part 3 lands as a normal hash-moving PR with a `sim`
  line in its bump file. Part 5 moves it once more.

## What exists today (survey, 2026-10-28)

- **KJKV save** (`crates/sim/src/save.rs`): `VERSION = 59 + pending::SAVE`
  (:108). `Save { hz, level: LevelId, snapshot, script }`. The snapshot and the
  script state are two length-prefixed sections. The only back-compat read is
  an empty script section becoming `State::default()`.
- **Version check**: `Reader::header` (`crates/sim/src/wire.rs:158-174`)
  refuses anything but the exact version (`DecodeError::Version`). There is no
  range and no hook.
- **KJKS snapshot** (`crates/sim/src/snapshot.rs`): `VERSION = 72 +
  pending::SNAPSHOT` (:176). One flat positional stream: a head, the entities,
  a tail of about 20 systems (talk, goods, progress, pazaak, fog, …), then the
  stash. There are no tags and no lengths per system. `split`/`Parts` cuts
  head/entities/tail for network diffs.
- **`script::State`** (`crates/sim/src/script/state.rs`) is positional too,
  and holds both the story (globals, party, items, journal, XP, careers,
  reputation, galaxy) and transient VM state keyed by `SimId` (pending,
  delayed, actions, effects…). The transient half is why most combat or action
  changes bump KJKV.
- **KJKT pack** (`pack.rs`, `VERSION = 20 + pending::PACK`): creature and item
  templates. Party `Npc::template_data` embeds KJKT bytes, so a pack bump
  reaches saves.
- **KJKA install** (`install.rs:930`), **KJKR replay** (`replay.rs:46`) and
  **KJKL gathered** (`pack.rs:392`): not stored in saves.
- **Level digest**: `Level::digest` (`world/level.rs:357-389`) is one BLAKE3
  over module data, collision, anims, nav, friendly fire, scripts, minigame and
  arena digests. Only the combined hash is stored.
- **Two digest refusals**:
  - at load, `Save::world` → `LevelId::check` (`save.rs:136`, `level.rs:67`):
    "the game's data is not what the save was made with";
  - lazily, on walking back into a module left behind:
    `InstallError::StashChanged` (`install.rs:284-295`) compares the stashed
    `Place` (game, module, digest, players) with the new level's. A stale save
    loads fine and fails later in play.
- **Bumps** (`bumps/README.md`, `crates/bumps`, `crates/sim/build.rs`,
  `xtask/src/pin.rs`, `pin.yml`): a branch adds `bumps/<branch>.toml` with
  `sim = "…"` and `[formats] snapshot = "history line"`. The build counts them
  into `pending::*`, and `pin.yml` numbers them at merge, writes the history
  lines and deletes the files.
- **Fixtures**: no `.kjkv` is committed anywhere. `kjk chain` checkpoints are
  KJKV saves in `kjk-artifacts/checkpoints/<game>/` (not in git); a format bump
  invalidates all of them.
- **CI** runs on `ubuntu-latest` without the games' files, so it can decode a
  save but can't plan its level.
- **Load path**: `session_flow.rs:260` `prepare()` → `saves::read` →
  `Save::decode` → `play::load_for` (`play.rs:416`) plans the module →
  `play.rs:775` `save.world(level, seats, tick)`. A reload after a party wipe
  goes through `sim_host.rs:1096`/`:1240`.

## The load ladder

How a load runs once this design is in. Each rung is a part below.

1. The safety net's `sha256` check (a damaged save stops here).
2. **Decode with migrations** (part 1): any version from v0.1.0's up to the
   current one.
3. **Reconcile stamps** (part 3): re-stamp digests, check what can't be
   matched.
4. If 2 or 3 truly fails: **offer the story restore** (part 4).
5. If there is no story record either: the safety net's refusal text.

Rungs 2 and 3 are silent to the player, logged with a `save-migrate` target.
Rung 4 asks first.

## 1. In-tree migrations

### 1a. Reading a range of versions

- `Reader::header` stays as it is for formats that stay exact. A new
  `Reader::header_since(magic, oldest, current) -> Result<u32>` returns the
  version found. It refuses anything older than `oldest` or newer than
  `current`.
- `Save::decode`, `Snapshot::decode` and the KJKT reader use it. The version
  found is passed down to every `read`/`decode` in that format (a `v: u32`
  argument, or a field on the reader; the builder picks).
- The **oldest supported version** of each format is a constant next to
  `VERSION`: `OLDEST = 59` for KJKV, `72` for KJKS, `20` for KJKT (the v0.1.0
  values).

### 1b. How a change reads the old layout

Every change to an encoding keeps its old read beside the new one, gated on
the version that introduced it:

```rust
// snapshot.rs, decode of the tail
let drawn = if v >= since::snapshot::SABER_DUEL_FIXES { input.bool()? } else { false };
```

- **A field added**: read it from new versions, and give it what an old save
  means (empty, `None`, default) in old ones. This is most bumps.
- **A field removed**: old versions read it and drop it.
- **A field whose meaning changed**: read the old form, then convert it in
  `migrate::<format>::fixup(v, &mut value)`, one small function per such
  change, in `crates/sim/src/migrate.rs`. It runs after the decode, with the
  whole save in hand, so a change that needs another system's state can see
  it.
- `encode` only ever writes the current layout.

This is what the save-compat skill already does by hand in a throwaway
worktree ("patch only the decoders… filling new fields with what an old save
means"). The difference is that the patch stays in the tree.

### 1c. Naming versions before they have numbers

Numbers are given at merge by `xtask pin`, so a branch can't write
`v >= 73`. Instead each bump file gets a **named version**:

- `crates/sim/build.rs` already turns bump files into `pending::*` counts. It
  also writes, per bump file and format, a constant named after the file:
  `since::snapshot::SABER_DUEL_FIXES = <main's version> + <its place among the
  pending files>`.
- `xtask pin` moves those constants into a committed
  `crates/sim/src/since.rs` with their final numbers, as it already writes the
  history lines. Decoders don't change at pin time; only the constants' home
  does.
- Two branches bumping the same format both read correctly after either
  merges, since each names its own constant.

### 1d. Which formats need migrations

| Format | Stored in a save | Needs a migration |
|---|---|---|
| `save` (KJKV) | yes | yes |
| `snapshot` (KJKS) | yes | yes |
| `pack` (KJKT) | yes, in party `template_data` | yes |
| `install` (KJKA) | no, network only | no |
| `replay` (KJKR) | no | no (replays use their own build) |
| `gathered` (KJKL) | no, a cache | no |

**Pack bytes in saves carry no header.** Party `Npc::template_data` is bare
creature bytes (`pack::creature_of`, `pack.rs:228`), and a pack bump doesn't
move KJKV, so a save can't tell which pack layout its templates use. *My
call:* the save records its pack version, a new KJKV field added in part 1's
own bump (old saves read it as 20, the v0.1.0 value), and the creature reader
takes that version like the others. `creature_of` stops turning a decode
error into `None`; a template that won't decode is an error in the load, not
a silently missing companion.

*My call:* `install` needs no migration. It is the announcement a host sends
its joiners, and co-op already requires the same build on both sides. If a
save ever embeds an announcement, it moves to the "yes" rows.

### 1e. Bump files state the migration

A bump file that names `save`, `snapshot` or `pack` must also say how old
saves are read:

```toml
sim = "a swing that cut a body before meeting its blade ..."

[formats]
snapshot = "a swing's contact keeps whether its chance to be turned was drawn (`Contact::drawn`)."

[migrations]
snapshot = "old saves read `drawn = false`: no swing in a save is mid-contact"
```

or, when there is no way to read old saves:

```toml
[formats]
snapshot = "…"

breaking = "why old saves can't be read, and what the story restore keeps"
```

- `crates/bumps` parses `[migrations]` and `breaking`. For each of `save`,
  `snapshot` and `pack` in `[formats]` there must be a `[migrations]` entry or
  a `breaking` line, else the parse fails, and with it the build (as a bad bump
  file does today). A `[migrations]` entry for a format not in `[formats]` is
  an error too.
- `xtask pin` writes the migration lines into the version history comments
  next to the history line (`// 73: … — old saves: …`), and a `breaking` line
  into `bumps/BREAKING.md`, which release notes are built from.
- A `breaking` bump sets `OLDEST` to its own version at pin time. *My call:*
  allowed only with the owner's approval in the PR, and only when the story
  restore (part 4) can carry players over.
- `bumps/README.md` gets a short section on writing a migration, and the
  save-compat skill shrinks to "pull, run, it loads; if not, that's a bug".

The text alone proves nothing. CI proves it with the format fixtures (2a).

## 2. Old saves that must load

### 2a. Format fixtures (public, CI on every PR)

Real saves can't go in the public repo (anti-goals), and CI has no games. So
the format half is checked with **synthetic saves**:

- A *kitchen-sink* save built in code, with no game data: a small test level
  (the sim's tests already build worlds in code) and every field the formats
  hold set to a non-default value. A test fails if a field of `Snapshot`,
  `script::State` or a KJKT template encodes the same as its default, so new
  fields get added to the generator.
- `xtask pin` writes it, at each pin that moves `save`, `snapshot` or `pack`,
  to `crates/sim/tests/saves/kjkv-<N>.kjkv` and commits it with the pin.
- A test (`sim`, runs in CI's existing test job) decodes every fixture with
  the current code, re-encodes it, decodes that again and compares, and loads
  it with `Save::world` on the test level.
- On a PR the newest fixture is main's version and the branch is main + 1,
  so the PR's own migration is what the test exercises.

Fixtures from before a `breaking` bump move to `tests/saves/retired/` and are
only checked to be refused with the right message.

### 2b. Real saves from every release (the owner's machine, before a tag)

The data half needs the games' files, so it runs where they are:

- **Corpus**: per release, at least Endar Spire, Taris, mid-Korriban and TSL
  Peragus, each as a slot folder (`save.kjkv`, `meta.json`, `story.json`). They
  come from the `kjk chain` checkpoints of that release's tag (K1: Endar Spire,
  Taris, Korriban; TSL: the prologue route to Peragus), plus any owner save
  worth keeping. `kjk saves capture <tag>` copies them in.
- **Where it lives** (the owner's choice):
  `kjk-artifacts/save-corpus/<tag>/<name>/`, git-ignored, mirrored to the
  private archive repo so it isn't lost with a disk. The release checklist
  includes the mirror step.
- **Gate**: `kjk saves check [<corpus>]` runs every save through the whole
  ladder on main: migrate, re-stamp, `Save::world`, 300 headless ticks, then
  walk back into one module left behind (the `StashChanged` path). It prints a
  table per save: formats migrated, stamps moved, objects reconciled, fallback
  used or not.
- **When**: before every `v*` tag, as a step of the release checklist in
  `docs/` (release.yml can't run it, having no games). *My call:* a save that
  needs the story restore is a failure in this gate unless its release is
  before a `breaking` bump.

### 2c. The chain gets it for free

`kjk chain` checkpoints are saves. After this, a format bump migrates them
instead of invalidating them, so the chain re-runs only what the change
touched. The chain's cache key still includes the checkpoint bytes, so
nothing stale is reused. *My call:* the chain keeps regenerating checkpoints
on `sim` hash moves, as now; migration only saves the re-run on format-only
bumps.

## 3. Re-stamping at load

*Status 2026-10-30:* 3b is built (`sim::Save::restamp`,
`SimWorld::stale_stamp`, `sim::Restamp`; the approved test
`stale_stamps_load`). 3c's reconciliation and the new snapshot fields it
needs (component digests, origin keys and rosters, per-continuation script
digests) are not built yet: a re-stamped save is trusted as `restamp.rs`
trusted it.

### 3a. What a digest mismatch means

A digest changes when kjk reads the same game differently (collision, nav,
anims, the script library) or the player's game files differ (patch, mods).
Neither makes the saved state wrong in itself. The save-compat skill's
`restamp.rs` proves this every time the owner pulls: re-stamp blindly, and the
save plays.

So a digest mismatch **warns and re-stamps**. It refuses only on what truly
can't be matched (3c).

### 3b. Where it happens

- **The current module**: `Save::world` no longer refuses on
  `LevelId::check`. If the name matches and the digest doesn't, it logs
  `save-migrate: <module> data changed since the save (old → new), re-stamped`,
  sets `save.level` and `snapshot.place.digest`, and goes on. A different
  module *name* is still a refusal (that's a bug, not a data change).
- **Modules left behind**: re-stamped lazily, where `StashChanged` is raised
  today (`install.rs:284-295`). If the stashed `Place` differs from the new
  level's only in `digest`, the install logs the same warning, adopts the new
  digest and goes on. A different game, module or player count still refuses.
  This avoids planning every module left at load, which is what makes
  `restamp.rs` slow.
- The `StashChanged` and `WrongLevel` errors stay for the cases that are still
  refused, with the safety net's refusal text.

### 3c. What truly can't be matched

After re-stamping, the level is reconciled with what the save holds. Each rule
is engine-wide:

- **Positions off the walkable mesh** (nav or collision moved): the entity is
  moved to the nearest walkable point within 2 m, logged. Further than that:
  to the module's nearest waypoint, logged.
- **Saved objects whose source is gone from the module** (a GIT entry or
  template removed): dropped, logged, unless they are party members or carry
  items of the party, which are kept where they stand.
- **Objects the module's data now has that the save doesn't**: they appear
  only if the save knows the module's original object list and they weren't
  in it (new in the data), not if play destroyed them. This needs an *origin
  key* (GIT index + template resref) on each saved entity, which entities
  don't carry today, and a per-level *roster* of the origin keys the module
  had at first install, kept with each place. Both are new snapshot fields
  (so a migration: old saves' entities have no origin key, are matched by
  tag and position where they can be, and no new objects are added).
- **Saved script continuations into changed scripts**: delayed commands and
  action queues point into compiled script code. If the script library digest
  changed, every saved continuation is checked against its script's own
  bytes. That needs a per-script digest kept with each continuation, another
  new field (old saves: trusted, as `restamp.rs` trusts them today). A
  continuation whose script changed is **the one thing that truly can't be
  matched**: resuming old code offsets in new code is undefined. The load
  stops there and offers the story restore (part 4).

To tell *which* component moved (only nav? scripts?), the save also keeps the
component digests (`Digests`) beside the combined one. Old saves have only the
combined one, so for them every component is treated as possibly changed,
which is safe.

### 3d. What the player sees

Nothing, when 3a-3c succeed. The log gets the lines above, and the safety
net's bug report picks them up from `kjk.log`. *My call:* no in-game notice,
since a digest change is the normal case after every update.

## 4. Story record and story restore

### 4a. What it is

`story.json`, written in the save's slot folder next to `save.kjkv` and
`meta.json`, in the same atomic write and `fsync` as the safety net's 1c. It
holds the player's progress **keyed only by the games' own identities**
(resrefs, tags, GIT indices, global names), never by `SimId` or anything
kjk-internal. That's why it survives any engine change.

Contents, from `script::State`, the snapshot and the sheets:

- `schema` (its own version), `game`, `build`, `saved`;
- **where**: module, position and facing of the leader, and the nearest
  waypoint's tag as a backup;
- **globals**: numbers, booleans, strings, places;
- **party**: available NPCs (template resref), members, leader, influence,
  TSL bases, AI styles, and for each character: name, template, appearance,
  portrait, class levels, attributes, skills, feats, powers, HP/FP and maxima,
  alignment, XP;
- **items**: per holder, equipped slots and inventory as template resref,
  stack size, charges, upgrades; gold, components, chemicals;
- **journal**: entries and their states;
- **galaxy** map state, **reputation** adjustments, `unescapable`, `transit`;
- **visited modules**: per module left behind, whether its spawn scripts ran,
  the origin keys of objects destroyed, door and placeable states (open,
  locked) by origin key, and object locals by origin key.

It's JSON so it can be read, diffed and repaired by hand, and it's small
(tens of kB). Its `schema` only grows: fields are added, never removed or
renamed, and a reader ignores what it doesn't know. The safety net's
`meta.json` `formats` gains `story: <schema>`. *(To agree with the safety
net's author before building; it's an additive optional field.)*

### 4b. Story restore

When rung 2 or 3 fails and `story.json` exists, the game shows: "This save
was made with 0.3.1 (ab12cd34) and can't be loaded exactly. Restore your story
progress? Party, items, journal and choices come back; modules you already
visited may be reset." **Restore** / **Cancel**.

Restore:

1. Installs the saved module fresh, as a new arrival, and places the party at
   the saved position (or the waypoint).
2. Sets globals, party, sheets, items, journal, galaxy, reputation.
3. For each visited module, keeps the facts above in the stash form a later
   install applies (destroyed objects stay gone, doors stay open, locals
   restored), so walking back in looks mostly as left. What's lost: loot on
   the floor, bodies, creatures mid-fight, half-done conversations, timers.
4. Doesn't write over the slot. The player's next save is a normal save.

The owner chose this lossy restore as the last rung, over refusing the save.

### 4c. Kept honest

- A round-trip test: for each format fixture and each corpus save, write
  `story.json`, restore from it, write it again; the two records must be equal
  (ignoring `build` and `saved`).
- The corpus gate (2b) restores every save from its story record as well as
  loading it, so the fallback is exercised on real saves at every release,
  not only on the day it's needed.

## 5. Later: tagged per-system sections

Parts 1-4 make bumps cheap but still mean writing a `v >= X` line per change.
Part 5 makes most bumps need none:

- `wire::Writer::tagged(tag: [u8; 4], version: u16, body)` writes tag,
  version and length. `Reader` walks the tags: a known tag is read with its own
  version; an unknown tag is skipped; a missing tag gives its system's default.
- The snapshot tail (talk, goods, progress, pazaak, fog, visuals, screens…),
  each entity's optional parts, and `script::State`'s systems become tagged
  sections. The head, the entity list and the stash keep their structure so
  `split`/`Parts` network diffs still work.
- A change to one system bumps that section's own version and touches only its
  reader. Adding a whole system needs no migration at all. The format-wide
  KJKS/KJKV numbers then move only for structural changes.
- The story half and the transient VM half of `script::State` go in separate
  sections, so a change to action queues never touches the story.

*My call:* this moves the sim hash once (the snapshot's bytes change), so it
lands as its own PR after parts 1-4, with its own bump file carrying a
migration from the last untagged versions. It is designed now so parts 1-4
don't block it, but built only when the owner asks.

## Order of work

1. Part 1: `header_since`, the version argument, `since.rs`, `[migrations]`
   and `breaking` in `crates/bumps` and `xtask pin`, the save's pack version,
   docs. Waits for the
   safety net's `crates/saves` work to merge; parts 1 and 2a touch only
   `crates/sim`, `crates/bumps` and `xtask`, so they can start sooner.
2. Part 2a: kitchen-sink generator, first fixtures at the current versions,
   the CI test.
3. Part 3: lazy re-stamp, `Save::world` warning, reconciliation, the
   component digests, roster and per-continuation digests (one bump with its
   own migration).
4. Part 4: `story.json` writer, the restore, the round-trip test.
5. Part 2b: `kjk saves capture` / `kjk saves check`, the corpus from v0.1.0's
   checkpoints, the release checklist.
6. Part 5, when asked.

Each step merges alone.

## How we know it works

- 2a's fixture test, on every PR.
- A test per reconciliation rule in 3c on the test level: a moved nav, a
  removed GIT entry, a new GIT entry, a changed script under a saved
  continuation.
- `crates/bumps` tests: a format bump without `[migrations]`/`breaking` fails;
  a stray migration fails; `breaking` passes.
- 2b's gate on the owner's machine before every release, starting with
  v0.1.0's saves on the first release that has this.
- The sim hash is unchanged by parts 1, 2 and 4. Part 3 moves it once,
  through the normal pin.

## Left to the builder

- Passing the version down: an argument or a field on `Reader`.
- The kitchen-sink generator's level and how it checks every field is
  non-default.
- The exact JSON shape of `story.json`, as long as it keys by game identities
  and only grows.
- How origin keys are derived for objects spawned by scripts (not from a GIT
  entry): resref + spawn order per module is a starting point.
- The nearest-walkable search and the 2 m threshold.
- Where `kjk saves capture/check` live among the launcher's verbs.
