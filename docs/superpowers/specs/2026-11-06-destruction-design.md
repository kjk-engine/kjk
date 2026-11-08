# Environmental destruction — design

2026-11-06. Owner: reclaimterrain. Status: second pass built 2026-11-08
(props fly apart into pieces of their own models, hits land where they
strike and leave marks, damage props crumple; workbenches intact):
`docs/COMBAT.md`, "Destruction". The second pass followed the owner's
playtest of the prop hall: "Second pass: what breaking looks like"
replaces "What breaking looks like" below where they differ. Rooms are
still to come.

Companion table: [`2026-11-06-destruction-props.csv`](2026-11-06-destruction-props.csv),
every kind of placeable used in K1 and TSL (351 appearance rows, 6,437
instances) with its class, material, size and how many instances stay
breakable once the guards below are applied.

## Intent

Fights in K1 and TSL should leave a mark on the world. Grenades, saber and
melee swings, blaster bolts and Force powers damage what they hit, not only
creatures. A grenade in a cluttered room should feel like a grenade: chairs,
jars, crates, plants fly apart, consoles spark. It has to be fun ("lots of
destruction"), but it must never break the game, and it is built entirely by
agents, so every part has to be generated and checked automatically rather
than placed by hand.

## Goals

- Every damage source the party and its enemies use can damage the world:
  weapons (melee, saber, fists, blasters), grenades, Force powers.
- Props across both games break or show damage, from KOTOR's own placeables.
- Later (slice 2) the rooms themselves take damage: chunks off pillars,
  railings snapping, panels blown out.
- Damage is graded, not on/off: small arms mark and chip, heavy weapons
  wear things down, grenades and area powers wreck a room.
- Debris is made from KOTOR's own meshes and textures, cut into pieces.
- Co-op: every seat sees the same broken world. Saves and areas left keep it.

## Non-goals

- No new paths, ever: nothing breaks open a wall, door, floor or shortcut.
- No blocked paths: no rubble, fallen pillar or wreck ever blocks walking.
- Lootable containers stay intact in slice 1 (owner, 2026-11-06). Dropping
  their contents as remains when destroyed is a follow-up (below).
- Bodies, corpses, skeletons are not part of this (gore is its own topic).
- No hand-drawn effects or new art (owner's standing rule: KOTOR's own
  models, textures, emitters and sounds only).
- No new props placed into modules (no new content; polish over content).

## Anti-goals (a failure even if it "works")

- A quest, script, dialogue or cutscene behaves differently because
  something broke. The chain must play exactly as before.
- The smallest blaster obliterates everything. Pistols mark and chip.
- Co-op seats disagree about what is broken, or the snapshot grows enough to
  matter for bandwidth.
- Frame drops when a grenade goes off in the densest room.
- Debris that looks invented, or floats, jitters or piles up forever.

## What the engine already has (survey 2026-11-06)

- Placeables read `HP`, `CurrentHP`, `Hardness`, `Plot`, `Min1HP`, `Static`,
  `Useable` (`crates/asset_odyssey/src/module.rs:932-967`).
- Bashing: party weapons and grenades hurt **locked, non-plot** doors and
  containers; damage less hardness × `interaction.bashing.times` (7)
  (`crates/sim/src/world/objects.rs:188-249`, `docs/COMBAT.md:221-260`).
  `Source::Power` never hurts objects.
- `ObjectState { state, lock, damage }` is in the snapshot per entity and in
  the stash of modules left, so it is synced in co-op and saved
  (`crates/sim/src/snapshot.rs:230-290`). `ObjectState::GONE`
  (`collision::GONE`) already makes a placeable passable.
- Bolts stopping on a level entity, saber sweeps and strikes meeting bashable
  entities, grenade bursts catching bashable objects in `grenades::RADIUS`
  with a line-of-sight trace (`crates/sim/src/world/bolts.rs`, `sabers.rs`,
  `strikes.rs`).
- Looks-only Rapier physics for ragdolls (`docs/RAGDOLLS.md`), mesh cutting
  with caps (`crates/ragdoll/src/cut.rs`), KOTOR emitters incl. chunk
  emitters (`docs/MODEL-FX.md`), `placeablesounds.2da` "destroyed" sounds.
- Every string a module's NCS and DLG use is gathered
  (`crates/session/src/library.rs:764-855`), so props scripts look up can be
  found.
- Nav is baked once per area and never changes; marks (saber burns, FX
  decals) are view-only and land on rooms only.

Gaps: unlocked props can't be hurt; powers never hurt objects; a broken
placeable keeps its collision; room geometry has no identity in collision
(no room/node id per brush) and takes no damage.

## What the inventory says

Every module of both games, every GIT placeable, read with the UTP as
fallback (throwaway tool, 2026-11-06):

| | K1 | TSL |
|---|---|---|
| Placeable instances | 3,214 | 3,223 |
| Kinds (appearance rows) | 157 | 194 |
| Break outright (after guards) | 882 | 1,190 |
| Damage only (after guards) | 207 | 220 |
| Stay intact | 2,125 | 1,813 |
| Modules with something to hit | 75 of 111 | 48 of 78 |

Findings that shape the design:

- **HP and Hardness are toolset defaults** (HP 15 on 86% of K1 and 99% of
  TSL placeables, Hardness 5 on 99.9%). They can't tell a jar from a
  computer, so destruction uses its own toughness by material and size. The
  existing bash rule keeps reading the UTP values, unchanged.
- **Static placeables are a third of everything** (K1 1,194, TSL 1,396: TSL
  places its chairs, barrels, cabinets, tables this way). They are where most
  of the coverage comes from, so `Static` does not exclude a prop by itself.
- **Invisible placeables are script anchors** (a quarter of K1). Never touched.
- **Only 39 (K1) / 32 (TSL) kinds have a `damage`/`die`/`dead` animation**,
  mostly containers and panels. Everything else needs generated fracture.
- **Coverage is uneven.** K1: 58 of 111 modules have fewer than 5 things to
  hit (many are cutscene `stunt_` modules, but also the Star Forge
  `sta_m45*`, Leviathan `lev_m40ac`, Kashyyyk Shadowlands, Rakatan temple
  `unk_m4*`, some Taris sewers). TSL: 40 of 78 (the Ebon Hawk, Peragus
  `10xper`, Harbinger, Nar Shaddaa docks, Dxun, Onderon palace, Malachor).
  Props alone can't make "stuff throughout"; slice 2 (rooms) is what reaches
  those places.

## Which props, and how

Every kind of prop gets one class in a reviewed data table
(`data/destruction/props.csv`, already on the `claude/prop-hall` branch,
where the arena's prop hall reads it; the companion CSV adds counts):

- **break** — destroyed outright: pottery and bottles, plants and food,
  furniture (chairs, benches, beds, tables, desks, shelves, cabinets,
  barrels, cans), loose junk and rubble piles, empty crates and lockers.
- **damage** — shows damage, never goes: consoles and panels (sparks,
  cracked screens), machinery and vehicles (swoops, speeders, kolto tanks,
  cages, droids), stone (statues, monuments, rune pillars), cloth (flags,
  banners, carpets). These are usually too big, too load-bearing for a
  scene, or too often a scene's focus to vanish.
- **intact** — never touched: invisible anchors and barriers, force cages,
  effects (steam, smoke, sparks, fire, water, insects), bodies, holograms,
  star maps, puzzles (Ring of Hanoi), pickups (crystals, datapads), story
  machines (hyperdrive, shuttles, escape pods, battle screens).

Each prop also gets a **material** (ceramic, organic, wood, plastic, metal,
electronic, stone, cloth, debris) and a **size** (small, medium, large).

On top of the class, **guards per instance** make a prop intact whatever its
kind. Any one of these is enough:

1. `Plot`.
2. `HasInventory` (a container: intact for now).
3. `Useable` (something you click is part of the game).
4. A conversation.
5. Any event script set (OnHeartbeat, OnUsed, OnDeath, OnDamaged, …).
6. Its tag or template is among the strings of the module's scripts or
   dialogues (`Library.strings`), or of the shared scripts the module runs.
   The survey only checked the module's own NCS/DLG; the real guard must
   include the shared script layers too.
7. `Min1HP`.
8. Locked props keep today's bash behaviour exactly.

The guards are computed when the module loads, never hand-maintained. The
table is per appearance; per-instance exceptions, if any are ever needed, go
in the same file by module + tag.

## How much damage it takes

KOTOR's weapon damage stays KOTOR's. Destruction adds its own toughness, set
by **feel targets**: how many average hits a source needs. These targets are
the spec; the hardness and HP per material and size in
`data/rules/destruction.toml` are tuned until a probe test shows them.

| Source | small (jar, plant) | medium (chair, crate, table) | damage-only things |
|---|---|---|---|
| Blaster pistol | 1–2 shots | 6–10 shots | marks; first damage stage after ~10 |
| Rifle, heavy pistol, repeater burst | 1 | 3–5 | first stage after ~5 |
| Vibroblade, melee, fists | 1 (fists: 2–3) | 3–4 (fists: never) | first stage after ~4 (fists: never) |
| Lightsaber | 1 | 2 | first stage after 2; wrecked after ~6 |
| Frag / concussion grenade | everything in radius | everything in radius | one or two stages up |
| Thermal detonator, plasma | everything | everything | wrecked |
| Force Wave / Storm / area powers | everything in area | most in area | one stage up |
| Force Push, Whirlwind (single) | breaks the target | breaks the target | one stage up |
| Force Lightning | organic and cloth burn | wood burns | electronics: wrecked fast |

Rules behind the table:

- A hit counts only for the damage above the material's **hardness**, so
  weak hits on hard things only mark them (blaster on stone or metal).
- **Damage-type multipliers per material**, from KOTOR's 13 damage types: ion
  wrecks electronics but barely touches stone; slashing and saber energy cut
  wood and metal; fire and sonic hit glass, ceramic, cloth, organic hard;
  bludgeoning (Push, Wave, fists) shatters ceramic and wood.
- **Area beats single.** Grenades and area powers get a destruction
  multiplier, so "lots of destruction" comes from explosions and Force, not
  from shots.
- Everyone's attacks count, the enemies' too: a stray bolt or an enemy
  grenade breaks things just as the party's do. (My call; it's what makes
  firefights look like firefights. AI never targets props on purpose.)
- Breaking a prop gives no XP and makes nobody hostile.

## What breaking looks like

Damage stages come from the damage fraction already in `ObjectState.damage`,
so they need no new state:

- **break** class: intact → damaged → broken.
- **damage** class: intact → scarred → wrecked (stays wrecked).

Visuals, using KOTOR's own material only:

1. **The model's own animations first.** Where a model has
   `damage`/`die`/`dead`, those clips show the stages.
2. **Generated fracture otherwise.** An offline pass (in `kjk import`, cached
   content-addressed in the render pack like everything else) cuts each
   breakable model into roughly 6–16 chunks with seeded cuts, capping the cut
   faces from the model's own texture (the cap approach of
   `crates/ragdoll/src/cut.rs`). The seed comes from the model, so every
   machine builds the same chunks.
3. **Damaged stage** for props without a damage clip: a few small chunks
   knocked off (looks-only), plus the hit's KOTOR impact effect and sound.
4. **On break:** chunks fly through the existing looks-only physics against
   the room triangles. The **base chunk** (the one at the prop's foot) stays
   where the prop stood and is not simulated, so every seat and every save
   shows the same wreck. Flying chunks settle and fade after ~10–20 s, and
   are dropped on leaving the area (the agreed "big damage stays, small bits
   fade").
5. **Effects and sound:** KOTOR's own impact, spark, smoke and explosion
   emitters per material, and `placeablesounds.2da` "destroyed" sounds (by
   material where the prop has none).
6. **Damage-only things** (consoles, machinery, stone): the scarred and
   wrecked stages chip chunks off and add KOTOR's spark or smoke emitters
   (consoles keep sparking when wrecked), but the prop's footprint and
   collision stay.
7. Caps: at most a rule-set number of live flying chunks (start: 256); the
   oldest fade first.

## Second pass: what breaking looks like (owner playtest 2026-11-08)

The owner sliced and Force-pushed the prop hall's props and found the first
pass wanting: broken props just vanished, damage-only props looked
invincible, and no hit showed where it landed. Their four rules:

1. **Big and wall-mounted things never split or vanish.** Large consoles,
   wall panels, terminals, desks and machinery crumple instead: dents,
   scorch, screens that flicker and die, sparks and smoke.
2. **Workbenches and lab stations are intact.**
3. **Every hit shows where it landed**, as that material takes it, and
   leaves a mark that stays.
4. **Nothing just disappears.** A breakable prop flies apart into pieces of
   itself.

### Classes, revised

- **intact**, added: the workbenches and lab stations, `RepairTable1`,
  `RepairTable2` (both games) and TSL's `Work_Table02`, and any further
  row the owner names after the next playtest.
- **damage** instead of **break**: the desks (`Desk1_office`, `Desk2_fancy`,
  TSL's `Ex_Desk01`), the kiosks (`Kiosk1_food`, `Kiosk2_droidparts`,
  `Kiosk3_plain`) and `LargeLocker`: big furniture that stands against a
  wall. The rule from now on: **every electronic or large kind is damage,
  never break**; a test pins it.

### Where a hit lands (view only, no sim change)

The sim keeps no point of impact (`Bash` has none), and none is added: a
hit's point is looks-only, so each machine finds it itself when a prop's
`ObjectState.damage` rises:

- **Bolts:** the view's own bolt trace (`shot_fx`) already ends on the
  prop (`Trace.entity`), with its point and normal: exact.
- **Blades and fists:** the attacker's blade (or hand) segment as drawn,
  against the prop's own triangles: the point where it first crosses them.
- **Blasts and powers, and anything not found above:** the point on the
  prop's body box facing whoever (or wherever the blast) it came from, the
  nearest fighting creature in reach otherwise.

### What a hit shows

At that point, facing that normal:

- **Shards of the prop itself**: three to six small pieces cut from the
  prop's own mesh near the point, in its own texture, flung out along the
  normal; they tumble, land and fade within 2 s. Wood, ceramic, stone,
  plastic and debris throw shards; metal and electronics throw KOTOR's
  sparks instead, plants and cloth its dust splash.
- **KOTOR's impact effect for the material** (as the first pass, now at the
  point, not the middle).
- **A mark that stays**: a scorch for bolts and powers, a burn for sabers, a
  dent's dark streak for blunt blows, drawn on the prop's own triangles as
  `body_marks` draws on bodies, from `saber_marks`' textures. A prop keeps
  its 8 newest marks; marks are looks-only (not synced, not saved).
- **A shake**: small and medium props jolt away from the blow and settle
  (0.15 s, a few degrees); large and damage-class props don't move.

### Damage stages, drawn from the prop's own mesh

The stage is `sim::destruction::stage`, already synced and saved, so every
seat and every save shows the same stage. What each stage looks like is
seeded from the model and the object, so it looks the same everywhere
without sending anything:

- **Scorch:** the prop's material darkens by stage (damaged/scarred ×0.75,
  broken/wrecked ×0.45), on its own copy of the material.
- **Dents (damage class):** at each stage, a few dents pressed into the
  mesh at seeded points on its surface: vertices within the dent's radius
  move inward along their normals, falling off to the rim. Large props get
  more and wider dents. The prop's collision never changes.
- **Screens (damage class):** a screen is a mesh that glows
  (`self_illum` > 0) or animates its texture (a flipbook or a scrolling
  UV). Scarred: it flickers. Wrecked: it goes dark (no glow, no
  animation, darkened) with a burst of sparks as it dies, then keeps
  sparking (the first pass's smoulder).
- **Wrecked damage-class props** keep smoking or sparking as the first pass
  does.

### Breaking: pieces of itself

- **Fracture, generated at load, in the view:** when a breakable prop's
  model is loaded, a task off the main thread cuts its own mesh into pieces
  (small 6, medium 10, large 16): seeded points inside its rest bounds, each
  triangle to its nearest point, each piece's open edges closed by a fan cap
  (the approach of `ragdoll::cut`) textured from the model's own texture.
  Seeded from the model's name, so every machine cuts it the same, and
  cached per model. This replaces the offline import pass of the first
  design: the meshes are small and cutting them at load costs less than a
  new cache format. (My call.)
- **The base:** pieces whose middle is in the lowest quarter of the prop
  stay where it stood as its wreck, drawn and never simulated.
- **On break:** the other pieces fly from the blow (away from the hit, up
  and out, harder for blasts) and tumble as rigid bodies against the room
  in the ragdoll world (`ragdoll::World`, with the level faces it already
  has). Pieces bigger than a fist stay where they settle for the visit;
  smaller ones fade after 10–20 s. At most 256 flying pieces; the oldest
  small ones fade first.
- **A prop already broken when the area loads** (a save, a joiner, a module
  come back to) shows its base only.
- **Never** hidden outright any more, except a model that has no triangles
  to cut.

## Rules for collision, nav and story

- A **broken** prop becomes `ObjectState::GONE`-like for collision: bodies,
  bolts and sight pass where it stood. The base chunk is low and drawn only.
  Nav stays as baked: AI keeps walking around the old footprint, which is
  safe and rarely visible. (My call; re-baking nav is out of scope.)
- A **damage-only** prop keeps its collision in every stage.
- Nothing about breaking a prop runs a script: guarded props are the only
  ones with scripts, and they never break.
- Cutscenes and scripted fights (the Endar Spire battle, `stunt_` modules)
  follow the same rules, with no per-module "no destruction" flag: their
  blows, stray bolts and grenades break things, which may differ run to run.
  The chain proves the story doesn't change; the owner's reel sampling
  judges the look.
- Toughness: for a destructible prop, the destruction table's HP replaces
  the file's default HP, and `ObjectState.damage` counts against it, so the
  stages read from the same field.

## Co-op and saves

- All of it lives in the host's sim: which props break and their damage, in
  `ObjectState` (already in snapshots, saves and the stash of modules left).
  The expectation is **no snapshot or save format change for slice 1**; if
  the builder finds a broken-but-drawn state needs a new bit, it goes in with
  a normal `bumps/<branch>.toml` and a migration.
- Flying chunks are looks-only: each machine plays them from the break event
  and the model's seed. They are not synced and need not match exactly; the
  base chunk and the broken set always match.

## Slice 2: the rooms (outline, its own design later)

The owner wants rooms to take damage too, built after props:

- Collision brushes get room and node identity (today a brush only knows
  contents, surface material, entity, states).
- An offline pass classifies room mesh nodes: free-standing pillars,
  railings, panels, pipes, consoles built into walls. Never floors, walls
  that bound walkable space, doors or anything over walkable floor that
  could fall onto it.
- Damage removes chunks above knee height and leaves a **stump** that keeps
  the node's footprint and walking collision, so no path opens or closes.
  Railings snap and leave posts.
- Per-node damage per module goes in the module state (a format bump).
- Saber burns and blaster scorch on rooms become persistent marks there.
- This is what reaches the sparse modules (Star Forge, Leviathan, Peragus,
  Malachor, Dxun, …).

## Follow-ups (not in slice 1)

- **Container remains:** a destroyed container drops its contents as a
  remains bag (as creature remains already do), so containers can join in.
  The `TSL k_contain_bash` OnDamaged script (61 containers) already opens
  bashed containers; the follow-up has to respect it.
- Bodies and gore.
- Pushing props around with Force Push (moving collision is a bigger topic).

## How it is checked (quality gates)

Every gate is automatic so agents can run it:

1. **Catalogue completeness:** a test fails if any appearance used by any
   module of either game has no class, so new data never slips through.
2. **Guards:** a probe over every module of both games asserts that no plot,
   container, useable, scripted, conversation, script-named or Min1HP
   instance is breakable, and lists the counts (as in the table above).
3. **Feel targets:** a sim probe fires each source at each material and size
   and asserts the hits-to-break table above.
4. **Wreck chain:** `kjk chain k1 --wreck` (and `tsl` once, since this is a
   TSL-relevant system) breaks every breakable prop in a module on arrival
   and, where the route stands still, sets off a test-only blast at every
   room's centre that hits props only (never creatures, so nobody dies or
   turns hostile). It must pass like the plain chain. "Plays as before"
   means story globals and journal match the plain run, as `--droids chaos`
   checks; fights may differ (cover gone, sight opened), as they already do
   between runs. K1 29/29 is the merge bar. Real gameplay destruction is also
   covered by the plain chain's own fights.
   When it catches a prop a script needed (one found without its name: by
   type, by area loop, or by a tag built at run time), the fix is a new
   automatic guard rule covering every such case, with a per-module
   exception in the props table only when no rule can say it.
5. **Co-op:** a two-seat probe where the guest breaks props; both seats'
   broken sets match; `kjk net codec` determinism stays green.
6. **Saves:** break, save, load, leave, come back: the same props are broken,
   the base chunks stand, flying chunks are gone.
7. **Performance:** a frag grenade and a Force Storm in the densest rooms
   (top 3 modules per game by breakable count) keep every frame, the spike
   included, under 16.7 ms on the owner's Mac at 60 fps, measured as in the
   FPS + shadows work. Flying chunks cast no shadows; if the cap of 256
   can't hold the budget, the cap drops, not the frame rate.
8. **Looks:** the arena's prop hall (`kjk play --stage arena`, K1;
   `--stage arena:203tel`, TSL) holds one of every breakable or damageable
   kind, the place to try every source on every material; a reel per material (ceramic, wood, metal furniture, plants,
   consoles, machinery, stone, cloth) sent to the owner. The owner samples;
   anything they flag is fixed as a rule, not one prop.
9. Rulebook lint, `cargo clippy`, the approved tests.

## Build order (slice 1)

One branch, one stacked PR per step, chain at the end of each batch:

1. Catalogue: `data/destruction/props.csv` (exists), the load
   path, the guards, gates 1–2.
2. Sim: `destruction.toml` rules, `hurts` for destructible props from every
   source including powers, collision on break, gates 3, 5, 6.
3. Fracture: offline chunking in import, base chunk, cached; reviewed on
   a handful of models first.
4. View: stages, flying chunks, effects and sounds, caps, gate 7 and 8.
5. Wreck chain (gate 4) over both games; fix what it finds.

Steps 1 and 3 can run in parallel; 2 needs 1; 4 needs 2 and 3.

## Left to the builder

- Exact hardness/HP/multiplier numbers (tuned to the feel targets).
- Chunk counts per size, cut method, fade timings, the live-chunk cap.
- How "broken but base drawn" is represented, provided it stays in
  `ObjectState` and saves.
- Which KOTOR emitters and sounds go with each material.
- The guard implementation details, including how shared script layers are
  scanned.

## Calls I made (not decided by the owner)

- Names: `data/destruction/props.csv`, `data/rules/destruction.toml`,
  `--wreck`.
- Damage-only for consoles, machinery, vehicles, stone and cloth rather than
  destroying them.
- Static props are breakable unless a guard holds.
- Enemies' attacks break things too; no XP, no hostility.
- Broken props become passable; nav stays as baked.
- Bodies and Force-pushing props around are out of scope.
- The prop classification in the companion CSV (owner to skim). Approving
  this document approves the CSV; later class changes go through the same
  table and gates 1–4.
- The wreck chain's blasts hit props only; "plays as before" = story globals
  and journal, not identical fights.
- Frame budget: no frame over 16.7 ms, chunks without shadows.
- Scripted fights and cutscenes wreck their sets like anything else.
- Guard misses are fixed by a new automatic rule before any per-module
  exception.
- Second pass: hit points, shards, marks and shakes are looks-only, found by
  each machine; fracture is cut at load in the view, not offline at import;
  pieces bigger than a fist stay for the visit, not across saves (a save
  shows the base); desks, kiosks and `LargeLocker` move to damage, and
  every electronic or large kind is damage.
