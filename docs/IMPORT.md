# Engine principles and the import step

Status: **accepted** by the owner (2026-10-02); the import step is being
built slice by slice (part 2). Part 1 is in `ROADMAP.md` ("Principles"); part 2
is the design of the import step; part 3 is disk space across worktrees.

## Part 1: principles

kjk is its own engine. KOTOR's story, world and content, and JA's
movement, sabers and Force, come into it as content and as rules to honour,
not as engines to rebuild.

1. **Our engine first.** The core (simulation, networking, rendering, audio,
   tools) is designed the way a modern engine would be, for co-op, speed and
   quality of life. No old engine's limit is copied unless the content needs
   it to work.
2. **Import once, run on ours.** The player's installs are read by the
   importer only. It turns them, once per install, into our own formats in
   a local store (part 2). The engine and the game run from the store, never
   from GFFs, BIFs or PK3s.
3. **Honour the content, not the engine.** Old behaviour is reproduced
   exactly where content depends on it: what a script expects a routine to
   do, the order events fire, KOTOR's rules numbers where they are the
   game's design. Everything else (how things are stored, loaded, drawn,
   networked, timed) is ours.
4. **Scripts: the original bytecode, our routines.** The games' compiled
   scripts run in a small VM, because decompiling or hand-porting
   thousands of scripts would break the story in ways we would find one by
   one. Every routine they call is a general engine system (effects,
   inventory, party, combat, dialogue), designed for the engine and shaped
   by KOTOR only as far as its content needs. The VM is a compatibility
   layer at the edge, not the core.
5. **One model for every game.** K1, TSL and JA content map into the same
   engine schemas (items, creatures, templates and instances, dialogue
   graphs, effects); differences stay inside the importer.
6. **Fast, measured, testable.** Module loads are lookups, not parsing.
   Every claim cites a check, a sweep or a trace.

What this changes in practice:
- The import step (part 2) replaces runtime parsing.
- Objects become **templates and instances**: a module's placements are one
  source of instances, a script's `CreateObject` another, the party a third
  (today: a module index plus a spawned list).
- The renderer **streams** what it draws (today it loads a module's
  creatures with its scene, and a creature made in play from the install
  off the frame it comes in, `game::made`).

## Part 2: the import step

### What it is

`kjk import <game>` reads an install and writes **products** into the store.
Products are our own formats, built for loading speed. Anything that needs
one and finds it missing imports it then (so nothing has to be run by hand),
but a full import up front makes every later load fast. `--force` makes
every product again; `--module NAME` is the module its check plans with
and without them (`end_m01aa` by default).

| Product | From | Replaces at runtime |
|---|---|---|
| **Game pack**: items, creatures (sheets dressed), placeables, stores, factions, talents, rules tables | UTI, UTC, UTP, UTM, 2DA | `session::sheets`, `items`, the catalogs read every plan (~20 ms, growing) |
| **Dialogue pack**: every DLG as our graph, voice lengths, lip sync | DLG, WAV headers, LIP | reading voice files at plan time (made client plans ~4× slower) |
| **Script pack**: every NCS, the routine catalog, string facts (named templates, scripts) | NCS, `nwscript.nss` | the per-module library gathering |
| **Module pack**: objects, links, entry, sound, the area's collision and navigation mesh | IFO, ARE, GIT, LYT, WOK, MDL | `ModuleIr`, `AreaCache` (in memory only today) |
| **Render pack**: models and animations in our IR, textures in GPU formats (BCn/KTX2), generated WGSL | MDL/MDX, TPC/TGA, TXI | per-run model and texture decoding |
| **Audio pack**: decoded or re-encoded sounds and music; movies via `ffmpeg` | WAV, MP3, BIK | per-run decoding |
| **JA pack**: animations, sabers, FX, sounds | PK3s, GLA/GLM, EFX | per-run JA loading |

The sim and the game see the packs only; the readers in `fmt_odyssey`,
`asset_odyssey` and `fmt_ja` become the importer's.

### Keys, versions, safety

- **Content-addressed.** A product's key is a hash of its importer's format
  word and its inputs (their bytes, or the install fingerprint for a whole
  pack). Two branches with different importers write different keys: they
  never overwrite each other. A key hit is always the same bytes a miss
  would write (PLAN's cache rule).
- **Atomic.** Written to a temporary file and renamed into place: a reader
  never sees half a product, and two processes importing the same thing at
  once both end with the same file.
- **Install changes** (a patch, a mod in `override/`) change the
  fingerprint, so the packs that read them are imported anew; the rest are
  kept.
- **Determinism.** A level's digest becomes its packs' keys: two machines
  with the same keys run the same content (the join handshake already
  compares fingerprints).
- **Never shipped.** The store holds game content, so it lives outside every
  repository, and the publish check never sees it.

### Where: one store for every worktree

`KJK_STORE` if set, else the user's cache folder:

| OS | Default |
|---|---|
| macOS | `~/Library/Caches/kjk` |
| Linux | `$XDG_CACHE_HOME/kjk` (else `~/.cache/kjk`) |
| Windows | `%LOCALAPPDATA%\kjk` |

```
<store>/
  import/<game>/<fingerprint>/<product>/<xx>/<key>   packs, by install
  cache/<kind>/<xx>/<key>                             other derived products
  cache/fingerprint/<game>.tsv                        the installs' fingerprints
```

Every worktree, the main checkout and later the shipped game use the same
store: a game is imported once per machine, not once per worktree.
`kjk store path` prints where it is; `kjk store du` shows its size by
product and game; `kjk store gc --days N` (30 by default; `--dry-run`
only counts) removes what no run used for N days (a read marks a file used: its
modified time). A kind whose format changes names it (`bake/k1/area-v6`),
and `gc` also removes, whatever their age, the kinds a newer format
supersedes: `<name>-vN` beside a higher `<name>-vM`, and a plain `<name>`
(made before kinds named their format) beside any. Only versions lower than
the highest there go, so an older build's gc never drops a newer one's.

Run folders (`kjk-artifacts/runs/`: recordings, videos, facts) stay per
worktree, since they are a task's evidence; there is no `kjk runs prune`
yet, so old ones are deleted by hand.

### Slices

1. **The store** (done 2026-10-02: the `store` crate): the root, atomic
   content-addressed writes and reads, last use as each file's modified
   time, `kjk store path|du|gc|check` (9 checks); the install fingerprint
   cache moved there.
2. **Game and dialogue packs:** catalogs and dialogue graphs with voice
   lengths imported once; plans read them. This removes the per-plan
   parsing, and the party's conversations can then be loaded in every
   module at no cost (the open point from I19).
   - Done (2026-10-02): `kjk import <game>` fingerprints the install (K1:
     15,374 files, 5.6 s the first time, then from the cache) and makes the
     **templates** (every item and creature template of the shared data,
     `sim::pack::Templates`, 0.2 MB) and the **voices** (13,797 voiced
     lines' lengths, 2.3 s, 0.3 MB). An installer takes them when the
     install's fingerprint is in the cache and they are in the store
     (`session::AreaCache::with_imported`); plans then read only a module's
     own templates, and again each creature template that names an item
     the module gives its own copy of (the templates keep the items each
     creature's lists name, read or not: TSL's `101per` gives T3-M4 its own
     `mininglaser`, and its mining droids a `heavymininglaser` the shared
     data lacks). Its check: a module planned with them is the same level
     and script library as without. The companions' conversations are in
     every module again (462 scripts); with the import a plan stays at
     about 290 ms, without it the first plan on a cold disk took 750 ms.
   - Still per plan: parsing the DLGs and the scripts themselves (the
     script pack, slice 3).
3. **Script and module packs:** library, module data, collision and
   navigation from the store (the level cache becomes persistent and
   shared).
   - Done (2026-10-02): **area bakes**. An area's collision and navigation
     mesh are kept in the store once built (`bake/<game>/area-v6`), keyed by
     the install's world data, the module, the walking clearance and the
     builders' own code (`collision_odyssey::BUILDER`, a hash of the
     collision crates' sources, the `fmt_odyssey` and `asset_odyssey`
     readers they build from, and the clip files, stamped at build time, so
     changed code never reads an old bake; the containment audit and the
     probes, `audit.rs` and `probes.rs`, are left out: they check a bake and
     take no part in it). They are written bit for bit
     (`collision::Level::encode`, `NavMesh::encode`, every float by its
     bits) and deflated (about half: K1's 118 areas take 1.0 GB); a decoded
     one is checked to point only inside itself. `kjk import`'s check plans
     a module from the install and from the store: the same level,
     collision, navigation and library. Reading a bake took about 14 ms
     raw (deflate adds some tens of ms) against about 140 ms to build the
     Endar Spire.
   - Since 2026-10-02 (`area-v3`), area bakes are in **LZ4**, not deflate:
     inflating took 50 to 70 ms of a large area's read; the Endar Spire's
     collision now reads in about 19 ms (it was 65 to 77), its whole plan
     in about 155 ms (was about 255). The `area-v2` bakes stay for
     worktrees not yet rebased, until `kjk store gc` drops them.
   - Since 2026-10-02 the bakes' folders name their format
     (`bake/<game>/area-v6`, `bake/<game>/library-v2`), and `kjk store gc`
     drops the superseded ones whole: on this machine the old
     `bake/k1/area` (deflated `area-v2` and the first `area-v3` bakes) and
     `bake/k1/library`, 126 files, 1,036 MB. An area left in the old folder
     is built once more (about 140 ms) the first time it is planned.
   - Done (2026-10-02): **script library gatherings**. A module's library
     (the scripts and conversations its objects and the creature templates
     reach, and the strings its scripts hold: `sim::pack::Gathered`, our
     own encoding of each conversation) is kept in the store once gathered
     (`bake/<game>/library-v2`, deflated, about 0.3 MB a module), keyed by the
     install's world data, the module, what the walk starts from and the
     gathering's code (`KJK_LIBRARY_GATHERER`, a hash of
     `session/src/library.rs` and the DLG, GFF and NCS readers). Each DLG
     is parsed once now, not three times, and the library's digest is of
     each conversation as it runs (our encoding), not of the DLG's bytes.
     The Endar Spire's library: about 50 ms a plan before, 10 to 20 ms
     from the store. `kjk import`'s check covers it (K1 and TSL).
   - Still per plan: parsing the module's IFO, ARE and GIT (the module
     pack, about 10 ms; not stored yet).
   - Since 2026-10-02 an install is **opened once a process** and shared
     (`game::installs`): its files' index (about 60 ms for K1), its
     fingerprint from the cache, and its base resolver (`chitin.key`, the
     BIF tables, the global archives, about 55 ms), which the installers,
     the game's screens, the HUD, the dialogue view and the play's bodies
     all stack on. Before, a menu run opened K1 four times, two of them
     before its window. A run that plays (the menu, `kjk play`, `host`,
     `join`, `module`) opens the installs on threads at launch
     (`installs::warm`), so they read while the window comes up: the menu's headless start and quit took about 0.6 s, now
     about 0.36 s; `kjk play` to its first world about 2.1 s, now about
     1.85 s. The run's manifest lists each install's opening times.
   - Done (2026-10-02), the first of the **JA pack**, filled as runs read
     (no import step: each product is keyed by what it is made from, so
     it needs no install fingerprint):
     - Archive entries of 1 MB and more are kept unpacked
       (`import/ja/unpacked`, keyed by the PK3's name, length and time
       and the entry's name, sizes and CRC). Only the skeleton's
       animations (`_humanoid.gla`, 11.8 MB) are that large; a run read
       it three times, each inflate tens of milliseconds and hundreds on
       a busy machine. Read back: about 5 ms.
     - The shader-to-texture map (every `.shader` file parsed, 3 times a
       run, ~110 ms on a busy machine) is read once a run and kept
       (`import/ja/shader-textures`, keyed by every shader file's CRC and
       the reading code's stamp, `KJK_JA_SHADERS`).
     - Decoded sounds (`import/sound/clip`, keyed by the file's bytes,
       `audio::Clip::encode`): 103 clips read back about 3 times as fast
       as decoding their MP3s (8–16 ms against 22–55 ms, measured in one
       process on a busy machine); 38 MB, as the decoder makes every
       sound stereo 32-bit float.
4. **Render and audio packs, and streaming:** models, textures and sounds
   from the store, loaded when first needed.
   - Done (2026-10-02): creatures made in play (a companion carried into
     the next module, a script's `CreateObject`) load off the frame they
     come in, on the task pool (`game::made`), and are spawned in a
     later one: 0.1 to 0.2 ms of a frame, where loading took 20 to 300 ms
     of one (half a second from a cold disk). docs/WORLD-STATE.md has the
     measures.
   - Done (2026-10-02, I20): **movies**. Each Bink made ours by `ffmpeg`
     (JPEG frames and an MP3, `movie`), in `import/<game>/movies` keyed by
     the Bink's bytes, made when first played or all at once by `kjk movie
     import <game>` (K1: 64 movies, about 600 MB). The owner's choice: the
     **sounds stay the install's** (MP3s behind RIFF headers, IMA ADPCM,
     PCM), read and decoded per area on a thread of the view's; as PCM in
     the store they would take about ten times the space for nothing a
     load needs (`SOUND.md`).
5. **Templates and instances** in the sim, replacing module index plus
   spawned list.
   - Done (2026-10-02): **one instance table** per area
     (`sim::instance::Instances`): the module's placements first (a
     placement's number is still its entity in the collision), then what
     play made (scripts' `CreateObject`, `SpawnAvailableNPC`, the party's
     companions carried in), each with its facts. The world, the scripts,
     installs (companions, the stash), saves and every view and probe find
     an object's facts by its number there, never by index into the
     module's objects or the spawned list; an object entity carries its
     facts. The snapshot still names what play made by template (no
     snapshot or save format change).
   - **Placements share their templates:** a creature its GIT entry gives
     nothing of its own beyond its place is its template where it stands
     (`module::Placement::Template`), the template the script library has
     (from the store when imported), its UTC not read in the plan; a
     creature that differs (its entry overrides the template, an encounter
     changes it) keeps its own facts. Every creature of K1 (1,619 in 117
     modules) and TSL (1,700 in 82) shares its template. The module digest
     (KJKM 9) names the template and lists each one once: K1's creature
     facts in the module digests went from about 1.19 MB to 0.84 MB in all.
     Reading a module's objects in a plan took 4.9 ms on average over K1's
     modules, now 3.8 ms; `kjk launch-report` (end_m01aa, 5 runs): the plan
     68 to 67 ms, its module and objects 9 to 7 ms.
   - Recordings and saves made in a module before KJKM 9 no longer load
     (the level's digest changed); the proving ground's did not change but
     for the version, so the pinned hash moved for that alone.

I19's remaining parts (journal, XP, stores) go on top of slice 2.

## Part 3: disk space across worktrees

Measured 2026-10-02, the disk at 89% (51 GB free):

| Folder | Main checkout | A typical worktree |
|---|---|---|
| `target/` (Rust builds) | 13 GB (11 GB debug, 1.9 GB release) | up to 6.8 GB |
| `kjk-artifacts/runs/` | 1.5 GB | up to 1.7 GB |
| the store | (4 MB today) | |

So the shared store stops game data from multiplying, but most of the space
is Rust's build folders. Options (they combine):

- **A. Smaller debug builds** (recommended, one line in `Cargo.toml`):
  dependencies without debug info and our crates with line tables only
  (backtraces still name file and line). Usually halves `target/debug`.
- **B. One shared build folder** (`CARGO_TARGET_DIR` for every worktree):
  the biggest saving, but cargo locks it while it builds, so parallel
  agents wait for each other, and switching branches rebuilds our crates.
- **C. A shared compile cache** (`sccache`): faster first builds in a new
  worktree, no disk saving.
- **D. Housekeeping:** `cargo clean` in worktrees whose branches merged;
  old run folders deleted (no `kjk runs prune` verb yet).

The owner chose **A** and the housekeeping (2026-10-02): the main
checkout's `target/` went from 13 GB to 2.1 GB after a rebuild (its debug
part from 11 GB to 755 MB), and the merged worktrees' build folders were
removed (the dialogue worktree's 6.8 GB among them).
