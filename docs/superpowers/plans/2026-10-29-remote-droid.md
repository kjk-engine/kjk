# Remote Droid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A co-op guest with no companion to drive flies a remote droid with a shock bolt, a Sonic Blast and a stim dart, and can never touch the story.

**Architecture:** The droid is a new ECS component, `Drone`, in a new module `crates/sim/src/world/remote.rs`. It carries none of `Avatar`, `Player`, `Fighting` or `Motion`, so every story system skips it with no filter. It is derived each authority step from which seats have no body, so it is never saved. It is sent to clients as `Kind::Remote(Seat)` so they can draw and predict it. Its kit goes through one gate, `remote::fair`, and its hits are credited to the party leader.

**Tech Stack:** Rust, bevy_ecs, the sim's probe framework (`kjk sim probes`), the rulebook (`data/rules/*.toml`), the `kjk chain` runner, and `cargo xtask duo`.

**Spec:** `docs/superpowers/specs/2026-10-29-remote-droid-design.md`. Read it first; this plan argues from it.

## Global Constraints

- Work on branch `claude/remote-droid` in its own worktree, never on `main`. Use one PR, squash-merged (PR title and description become the commit).
- Never edit a version number or `KJK_SIM_HASH`. Add `bumps/claude-remote-droid.toml` with:
  - `[formats] snapshot = "an entity may be a seat's remote droid (`Kind::Remote`, tag 4)."`
  - `[migrations] snapshot = "old saves hold no droids; the droid is rebuilt each step"`
  - no `sim =` line
- Solo play and the pinned determinism run (`sim/src/determinism.rs`, every seat driving) create no droid and draw no RNG. `sim_determinism` must pass with only the snapshot bump.
- The droid draws no randomness: the kit uses fixed numbers, never `rolls`.
- Every number lives in `data/rules/remote.toml` (format: `data/rules/README.md`) and is read as `rules::remote::…`. Regenerate the rulebook with `cargo xtask rulebook`.
- Owner rulings (2026-10-29):
  - a freed companion takes the guest off the droid at once
  - the loss is unchanged: with the whole party down the game is lost, droid or not
  - droid kills give the party XP as any kill does
- Every `kjk` run passes `--mute`.
- Before each commit: `cargo clippy --workspace -- -D warnings` and `cargo test -p sim`. The K1 chain runs at Task 9 and before the PR, not after every task.
- Show visual evidence as it lands: a screenshot or gif per visible task, sent with SendUserFile using absolute paths.

## Review Focus

1. **A guest joins mid-conversation or mid-movie.** The droid must appear only after the hold ends. While held, the guest sees the leader's camera, as today. Test in Task 6.
2. **A companion leaves while its guest is mid-fight** (the Taris Bek base strips, Kashyyyk takes Zaalbar). The droid must launch from that body's last place, not from the leader across the map. Test in Task 1.
3. **The leader changes body** (`SwitchPlayerCharacter`, a portrait-click lend). The leash and the credit must follow the new acting body the same step. Test in Task 4.
4. **The droid is stuck inside a door that closes on it, or below the floor after an install.** It must snap behind the leader within one second, never ending up in solid geometry. Test in Task 2.
5. **Two droids target one downed member in the same step.** Only one revive applies; the lowest seat wins, as FALLING.md's hand revive does. Test in Task 5.

---

### Task 1: The droid body exists, docks and is never saved

**Files:**
- Create: `crates/sim/src/world/remote.rs`, `crates/sim/src/world/remote/probes.rs`, `bumps/claude-remote-droid.toml`
- Modify:
  - `crates/sim/src/world.rs` (`pub mod remote;`)
  - `crates/sim/src/world/state.rs:55-60` (the kind mapping), `:320-397` (restore spawns a `Drone` from `Kind::Remote`) and `:411-458` (the schedule)
  - `crates/sim/src/snapshot.rs:192` (`Kind`), `:2293`/`:2385` (encode and decode, tag 4) and `:1300` (`own` never matches `Remote`)
  - `crates/sim/src/save.rs:146` (`Save::world` drops `Kind::Remote` entities)
  - `crates/launcher/src/sim_probes.rs` (register `("remote", sim::remote::probes)`)

**Interfaces:**
- Produces:
  - `pub struct Drone { pub seat: Seat, pub at: Vec3, pub velocity: Vec3, pub yaw: f32, pub ready_ms: [u64; 3] }`, a bevy `Component`. The `ready_ms` index is `KIT_BOLT = 0`, `KIT_SONIC = 1`, `KIT_DART = 2`.
  - `pub enum Kind { …, Remote(Seat) }`
  - `pub(super) fn launch(world: &mut World)`. It runs on authority, chained right after `drivers::assign`. It spawns a `Drone` for each held seat ≥ 1 that sent a command, drives no body, and is not in a minigame. It despawns the drone of a seat that now has a body.
  - `pub fn Snapshot::remote(&self, seat: Seat) -> Option<&EntityState>`

- [ ] **Step 1: Write the failing probes** in `remote/probes.rs`. Build worlds the way `drivers/probes.rs` does (`probe_comp_a`/`_b`, `Guests::Companions`).
  - `droid_for_seatless_guest`: party of 1, seat 1 sends a command, so `snapshot.remote(Seat(1)).is_some()` and `snapshot.body(Seat(1)).is_none()`.
  - `no_droid_solo`: one seat, 600 steps, no `Kind::Remote` in any snapshot.
  - `docks_when_companion_frees`: with a droid flying, `probe_comp_a` joins, so the same step `body(Seat(1)) == comp_a` and `remote(Seat(1)).is_none()`.
  - `launches_where_body_was` (Review Focus 2): the driven companion leaves the party at P, so the droid's `at` is within 1 m of P + 1.5 m up.
  - `not_saved`: write a save with a droid, so it holds no `Kind::Remote`. Loading it and stepping once brings the droid back.
  - `minigame_no_droid`: on a course stage, no `Kind::Remote`.
- [ ] **Step 2:** Run `kjk sim probes --only remote --mute`. Expected: every probe FAILs ("no remote" or does not compile).
- [ ] **Step 3: Implement.**
  - `Drone`, `Kind::Remote` with encode and decode tag 4, and `launch`. Spawn at the seat's released body if one was released this step (read `drivers`' released list through a `Launched(Vec<(Seat, Vec3)>)` resource that `assign` fills). Otherwise spawn 1.5 m above and 1 m behind the leader.
  - `Save::world` filters `Kind::Remote`.
  - Add the bump file.
- [ ] **Step 4:** Run `kjk sim probes --only remote --mute`, then `--only drivers`. Expected: all PASS, with drivers unchanged.
- [ ] **Step 5:** Run `cargo test -p sim sim_determinism`. Expected: PASS (it leaves the final hash to the merge because of the bump).
- [ ] **Step 6: Commit:** "The remote droid: a seatless guest's body, derived each step, never saved"

### Task 2: Hover flight, leash, stuck snap and recall

**Files:**
- Modify: `remote.rs`, `remote/probes.rs`, `state.rs` schedule (`remote::fly` after `move_actors`, on every step reason so clients predict it)
- Create: `data/rules/remote.toml` (chapter `remote`, sections `flight` and, later, `kit`)

**Interfaces:**
- Consumes: `Drone`, and the seat's `Command` (`forward`, `right`, `up`, `angles`, `buttons::USE`)
- Produces: `pub(super) fn fly(…)`, and the rules:
  - `remote.flight.speed` (m/s), `accel`, `drift`
  - `remote.flight.floor_min` = 1.0 and `floor_max` = 3.0 (m)
  - `remote.flight.leash` = 20.0 (m)
  - `remote.flight.stuck_ms` = 1000

- [ ] **Step 1: Write the failing probes.**
  - `hovers_between_floor_bounds`: 300 steps with `up = 127`, so the height above the floor under it is ≤ `FLOOR_MAX`. With `up = -127` it is ≥ `FLOOR_MIN`.
  - `stopped_by_walls_and_closed_doors`: fly into a closed door for 200 steps, so the droid stays on its near side and the door's state is unchanged.
  - `passes_creatures`: fly through a standing hostile, so the hostile's position is unchanged.
  - `leashed`: fly away 400 steps, so the distance to the nearest standing party member is ≤ `LEASH` + 1.
  - `snaps_when_stuck` (Review Focus 4): place the droid inside solid geometry, so within `STUCK_MS` it is behind the leader and `level_overlaps` is false.
  - `recall`: `USE`, so it is behind the leader the next step.
- [ ] **Step 2:** Run `kjk sim probes --only remote --mute`. Expected: the new probes FAIL.
- [ ] **Step 3: Implement `fly`.** Trace with `level.with(states, |w| w.trace(from, to, &Extents, Contents::SOLID))`, which includes doors' and placeables' collision. Use zero gravity and a spring toward the clamped height over the floor trace. The leash target is the nearest standing party member.
- [ ] **Step 4:** Run the probes. Expected: PASS.
- [ ] **Step 5: Commit:** "The remote droid flies: hover, leash, stuck snap, recall"

### Task 3: The droid touches nothing in the world

**Files:**
- Modify: `remote/probes.rs` only, unless a probe finds a leak. Any fix goes in the system that leaked, keyed on `Drone`'s absence of components, never on a module name.

- [ ] **Step 1: Write the probes.**
  - `inert_in_triggers_and_doorways`: park a droid inside a trigger with an OnEnter script that sets a global, inside an exit doorway, inside an encounter and on a conversation gather spot. Step 300. Assert:
    - the global is unset
    - no encounter spawned
    - `party::ready` is unchanged
    - the fog grid is unchanged
    - `Snapshot` minus the droid's entry equals the same world stepped with no droid
  - `not_a_pc`: `GetFirstPC`/`GetNextPC` count, `GetIsPC` on every object, and `GetFirstObjectInArea` iteration are all equal with and without the droid.
  - `unseen`: a hostile with the droid 1 m in front for 300 steps raises no OnNotice, and its `minds` target is unchanged.
  - `no_use`: `USE` held next to a container, a door and a talkable NPC opens and talks to nothing. `USE` is recall only.
- [ ] **Step 2:** Run `kjk sim probes --only remote --mute`. Expected: PASS (it should already hold by construction). A FAIL is a leak: fix it at the cited system and re-run.
- [ ] **Step 3: Commit:** "Probes: the remote droid changes nothing a story sees"

### Task 4: The fairness gate and leader credit

**Files:**
- Modify: `remote.rs`, `remote/probes.rs`

**Interfaces:**
- Produces:
  - `pub(super) fn fair(target: &Fighter, party_leader: SimId, view: &FairView) -> Fair`
  - `pub enum Fair { Yes, Protected, NotHostile, NotFighting }`
  - `FairView` carries:
    - the faction table
    - who is in a conversation or scene (`scene::hold`, `hold_talkers`)
    - `min_one_hp`, plot and immortal per target
    - who is in combat with the party (`world::InCombat`)
  - `pub(super) fn credit(world) -> SimId`: seat 0's acting body (`Snapshot::body(Seat(0))` on the authority), even when it is downed.
  - `pub(super) fn dueling(member: SimId, view: &FairView) -> bool`: the member is in combat with a creature for which `fair` returns `Protected`.

- [ ] **Step 1: Write the failing probes.**
  - `fair_cases`: one table over neutral, friendly, hostile-not-fighting, hostile-fighting, Min1HP, plot, immortal and in-conversation. Expected: `NotHostile`, `NotHostile`, `NotFighting`, `Yes`, `Protected`, `Protected`, `Protected`, `Protected`.
  - `credit_follows_switch` (Review Focus 3): after `SwitchPlayerCharacter` to T3, `credit` is T3's id the same step, and so is the leash target. After a portrait lend to Carth, it is Carth.
  - `dueling_member`: the PC fighting a Min1HP opponent, so `dueling(pc)` is true. Fighting a plain hostile, it is false.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement** `fair`, `credit` and `dueling`.
- [ ] **Step 4:** Run them. Expected: PASS.
- [ ] **Step 5: Commit:** "The remote droid's gate: hostile, fighting, unprotected; credit to the leader"

### Task 5: The kit (Shock bolt, Sonic Blast, Stim dart)

**Files:**
- Modify:
  - `remote.rs`, `remote/probes.rs`, `data/rules/remote.toml` (section `kit`)
  - `crates/sim/src/world/bolts.rs` (a droid `Missile` has `by = credit()` and `targets` filtered through `fair`; it hits world or creature only, never a placeable)
  - `crates/sim/src/world/powers.rs` (`throw_target` callable with a strength, not only a power level)
  - `crates/sim/src/world/falls.rs` (`revive` callable with no reviver body: `Reviver::Remote(Seat)`)

**Interfaces:**
- Consumes: `fair`, `credit`, `dueling`, `Drone::ready_ms`
- Produces: `pub(super) fn kit(…)`, a system right after `remote::fly`, authority only. Buttons:
  - `ATTACK` fires the bolt
  - `ALT_ATTACK` fires the Sonic Blast
  - `MEDPAC` fires the dart at the party member under the crosshair (`Command::aim`), else the nearest within range
- Rules, starting values:

| Rule | Value |
|---|---|
| `remote.kit.bolt.share` (‰ of a blaster pistol's shot) | 300 |
| `remote.kit.bolt.cadence_ms` | 600 |
| `remote.kit.sonic.reach` (m) | 6.0 |
| `remote.kit.sonic.cone_deg` | 60 |
| `remote.kit.sonic.strength` (relative to Push level 1) | 0.5 |
| `remote.kit.sonic.knockdown_ms` | 1000 |
| `remote.kit.sonic.knockdown_max_size` (appearance size) | medium |
| `remote.kit.sonic.cooldown_ms` | 8000 |
| `remote.kit.dart.range` (m) | 15.0 |
| `remote.kit.dart.revive_cooldown_ms` | 25000 |
| `remote.kit.dart.heal_cooldown_ms` | 12000 |
| `remote.kit.dart.heal_permille` | 100 |
| `remote.kit.dart.revive_hp_permille` | 250 |

- [ ] **Step 1: Write the failing probes.**
  - `bolt_hits_fair_only`: a fair hostile loses HP. A neutral, a Min1HP duelist and a placeable lose none. The hostile's `GetLastAttacker` is `credit()`.
  - `bolt_kill_gives_xp`: a droid kill raises the party's XP by the same amount as a leader kill of the same creature.
  - `sonic_pushes_fair_only`: hostiles in the cone move back at least 2 m and a medium one is knocked down. A large one only staggers. Nothing behind the cone moves, and neither does the duelist.
  - `dart_revives_and_heals`: a downed companion in range stands with `REVIVE_HP_PERMILLE` of its max HP, and the cooldown is set. A hurt one gains `HEAL_PERMILLE`.
  - `dart_respects_duel`: while `dueling(pc)`, the dart does nothing and the cooldown is not spent.
  - `dart_cannot_beat_the_loss`: everyone down means `Snapshot::lost` is set the same step, and the dart does nothing after.
  - `two_darts_one_body` (Review Focus 5): two droids dart one downed body in one step. One revive happens, the lower seat's, and only that seat's cooldown is spent.
  - `kit_draws_no_rng`: `rolls` draw count is equal with and without the kit firing.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement** `kit` and the three hooks named under Files.
- [ ] **Step 4:** Run `kjk sim probes --only remote,falls,force --mute`. Expected: all PASS, with falls and force unchanged.
- [ ] **Step 5:** Run `cargo xtask rulebook`, and commit the regenerated `docs/RULEBOOK.md` with the code.
- [ ] **Step 6: Commit:** "The remote droid's kit: shock bolt, Sonic Blast, stim dart"

### Task 6: Holds, scenes and hiding

**Files:**
- Modify: `remote.rs` (the kit and flight freeze during holds), `remote/probes.rs`, `snapshot.rs` (`EntityState` of a `Remote` carries a `hidden: bool` in the same tag-4 encoding)

- [ ] **Step 1: Write the failing probes.**
  - `held_in_conversation`: start a conversation. The droid is `hidden`, buttons do nothing, and its position is frozen. After the conversation it is shown again behind the leader.
  - `join_mid_hold` (Review Focus 1): seat 1's first command arrives during a movie hold. No droid appears until `Resume`, then one does.
  - `hidden_in_scene_and_fade`: the same for `scene::hold` and a script fade.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run them. Expected: PASS.
- [ ] **Step 5: Commit:** "The remote droid waits out conversations, scenes, fades and movies"

### Task 7: The client: model, camera, HUD, keys

**Files:**
- Create: `crates/game/src/remote_view.rs`
- Modify:
  - `crates/game/src/sim_view.rs:132` (draw `Kind::Remote` with TSL's remote appearance row, unless `hidden`)
  - `crates/game/src/play_view.rs:829` (`camera_subject`: the seat's droid before the follow camera)
  - `crates/game/src/cast.rs:1282` (`cast::me`)
  - `crates/game/src/hud.rs:466` (with no body but a droid, show three kit icons with cooldown sweeps)
  - `xtask/src/duo.rs` (add `--preset remote`: `end_m01aa`, two sides, seat 1 flies and fires each button once)

- [ ] **Step 1: Look up the remote.** Find its `appearance.2da` row and sounds in the TSL install with `kjk` 2DA tools. Record the row name in `remote_view.rs`, resolved at load by label, not by row number.
- [ ] **Step 2: Implement** the model, the hover bob, the engine-hum loop and the three visuals:
  - bolt: the `blaster_view` bolt in the droid's colour
  - Sonic Blast: the K1 distortion ring plus the sonic grenade sound
  - dart: a small projectile plus `gui_inject`
- [ ] **Step 3:** Run `cargo xtask duo --preset remote`. Expected: the verdict is PASS, and each side's shot shows the droid. Send the shots and a gif to the owner.
- [ ] **Step 4: Commit:** "The remote droid drawn: TSL's remote, its camera and its kit on the HUD"

### Task 8: The duel census sweep

**Files:**
- Create: `crates/launcher/src/remote_census.rs` (verb `kjk remote census k1|tsl`)
- Modify: the verb registry in `crates/launcher/src/verbs.rs`

- [ ] **Step 1: Write the census.** It scans every module's scripts for the duel pattern:
  - a conversation-ended fight or a scripted round: `SetMinOneHP(…, TRUE)`, `k_freeze_equip`, party removal (`RemoveFromParty`, `K_PARTY_STORE*`), or `SetPlotFlag`
  - next to `ChangeToStandardFaction(…, HOSTILE)`
  
  For each opponent found, it plays the module from its chain save (where one exists) to the round's start and asserts `fair` returns `Protected`. It lists, as [verify], the opponents in modules with no save (TSL past Peragus).
- [ ] **Step 2:** Run `kjk remote census k1 --mute`. Expected: every K1 duel in the spec's table is Protected (the Taris ring, Korriban duels, Leviathan Malak, Star Forge Bastila and Malak, Unknown World Bastila). Any that is not is a rule gap: widen `fair` engine-wide (a new protection signal), never by module, and re-run.
- [ ] **Step 3:** Run `kjk remote census tsl --mute`. Expected: Peragus is clean, and the rest is listed as [verify].
- [ ] **Step 4: Commit:** "kjk remote census: every scripted duel's opponent is protected from the droid"

### Task 9: Chaos droids in the chain

**Files:**
- Modify: `crates/launcher/src/chain.rs` and `playthrough.rs` (option `--droids chaos`)

**Interfaces:**
- `--droids chaos` adds seats 1 and 2 as phantom guests that never take a companion (`drivers::assign` skips a seat flagged `phantom`). Each step, each one:
  - flies toward a seeded-random point within the leash
  - presses `ATTACK` always, `ALT_ATTACK` and `MEDPAC` when ready
  - aims at the nearest creature
  
  The seed comes from the chain seed, so a failure replays.
- At each module exit it compares against the plain run's recorded state: plot globals and locals, journal, party membership and module order. XP, levels, HP and timing are excluded.

- [ ] **Step 1: Implement** the option and the comparison.
- [ ] **Step 2:** Run `kjk chain k1 --mute` (plain, to record the state). Expected: 29/29.
- [ ] **Step 3:** Run `kjk chain k1 --droids chaos --mute`. Expected: 29/29 and "story state: equal at every exit". Any difference is a leak: trace it to the system and fix it there, then re-run both.
- [ ] **Step 4: Commit:** "kjk chain --droids chaos: the chain with two droids spamming their kit"

### Task 10: Docs and the PR

**Files:**
- Modify:
  - `docs/COOP.md` (replace "it watches a follow camera" with the droid, and link the spec)
  - `docs/PARTY.md:68`
  - `ROADMAP.md` (the droid item moves to done when merged)
  - `KNOWN-ISSUES.md` (the TSL [verify] rows)

- [ ] **Step 1:** Write the docs in the files' own terse style.
- [ ] **Step 2:** Run `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `kjk sim probes --mute` (all) and `kjk chain k1 --mute`. Expected: all green, 29/29.
- [ ] **Step 3:** Push `claude/remote-droid` and open the PR. The title says what a player gets. The description covers:
  - the five safety rules
  - the probes
  - the census
  - the chaos-chain result
  - "KJKS +1"
- [ ] **Step 4:** Bind the PR with the ccd_pr tools and offer Auto-fix.
