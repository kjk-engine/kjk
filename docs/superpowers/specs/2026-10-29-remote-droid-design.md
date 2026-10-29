# The remote droid: a seat for every guest (design, 2026-10-29)

Status: approved by the owner 2026-10-29; built on claude/remote-droid (rulings in its plan ledger: no `Kind::Remote`, the droid is a world resource).

## Why

In co-op, a guest who has no companion to drive (because the party is too
small, or the story took the companions away) watches a follow camera today
(`docs/COOP.md`, "none free, it watches"). That is a lot of the game: the
Endar Spire before Trask, all of Taris with only Carth, Naga Sadow's tomb,
the Hrakert seafloor, the Leviathan rescue, the Star Forge, and TSL's
Peragus, Telos academy, Malachor and Trayus. ROADMAP.md's Next section has
the owner's fix: the guest flies a remote droid like Bao-Dur's remote, with
a small kit that helps the party.

**Goals.** It is fun to fly, it helps a little, and it can never break the
story. It works in every module, fight and scene of both games without a
per-module patch (`holistic-engine-semantics`).

**Non-goals.** The droid is not a companion. It has no inventory, levels,
feats, XP, dialogue, influence or save state. It is not a new Force power
for the party, and it does not change solo play.

**Anti-goals** (any of these makes it a failure, however well it works):
- The droid opens a door, starts a conversation, trips a trigger, mine,
  trap or encounter, or changes who a script thinks the PC is.
- The droid wins or spoils a scripted duel, or kills a plot creature.
- The droid makes a neutral NPC hostile, or starts a fight.
- A droid shows up in a cutscene shot or a conversation camera.
- Solo runs, the K1/TSL chains, or the pinned sim hash change behaviour.
- A save carries a droid, or loading one needs a droid.

## How it plays (my proposal)

- **When.** A guest seat whose `drivers::assign` finds no free party member
  gets a droid instead of the follow camera. When a companion frees up (one
  joins, or the story gives them back), the guest is moved onto it at once,
  as today, and the droid docks (a short fade). When the companion leaves
  or is benched, the droid launches from that body. *(Owner, 2026-10-29:
  yes, at once.)*
- **Flying.** The droid hovers 1 to 3 m above the floor (W/A/S/D, mouse
  look, Space up, crouch down; the existing `Command::up`). It is quick,
  about run speed, and drifts a little on stopping. It is solid against
  world geometry and closed doors and force fields. It passes through
  creatures, which neither see nor block it.
- **Leash.** It stays within 20 m of the nearest standing party member
  (rulebook number). Past that it is pulled back gently. If it gets stuck,
  falls out of the level, or the party changes module, it snaps back to
  hover just behind the party leader's shoulder. There is also a recall key.
  *My call:* it moves by world traces with the player's solids (level,
  doors, force fields, placeables), not over the nav mesh, so it can fly
  over railings and gaps but not through anything a player can't.
- **Can't be hurt.** Nothing targets it. It has no HP. Enemies don't
  perceive it, so it never alerts anyone or breaks stealth by being seen.
- **Hidden while the world holds.** During conversations, movies, holds,
  fades, minigames and cutscene cameras it is not drawn and its kit is
  off. The guest's camera follows the party leader meanwhile, as now.

### The kit (three buttons, all numbers in `data/rules/remote.toml`)

| Button | Name | What it does | Starting numbers |
|---|---|---|---|
| Primary | **Shock bolt** | A small blaster bolt (the existing `Missile`, a low `share`) | about 2 to 4 damage, 0.6 s between shots |
| Alt | **Sonic Blast** | A short cone of sound. Hostiles in it are shoved back a few metres and stagger. Small ones are knocked down. It reuses Push's throw (`powers::throw_target`) at low strength, the sonic grenade's sound, and the K1 distortion ring for the visual. | 6 m cone, 8 s cooldown, about 1 s knockdown, no damage |
| Third | **Stim dart** | A dart fired at a party member. On a downed one it revives them (the hand-held revive from FALLING.md, but at range and instant). On a hurt one it heals a little (medpac sound `gui_inject`). | 25 s cooldown (revive) or 12 s (heal), heal about 10% of max HP |

*Owner, 2026-10-29:* the loss is unchanged. With the whole party down the
game is lost at once, droid or not, so in a solo dungeon the dart cannot
save the PC. The droid's kills give the party XP as any kill does.

The droid's model and sounds are TSL's remote (K2 is already required).

## Safety: how it can never touch the story

### Rule 1: the droid is not a body the world knows

The engine's story paths all key on one of three components: `Avatar(seat)`
(a PC), `Player(MotionState)` (a body scripts can see) and `Fighting` (a
combatant). The droid is a new component, `Drone { seat, pose, velocity,
cooldowns }`, and carries none of them, nor `Motion` (the arena wanderers'
integrator). With no filter added anywhere, every one of these skips it:

- the Use key, doors run into, bump-opens, containers, locks and doorway or
  trigger tenancy (`world/interaction.rs:53`)
- the ready check (`world/party.rs:444`). A seat with no body already
  counts as ready.
- scripts' things, perception and the PC list (`world/scripts.rs:428-599`):
  `GetFirstPC`, `GetIsPC`, `GetFirstObjectInArea`, `GetEnteringObject`
- trigger OnEnter and OnExit, and traps (`scripts.rs:1563`, `:1605`)
- OnNotice and stealth (`world/scripts/perceiving.rs`, `covert.rs`)
- encounters (`spawners.rs`), trap detection (`traps.rs:282`) and fog
  (`charting.rs`). A droid reveals no map.
- body blocking, hits, pushes, heals and targeting: `movement.rs:137`,
  `harm.rs`, `bolts.rs`, `strikes.rs`, `powers.rs` and `minds.rs:253`
- conversation gather spots (`coop_talk.rs:108`)

One test pins this: a droid sitting inside every trigger, doorway and
encounter of a module changes nothing in a step.

### Rule 2: the kit touches only fair targets

Every effect goes through one gate, `remote::fair(target)`:
- **Shock bolt and Sonic Blast** act only on creatures that are hostile to
  the party, alive, and in combat with it. Hostile means
  `GetIsEnemy(target, party leader)`. Neutral and friendly creatures are
  never hit, so the droid can never start a fight.
- They also never act on a creature that a script protects: one with
  Min1HP (`SetMinOneHP`), plot or immortal flags, or anyone inside a
  conversation or scene. Scripted duels (the Taris and Korriban rings,
  Bastila and Malak on the Leviathan and the Star Forge, Mandalore's
  circle, the Handmaiden's sparring, Sion and Kreia) end through Min1HP and
  conversation rounds. So the droid can't hurry or skip them, and doesn't
  need a list of them. *[verify: the duel census below proves this rule
  covers each duel.]*
- When the only hostiles nearby are protected, the kit greys out with a
  one-line notice ("The remote holds back"), so it doesn't feel broken.
- **Stim dart** acts only on party members (`Snapshot` party), and never
  inside a hold. *My call:* it also does nothing to a party member who is
  fighting a protected creature (a duel), so a duel is won by its fighter
  alone.
- *My call:* the droid follows and is leashed to the nearest standing party
  member, and credit goes to seat 0's acting body even if that body is
  downed. With the whole party down (the loss) there is no droid.

### Rule 3: what the kit does is the PC's, as scripts see it

A hit needs an attacker for `GetLastAttacker`, `GetLastDamager`,
`OnDamaged` and kill XP. The droid isn't a script object, so it can't be
the attacker. Its hits and kills are credited to the party leader (seat 0's
acting body). That is what KOTOR's own scripts expect: the AI turns on a
PC, XP goes to the party as for any kill, and no script sees an invalid
object. A Sonic Blast that knocks a creature down raises the same events
as Push.

### Rule 4: the droid is derived, never stored

The droid is not saved. Each authority step rebuilds it from the seat
state, in the system that runs after `drivers::assign`, so loading a save,
reloading, installing a module or a guest rejoining all just recreate it
by the leader. It goes over the wire as a new snapshot kind
(`Kind::Remote(seat)`, tag 4) so clients draw and predict it. That is one
snapshot version bump through `bumps/` at merge. Saves don't change and
stay on the same KJKV.

Solo play and the pinned run (`determinism.rs`, four seats all driving)
never have a seat without a body. They make no droid and draw no extra
RNG, so the sim hash moves only by the version number.

### Rule 5: it draws no randomness

Bolt spread, knockdown and heal use fixed numbers, not `rolls`. That way
a droid can't shift the story's dice for anything that follows.

## Everywhere the party drops below three

This is the census from today's survey (routes, walkthroughs, the games'
scripts). Each kind of place, and what the droid does there:

| Kind of place | Examples | Droid |
|---|---|---|
| Party of 1 or 2 in normal play | Endar Spire start (the first join, before Trask), Taris with Carth, Kashyyyk before Jolee, TSL prologue (T3 alone), Telos with Atton | flies, full kit |
| Solo dungeons | Naga Sadow's tomb, Hrakert seafloor, Peragus admin and tunnels, T3's Peragus run, Malachor, Trayus | flies, full kit. Leashed to the one body. Underwater it's cosmetic. |
| `SwitchPlayerCharacter` segments | the Leviathan rescuer, T3 in Nar Shaddaa, Atton's solo, Mission/T3 on Taris | flies around the switched body (it's the party leader then) |
| Scripted duels | Taris and Korriban rings, Leviathan and Star Forge (Bastila, Malak), Dxun circle, Telos sparring, Trayus | flies, kit held back by Rule 2 |
| Captured, gear taken away | Leviathan cells, TSF jail, Atris's academy, G0-T0's yacht, Jorak Uln's gas | flies. No hostiles in combat means nothing to shoot. |
| Floor puzzles | Unknown World tiles, rings puzzle | safe: the droid trips no floor triggers (Rule 1) |
| Conversations, movies, holds | everywhere | hidden, kit off |
| Minigames | swoop races, Ebon Hawk turrets | no droid. Each seat already has its own run or gun. |

A gap: TSL past Peragus has no chain routes yet, so its rows come from the
games' scripts and are [verify] until those routes exist.

## How we prove it's safe

1. **Unit and probe tests** (`kjk sim probes --only remote`):
   - a droid parked in every trigger, doorway, encounter and gather spot
     changes nothing
   - the `fair` gate covers neutral, friendly, Min1HP, plot, immortal and
     in-conversation targets
   - credit goes to the leader
   - the droid is rebuilt after a load, a reload and an install
   - the leash and the stuck snap work
2. **The duel census, a sweep invariant.** Scan every module's scripts in
   both games for the duel pattern (a lone fight ended by a conversation or
   a round: `SetMinOneHP`, `k_freeze_equip`, party removal). Assert each
   opponent is protected when its round starts. A duel the rule misses
   fails the sweep, so a new class is caught, not patched by module.
3. **Chaos droids in the chain**: `kjk chain k1 --droids chaos`. The run
   adds two phantom guest seats that never take a companion. Their droids
   fly at random around the leader and spam all three buttons at whatever
   is in range. Pass means:
   - the chain is still 29/29
   - at every module exit, the story state equals a plain run's: plot
     globals and locals, the journal, party membership and the module
     order. *My call:* XP, levels, HP, timings and dice order may differ,
     because the droids kill things sooner by design.
   This is the main proof that "it works everywhere". It runs on demand
   and before merge, not daily.
4. **A duo playtest preset**, `cargo xtask duo --preset remote`: the
   Endar Spire start with two machines, one of them flying.

## Build order

1. **The droid body.** `Drone` component, `Kind::Remote`, spawn and dock in
   step with `drivers::assign`, hover movement with world traces, leash,
   recall and stuck snap. Client: the TSL remote model and a fly camera
   over it. Probe for "changes nothing".
2. **The kit.** The `fair` gate, then Shock bolt, Sonic Blast and Stim
   dart, with leader credit. Add `data/rules/remote.toml`, plus the HUD:
   three icons with cooldowns.
3. **Holds and hiding**: conversations, movies, minigames, fades.
4. **Safety proof**: the duel census sweep and the chaos-droid chain mode.
5. **Playtest and tune** the numbers with the owner. Update docs/COOP.md
   and PARTY.md.

One branch, one PR. About three to five working sessions.

## Left to the builder

- The exact hover height, speeds and drift, and all the rulebook numbers.
- The droid's light and engine sound loop, and the visual for each button
  (picked from `visualeffects.2da` rows and the sonic grenade's).
- Key bindings, following the existing scheme.
