# Combat in the sim (`sim::combat`, `kjk combat`)

The numbers the sim plays by are the rulebook's (`docs/RULEBOOK.md`, from
`data/rules/`); the ones quoted below are as they stood when written, and
where this page and the book differ, the book is what the game plays. `kjk rules k1` puts it beside the game's
own weapons, items and powers.

Every number below is the rulebook's ([`RULEBOOK.md`](RULEBOOK.md),
`data/rules/`).

## Characters
- Players (`Kind::Avatar`) and creatures (`Kind::Object(Creature, _)`) are
  characters: a `MotionState` (JA's movement step moves both) and a `Fighter`
  (centi-HP and the second bar, FP or stamina; life, weapon timer, pain and combat timers, target,
  last attacker, a ring of eight `Event`s by sequence number (who, the
  outcome, the location, the damage, the weapon that struck or was turned,
  and where a block happened), the saber's
  and the Force's state, the blades' pose and trails, the blade hits held
  for its grace). Snapshots (`KJKS` v54) carry both, each record one
  width whatever it holds (absent values blank behind a flag) so the
  network's XOR deltas stay small; players also carry their `Sheet`. A
  creature stands up at its GIT spot when its module installs.
- A party member at 0 HP is downed, not dead: getting up, reviving and the
  loss are `docs/FALLING.md`.
- `Sheet`: KOTOR's numbers (abilities, BAB, saves, max HP and FP, defence,
  feats and powers as row sets, faction, plot/min1hp/droid flags, two
  `Weapon`s with their proficiency, focus and specialization feats; the
  armour's DEX cap). `session::sheets` reads a creature's from its UTC,
  UTIs and `classes`/`cls_atk_*`/`cls_st_*`/`acbonus`/`baseitems`/
  `iprp_damagecost`; players are `Sheet::dev_soldier` until the party
  table (I19).
- Factions: `repute.2da` as a square matrix (`module::Repute`; the table
  has no player column, so it is taken from the player's row); row `a`,
  column `b` is how `a` regards `b`, 10 or less hostile. `kjk combat
  roster k1 end_m01aa` lists what was read. Scripts' `AdjustReputation`
  moves a pair both ways for the rest of the game (`module::Adjustments`
  in the scripts' state, saved; `Repute::with` gives the standing every
  check reads): Tatooine's Sand People at peace with one in their robes.
- Bodies block (`collision::Bodies`: level first, then bodies by id, the
  nearest hit wins), but with friendly fire off (the default; a level's
  `with_friendly_fire` sets a session's) party members pass through each
  other and each other's shots and blades.

## Passive feats (`combat::passive`)
What a feat a character simply has does, by `feat.2da`'s rows K1 and TSL
share; a line's tier is how many of its rows a sheet holds (each needs the
one before). Every K1 row's rule and its source: the feats journal
(2026-10-04). `kjk combat feats k1` shows them on the party's templates;
the approved test `feats` checks them.

- **Equipping** (`passive::may_equip`, `State::wear`): an item needs every
  feat its base item names (`baseitems.2da` `reqfeat0` to `reqfeat4`:
  weapon and armour proficiencies, implant levels, Jedi Defense for K1's
  robes); Proficiency All (93) stands for the proficiencies. A player's
  equip without them is refused with the game's line (talk table 1475,
  "You do not have the proficiencies required to equip that item.").
  Scripts' equips are not checked (reone checks none). A weapon a template
  holds without its proficiency keeps −4 to hit.
- **Two weapons** (`passive::hand_attack`, through `in_hand`): a weapon in
  each hand, or a double-bladed one (its `baseitems.2da` label), −6 to
  the main hand and −10 to the off hand; Two-Weapon Fighting, Improved,
  Master −6/−6, −4/−4, −2/−2; a small weapon (`weaponsize` 2 or less) in
  the off hand 2 off the main hand's (the game's "Balanced"). reone has
  −6/−10 and no feats. A double-bladed weapon's blows are the main hand's.
- **Two pistols** (`combat::pair`; KOTOR's two pistols, reone
  `DualPistols`, wield 6): a gun in each hand fires both, right then
  left, each on its own timer at its own refire and neither within the
  pair's share of a refire after the other (`weapons.pistol.pair_gap`),
  so the pair fires twice as often as one gun.
  Each shot is its hand's: its weapon, its penalty above, its muzzle (the
  left's the right's mirrored, `hand_muzzle`). The hand keys take
  neither down; the hands of the last shot ride in `Shooting::hands`.
- **Dueling**, Improved, Master: +1, +2, +3 to hit and to defence with one
  one-handed weapon (`weaponwield` 1, 2, 4) and nothing in the left hand,
  melee or ranged (beside the saber clash offence, C8).
- **Saves**: Conditioning, Improved, Master +1, +2, +3 to every save
  (`Sheet::save`, which scripts' saves, powers, grenades and the character
  sheet read); Uncanny Dodge 1, 2 +2, +4 against grenades.
- **Skills** (`Sheet::skill`, where the engine checks one: Security on
  locks, Treat Injury and Repair healing and reviving, Awareness's
  reaction): Caution (Demolitions, Stealth), Empathy (Persuade,
  Awareness, Treat Injury), Gear Head (Repair, Security, Computer Use)
  +1 to +3, Perceptive +2 (Stealth, Awareness), Skill Focus +2; none for a
  skill without ranks. `GetSkillRank` stays the ranks (reone's).
- **Damage reduction** (`passive::toughness`, a KOTOR attack's whole
  points, weapons and blasts): Improved Toughness K1 2 off physical
  damage, TSL a tenth over 20; Wookiee Toughness 2, TSL's II 5 and a
  twentieth, III 8 and a tenth. The level's game (`Level::game`) picks.
- **HP and FP**: levels (`WORLD-STATE.md`, "Levels").
- Class defence feats (Scoundrel's Luck, Jedi Sense, the droids' Logic
  Upgrades) are `acbonus.2da`'s columns, already the class defence.

## A step
`think` (creature minds, authority only: nearest visible hostile, aim with
a hashed wander, fire while on target, or with a saber or melee weapon
walk up and swing, with fists walk closer and punch; nothing new while in pain, stunned or confused) →
`force` (below) → `move_actors` (players by seat, creatures by id; each
move runs the weapon step: changing hands, knockdowns, saber moves, and a
blaster's torso held up ready, JA's `WeaponReadyAnim`, once its refire is
out and nothing holds the torso; fists' and, unlike JA's, a stun baton's
torso follows the legs once a punch has played) →
`fire` (JA's refire, `weapons::count_down` and `trigger`, the alternate
button a feat special, `specials::pull`; each shot starts its attack
animation on the torso over what plays, starting it over when it already
plays or when the step began in it (the move may have set the ready pose
meanwhile), as JA's torso animation start does, so every shot kicks;
bolts spawn, the disruptor is a hitscan; a player's (or a body a seat
drives) go to what its crosshair line meets (`mind::crosshair`: the line
through the middle of its screen, along the view: 16 units over the eye
for JA's third-person camera, moved by its command's `aim` to where its
camera stands, its own eyes in first person (`mind::aim_for`,
`sim_host::CameraAim`); the player state keeps it, so every client draws
the shot along the same line; a lit blade too, and a thrown saber
steers by it), from the muzzle or the eye, a creature's along its view;
spread from a hash of tick, shooter and shard; under prediction only the players
fire) → `sabers` (below) → `strikes` (beasts' attacks and fists, below;
under prediction only the players') → `fly`
(bolts traced every tick against level,
bodies and saber boxes, the owner skipped; one that starts inside
something is traced again where it stands; a lightsaber may turn it) →
`hurt` (authority and replay only) → `interact` → the placeholder's
systems.

## A hit (`combat::damage`: `assess`, then `apply`)
The size: a weapon's dice + bonus dice + item bonus (+STR melee, +2 with
its specialization feat) × the margin `p` = (21 + AB − defence) / 20 in
[5%, 95%] (C1; AB: BAB, DEX or STR, item, +1 focus, −4 without the
proficiency, the hands' two-weapon penalty or Dueling's bonus; defence:
10 + DEX up to the armour's cap + armour, natural and class, + Dueling's:
"Passive feats" below) × pace 4 × cadence / 3 s (C2) × `crithitmult` on a d20 in the
threat range (C4), plus sneak attack d6s not multiplied (the target's
defence less a feat's round penalty), then the target's guards by damage
type, before the margin and pace (step 9: `Sheet::guards` from every
equipped item, immunity and vulnerability in percent adding up, the best
resistance in points per KOTOR attack; a weapon of several types hurts as
the least guarded; a bonus against the target's race, `Weapon::versus`,
as its own type; then Improved and Wookiee Toughness's damage reduction;
all of it guarded deals nothing); a power's JA points as % of the target's maximum → ×
JA's location (box method, `combat::location`) → × the shard's share →
the knockback from that (5 units/s per JA-equivalent point, with the
knockback timer; none when the share refuses it, QUIRKS Q-0031) → the share (C17:
hostile yes, one side (the party, or friendly factions: repute 90 or
more) by friendly fire, plot never, a neutral outsider not by the party's
attacks nor by any blast or area, but by anyone else's aimed ones (the
outsider its attacker's target; a swing or bolt meant for another passes
harmless, as KOTOR's attack lands on its target alone):
KOTOR's areas take the caster's enemies, Tatooine's 8-HP banthas live
through the Sand People's grenades and gaffi sticks [verify]) → at least one hundredth when it harms →
then, in `hurt`: Knight and Master Speed's extra attacks (× 2 or 3 on a
weapon hit) → an energy shield (by the hit's damage type) → a battle
stim's vitality → Force protect (JA's 40/60/80% of the hit up to its cap,
paid from FP; the rest through, Q-0033) → rage (÷ level + 1, and it can
kill, Q-0044) → Min1HP floor. Integer hundredths, saturating;
dice from the `authority` stream. A death picks JA's death animation
(`combat::death`) and move_body goes limp; a creature that lives may flinch
(JA's NPC pain odds at medium skill; always to a blade, a saber's as JA's,
a sword's or vibroblade's too: the owner's balance of 2026-10-15), doing
nothing new until it has played.

A death KOTOR stages is KOTOR's own instead (`death::in_place`): one in
a scene (`scene::Held::scene`: a conversation holds the world, or the
blow's attacker or target takes part in one; a power's or a grenade's
kill too), or to a story's blow (`Source::Script`'s `staged`: a script's
own `EffectDamage`, not a power's or a grenade's; a scene's melee blow; a
scripted shot; `EffectDeath`). The body plays its dying clip where it
stands (`death::DIE`, `animations.2da` `die`), unpushed, and lies as the
clip ends, so a scene's camera films it there (end_room3's camera 22 on
end_soldier2). A blow KOTOR stages pushes no one and makes no JA flinch
(a scene's blow plays KOTOR's reaction, `blows::HURT`), and no saber lock
or kick takes a fighter in a scene. The sim's own damage in a script's
shape (a poison's doses, Force stun's sting, a saber lock's loss) is free
play's: JA's death, pushed, the lock loser's shove kept.

`kjk combat worked` recomputes I22 section 11's worked Endar Spire fights
through this pipeline from the modules' own creatures: every hit's mean
within 15% of the design's number (5% allowed for the crits it leaves
out), and the hits to kill.

## Beasts (`world::strikes`)
A creature whose model has no humanoid skeleton (`appearance.2da`
`modeltype` S or L: beasts, droids, aliens with bones of their own) cannot
swing JA's moves; with nothing in its hands it attacks KOTOR's way
(`module::Strike`, read by `session::sheets`; in the module digest). Its
mind's attack begins one when its target stands within its reach (its
`perspace`, its `hitdist` past that, to the target's body): an attack a
round (`ROUND_MS`; Speed's extra attacks scale the hit as above), its
weapon timer counting it down in move_body as fists' does. Its blow lands at
its model's hit moment (`m0a1`'s `hit` event, else `g0a1`'s, a supermodel's
too; half the animation without one: the rancor's 733 ms; half the round
without either) if the target is still in reach: a hit of its first
creature weapon (`CWeapon_R`, `_L`, `_B`: claws, bite, gore; bare,
`Weapon::UNARMED`, without) through `assess` and `apply`, its point the
target's chest on its side. Both sides are in combat from then (rounds,
`ScriptEndRound`), the target's `last_attacker` raises its `OnAttacked`. A
flinch or a knockdown (the timer cleared), the stunned and the hurting
lose the blow. Such a creature holding a weapon (a Gamorrean's axe) fights
as a humanoid does. K1's katarns, kath hounds and Gamorreans strike 0.5 s
into their 1.5 s attacks. Nothing draws the attack yet: beasts play their
own standing, walking and running clips.

## Fists (`world::strikes`)
A humanoid with nothing in its hands (`Weapon::UNARMED`, 1d3 bludgeoning;
not a beast) punches as JA's melee weapon does: `BOTH_MELEE1` and
`BOTH_MELEE2` in turn on the torso (by `SaberState::chain`'s parity; a
change of weapons starts again with the first), held their length, so
every view plays the punch from the snapshot. Otherwise it is a beast's
attack: a round's cadence, the timer counted down in move_body's plain step;
its reach the body's personal space (a creature's `perspace`; a player's
0.35 m, every K1 humanoid's) and 0.5 m past it, to the other's body, about
as far as JA's punch (20 units out, 8 on, a 6-unit box); its blow halfway
through the punch (the animation table's length; half a round without it).
A player's attack throws a punch whoever stands there, as JA's does; a
mind's only with its target in reach, its mind walking in to three
quarters of that (`mind::Attack::Blow`). The blow lands on the foe in
front (45° either side of the view) within reach: the target if it is
one, else the nearest; with friendly fire off the party's fists pass
through the party, as its shots do. A grip, a hand held out, a change of
weapons or a knockdown keeps a punch from starting, as it keeps a shot. It
runs in prediction for the players' own punches.

## Bashing (`world::objects`)
Locked doors and containers break under the party's own blows, JA style
(the owner's: no Bash in the target block). What may be bashed
(`module::bashable`): a door that is not scenery or a placeable, locked,
not `Plot`, with hit points left (its `CurrentHP` less what it lost,
`ObjectState::damage`). A saber's sweep meets its collision (each segment
through it a contact of the swing) or, a placeable's with no brushes (a
locker), its pick volume; a fist or a stun baton (TSL's plasma torch
too, which punches as fists do, a blow of the baton) its pick volume, a shot or
bolt its brushes or, a player's own bolt, a placeable's pick volume too
(a footlocker has no brushes: TSL's prologue's is shot open; companions'
bolts fly past to their foes), any grenade's blast its middle (KOTOR's grenade script
asks for creatures alone). A blow is worth what it would be against a
body that does not dodge (KOTOR's 95%) resisting its `Hardness` on every
damage type, then `BASH_TIMES` (7) over: the owner's pace, a door in 10 to
15 s of swinging (Taris's Low Security Door, 60 HP, Hardness 5: a saber
about 10 s). A weapon with TSL's Door Cutting (`itempropdef.2da` row 64,
the plasma torch) takes all its hit points at a blow, whatever its
hardness (Peragus's med bay door, Hardness 100, whose `a_plasmachk` asks
for the property) [verify the executable's own rule]. `Min1HP` holds at
1. At none it breaks: unlocked, a shut door opened away from the blow's
maker and never closed again, `OnMeleeAttacked` (`GetLastAttacker`,
`GetLastWeaponUsed`) on every weapon's blow, one its hardness turns
whole too, `OnDamaged` (`GetLastDamager`) on every blow that tells, and
`OnDeath` (`GetLastKiller`) on the last; a script's damage hurts any
object but a `Plot` one. Others' blows
do nothing, unless a script asks a creature to bash
(`DoDoorAction(…, DOOR_ACTION_BASH)`, `k_ai_master`'s answer to a door in
its way): it walks there and strikes a KOTOR attack of what it has in hand
a round, swinging. A broken door plays its model's `busted` where its
`genericdoors.2da` row's `StaticAnim` names it (the executable's rule; no
stock row does), else stands open. The target block shows a door's
vitality once blows have told on it. Approved test `bash_doors`.

## Lag compensation (`world::history`; I25, C18)
A seat's view lag rides in `TickInput` (recorded). The authority keeps 32
ticks of every character (from its own steps, never before its first):
its body (`remember`, the step's last system) and its fence (the blades
step's end: both blades' place with the place a tick before, whether they
stop others', and its guard against blades but for the arc: its defence
level and where it stood and looked, if it could block). One world per
attacker, at most 400 ms back (`sim::REWIND_MS`, 64 ticks kept; a
player's view lag is about its round trip and 80 ms more, so this covers
players to about 300 ms round trip): a player's hitscan meets the others as it
saw them, a bolt starts as far on as the lag (its way there traced the
same), and its aimed powers (grip, level-1 push and pull, trick, aimed
crowd control, the lightning and drain lines) find their target there;
its blades' sweeps meet the others' blades (clashes), guards (C6's state
block, its arc taken from where the guard stood then) and bodies as they
were at its view tick, the tree, locks and damage then running on both as
they are now. A tick the history has no fence for (the one a world
started from) is met as it stands. Creatures are never rewound.
`kjk combat lag k1`: compensated hitscans and lightning strikes land at
every lag, uncompensated ones fall away (150 ms: lightning 10/10 against
2/10).

**Defender grace** (`saber::grace`; C19). A blade's hit on a remote
player (its seat's view lags) who could block it (a lit blade, a defence
level) is held for g = min(view lag, 100 ms) in ticks, in the fighter's
`grace` record (four held at most, in the snapshot; a fifth lands the
oldest at once). Each tick of the hold, if that seat's command was made
seeing the world before the hit (the tick less its view lag before the
hit's tick) and leaves it blocking the blow by state (the sweep's own
test, now, at the blow's point), the chance to miss is drawn once (as
the sweep's, below): a miss becomes the sweep's state block (a Parried
event, the lock roll and the clash tree's parry, no damage), a hit lands.
Otherwise it lands when the hold ends, as the striker's weapon in the
hand and style it struck with. Blade sweeps that can be blocked are held
(a special's cannot), and **bolts**: a blockable bolt meeting the body of
a remote player who could turn it (a lit lightsaber, a bolt level of 1 or
more) stops there (`Missile::held`, in the snapshot) for the same g; a
late command blocking it by state turns it (`sabers::bolt_block`), else
it lands when the hold ends. Blasts, thrown sabers, kicks and powers land
at once. Players on the host (view lag 0, solo and lockstep) hold
nothing. `kjk combat duel-lag`'s bolt grace: a rifle at a defence-2 remote
player looking aside 400 ms, then at it, 20 s: at 80 and 150 ms 28 bolts
held, 7 turned by its late facing, 21 hits against 28; none held at 0 ms
or without the lag sent.

`kjk combat duel-lag` measures both (I25 measures): two players on the
duel ring, seat 0 remote at 0 to 350 ms, its client predicting its
swing from the snapshot it has. A swing *shows contact* when its predicted
blade cuts the other's body as the client saw it, by the authority's own
test (`sim::seen_cuts`, what the contact sparks show); the **disagreement rate** is the share of such
swings the authority neither landed nor turned (blocked or clashed),
or never began: a swing the client predicted that the authority's
verdict on it (a parry, a hit) kept from starting, which grows with the
lag and which no rewind helps (C20 predicts no verdict); compensation is
checked on the swings the authority began. Four seeds of 30 s each way:
compensated, 0%, 2.1%, 2%, 5.9%, 3.6%, 12% and 9.5% at 0, 80, 150, 200,
250, 300 and 350 ms; past 80 ms every rejected swing but one (at 300)
was never begun (the begun swings: 0–2% at every lag); uncompensated (JA's
way), 0%, 14.6%, 24.6%, 46.3%, 28.1%, 53.8% and 50%. With the old 200 ms
bound the compensated rate rose to 15.9%, 25% and 31% past 200 ms. The
cost is the defender's: a lagging attacker's blade meets it where it
stood when the attacker saw it, as far as 94 units from where it stands
(350 ms; 44 at 150). Grace held 40 and 42 blows at 80 and 150 ms and
turned 10 and 8 of them into blocks; the remote player took 30 and 34
hits, against 53 and 62 without compensation. The rewinds cost a few µs a tick in end_m01aa (26 sword
users and a lagging player swinging among them, 150 ms: 2–6 µs).

## Minds (`combat::ai`, the step in `world::minds`; I27)
A creature observes on its look ticks (every 200 ms): where the games'
scripts run, the one its script attacks (`ActionAttack`, however far) or
the one it fights while the fight lasts (within `ranges.2da`'s
PartyCombat); on a stage without scripts, the nearest visible hostile
within its `PerceptionRange` sight. Hostile is `module::relation`'s. Its
task: **Engage** (gunners stand and fire
as JA's troopers do (`combat::troop`), never
backing off from a blade, as KOTOR's: shots spaced by weapon, their spread
closing as the aim settles, a spot that sees the target sought only when
it goes out of sight; saber and melee fighters route to reach and
swing, a blade wielder out of a scene paced as JA's Jedi minds are
(`combat::pace`): it decides every 100 ms at its
own moment and holds its keys between, turns at 330°/s, keeps its
distance from its blade's tip by an aggression that rises every 2 to 5 s
and falls when it is blocked or hit (advancing, holding, backing off),
circles at a walk now and then, holds each way it moves a while before
turning back, parries when its enemy swings, and swings within striking
range, its swings chained from where the last ended (move_body, creatures
only) rather than by its keys;
Force users cast their scripts' talents (`k_ai_master` chooses every
power, item, grenade and feat), on a stage without scripts what fits,
`ai::choose_power`; two creatures outside the party whose scripts attack
fight in KOTOR's combat rounds (`minds::begin_round`, the hit model every
weapon hit shares: `damage::odds`, `damage::critical`,
`damage::weapon_damage`); a gunner beyond its
weapon's reach, KOTOR's `maxattackrange`, routes in first; a melee
fighter closing in bears off to a flank by its id; a foe newly spotted
is acted on after 1.5 s less 150 ms a rank of Awareness, meanwhile
sneak-attackable, a hit waking it at once), **Chase**
where it last
saw its target (8 s), **Follow** its leader (a companion,
`Mind::companion`, joining any fight on one of the party and never more
than 1024 units away), or **Idle**. It routes with a `sim::walk::Walker`
and opens doors it runs into. The mind's state rides in the snapshot
(`Fighter::mind`); its dice are hashes of the time and its id. A fallen
companion is knocked out, as KOTOR's are: downed, it gets up at 1 HP
once the party's fight is over (`world::falls`, `FALLING.md`).
`kjk combat ai k1 end_m01aa --arrive wp_end_weld01_02 --hostile end_sith2
--hostile end_sith3 --companion end_trask --companion carth` logs every
decision (`decisions.tsv`).

### Rounds between creatures (`combat::rounds`)
A creature's melee round on another outside the party (its script's
attack, an assured blow, any creature's fight while a scene holds the
world) plays K1's executable's clips, read off its rules and the game's
`combatanimations.2da` (journal `2026-10-24-gang-war/EXE.md`): blade
against blade is choreographed, the attacker's class's cinematic set
`c<class>a1` to `c<class>a5`; any other pairing plain, `g<class>a1` or
`g<class>a2` (a stun baton always so, K1 having no `c1a*`; bare hands
`g8a*`); a blade against a creature model `m<class>a*`. A creature model
attacker strikes by its own clips (`world::strikes`), not in these rounds.
A feat attack swings its feat's clip (Power Attack: a baton's `g1a1`, a
blade's `f<class>a3`). The target answers with the table's cell for the
swing's row and its own class: a hit's damage clip; a miss's parry for a
target holding a blade or a baton, else its dodge [verify the
executable's roll between them]; a `CutsceneAttack` the one its result
names. The blow lands at the swing's `hit1` (a gun's half a round in).
Each fighter attacks once a round, a duel's two half a round apart
(`State::swung`), however often its script asks. Classes are
`Sheet::wield_number`'s: 1 a baton, 2 a blade, 3 a staff or double blade,
5 a pistol, 7 a rifle, 9 a heavy weapon, 8 bare hands; with a weapon in
each hand by the left one's, 4 a blade, 6 a pistol, else 0. While in
such a round a fighter presses none of JA's attacks, closing in
included. Not drawn: the executable's reroll of a choreographed swing
repeating either of the last two, and its rule that a plain round's
target answers only out of a looping clip.

## Encounters (`session::encounters`; I28)
Our own per-module changes, `data/encounters/<game>/<module>.encounter`
(built in; never written into the install): HP (whole or ×), move, add
(absolute or `by` an offset), remove, faction, plot/min1hp flags, and
`per_player hp x<f>` for co-op; any line may hold only `if players >= n`.
Planned for the players driving a body then (I30 P21) and kept: stash,
saves and own link plan it so again (`Place::players`, in the digest). An
added creature goes last (its collision number); a removed one a waypoint.
The Endar Spire: hostiles gain half again per player past the first, and
the Starboard Section adds a trooper from three players. `kjk combat
encounters k1 end_m01ab` prints the table for 1–4 players. Not KOTOR's
encounter objects: those are "UTE encounters", below.

## UTE encounters (`sim::spawner`, the step in `world::spawners`; I35 B8)
KOTOR's own encounters: the GIT's `Encounter List` objects, each from a
UTE (its fields and creature list) with the GIT's outline and spawn
points, run as both games' executables run them
(`context/artifacts/i35/encounters.md` maps each rule).
- **Who starts one**: a living creature hostile to its `Faction` (the
  encounter's standing toward it under 11) in its outline seen from
  above, while it is active and not started; with `PlayerOnly`, a player
  or party member. Any character coming in or leaving, alive or dead,
  runs its `OnEntered`/`OnExit` (active or not) and is its last entered
  or left (`GetEnteringObject` in its scripts). Co-op: any seat's
  character counts, with no per-player scaling.
- **TSL's list**: a count drawn in `RecCreatures..=MaxCreatures`, each
  `SingleSpawn` creature once (by challenge, highest first), each other
  one its `GuaranteedCount` times, the rest drawn from the others.
- **K1's list** (NWN's budget): the points (1.5 to the total level;
  others than party members at half their level) of the hostile living
  creatures within 30 m across x and y of the last to enter make the party
  challenge (`ln(points / 4) / ln 1.5`, plus a half, truncated); its
  points at that plus `Difficulty` minus 1 join the pool; the pick walks
  the creatures highest first within the pool and a tenth (singles free,
  a lone challenge again and again, a shared one drawn at random), splits
  dear picks into cheaper ones toward `RecCreatures`, then tops up from
  the cheapest. The pool is never spent (QUIRKS.md Q-0265).
- **Making them**: one a step, its first. TSL deals them round its spawn
  points (`count / points` each; none each when the points outnumber them:
  all on the second, QUIRKS.md Q-0262); K1 puts all at the point farthest
  from the last to enter. A point taken is a safe spot within 20 m: the
  point itself, else the executables' square rings round it (half sizes
  1, 1 + w, … by the creature's width w, top and bottom edges then the
  sides), a spot on the walkmesh with room for the body and no body on it;
  failing that, the encounter's own place or the rings round it within
  30 m in clear sight of it (the "unseen by players" test sees none,
  QUIRKS.md Q-0263); failing all, the entry waits a step. Facing the
  point's way (`90° − orientation`). The creature is linked to it
  (`GetIsEncounterCreature`, its spawn script latching onto a combat zone
  as `k_inc_generic` has it); one more alive.
- **Exhaustion and reset**: each death or going counts once; the last
  of its alive signals `OnExhausted` and leaves it inactive and not
  started. With `Reset` and `SpawnOption` 1, `ResetTime` seconds later it
  turns active again (one reset counted, `Respawns` at most, -1 always) and
  starts at once if a qualifying creature stands in it (tar_m03ab's Vulkar
  patrol: 30 s, always). Its heartbeat runs only while active, every 6 s,
  the first skipped.
- **Kept**: its active, started, resets, difficulty, alive count, list,
  last entered and left, reset and heartbeat clocks (the save, KJKV 51;
  a module left keeps its own, its clocks moved on by the time away). Not
  kept, as in the games: the creatures' links (QUIRKS.md Q-0261), K1's
  pool, TSL's round. Every draw is the authority stream's.
- `kjk sim probes --only "ute encounters"` (the 204tel, 106per, 506ond,
  tar_m03ab and kas_m25aa numbers; placement; the step; the save).

## The Force (`combat::force`, the step in `world::powers`)
KOTOR's powers become JA's (I22's chains: push, lightning, drain, grip,
speed, heal, protect, absorb, mind trick; the stun and fear rows are
KOTOR crowd control); a character's best row of a chain is its JA level.
`Command::force` selects a row (or `force::PULL`); `FORCE_POWER` casts it
on the press, and grip, lightning and drain run while their buttons are
held. Costs are `spells.2da` `forcepoints` (`ModuleData::force_costs`; in
TSL as its executable adjusts them, below), paid at the cast; a held power
pays it again per second after its first half second (C12). The pool
refills in 20 s, paused while a power runs (C13); in TSL its own rule
replaces that for Force points (below), stamina keeping C13. JA's shapes and physics: push/pull at 256 × level less 0.7 a unit
(counters, absorb); grip's squeeze, lift, carry and crush; lightning's
50 ms strikes from the eye (JA's from the origin, Q-0032) and forked
bolts (Q-0015); drain takes FP and heals; speed scales movement (`force.speed.scale`);
protect and absorb (and TSL's Fury, JA's rage) exclusive; sight (TSL's
Force Sight) breaks tricks on its user; mind trick confuses creatures and
hides the caster from players; crowd control stuns for 3 s at most on a
failed save (Fortitude, the fear chain Will), then 1 s immune, fear ending
on damage (C11).

**Impacts are KOTOR's** (C10, the owner's choice (a), 2026-10-01): where a
row has an impact script in the level's library (K1: `k_sp1_generic`), JA's
delivery only finds whom the power catches and moves them (the shove, the
lift, the carry); the script, run as the caster first in the scripts'
step, decides everything else, damage included: `GetSpellId`, `…Target…`,
`GetSpellSaveDC` (`force::save_dc`), `ResistForce` (`force::resistance`),
the saves (d20 + `Sheet::save`; immune: 2), its
`GetFirst/NextObjectInShape` the caught. Push, lightning, drain, grip and
crowd control so; a held lightning or drain impacts once a second held
(C12's pace), a grip at its latch. Choke, Kill and Wound tick from code
their impact defers (K1's iterative damage, 1, 3 and 5 s in), and the grip
is that damage: it holds while the code waits (no level's time limit), and
let go (released, out of reach or sight, knocked down) its ticks still to
come and its victim's hold go with it (`scripts::LetGo`; owner's call
2026-10-23). A resisted impact defers none, and the grip drops after its
first half second. Heal, Valor,
Energy Resistance, Slow and Supress / Breach are pure casts (whom JA's
range finds; the script heals, buffs, slows or ends effects). A toggle
(speed, protect, absorb, rage, sight) keeps JA's delivery (the run, the FP
absorption) and its start is a cast at the caster: the script's effects
carry its row, last as long as the toggle (turned off early, they end),
and while one runs the native stand-in (speed's and protect's defence,
speed's attacks) gives way, so nothing counts twice; a script taking the
effect off (Breach) ends the toggle. Mind trick's K1 rows aim at
another (Affect Mind), so its toggle stays JA's. TSL's rules win where
they clash with JA's (the owner's call, 2026-10-19): TSL's Mind Trick and
Confusion are casts at the one aimed at (the crowd control's reach),
their scripts freezing it (`EffectMindTrick`: KOTOR's state 18, broken by
damage, an attack or a hostile within √10 m, `SCRIPTS.md`) or turning it
(`EffectFactionModifier`); TSL's Fury is a cast at the caster, its script's
Fury counting kills for damage (JA's rage, its health cost and its ÷ on
hits, gives way). KOTOR's states (`combat::effects::state`: the
cutscene holds, Force Crush's, a mind trick, a droid's scramble) hold as
long as their effects, no 3 s and no immune second: C11 is for the plain
holds. Under TSL's Force Body a cast's cost is 50 / 40 / 30 % from FP and
as much from HP (`combat::effects::pay`), refused unless more than one HP
is left after. Only Affliction and
Plague stay native (KOTOR's are poisons, not carried out yet). Without a
library (the proving ground) every power is native, JA's numbers as % of
the target's pools. Costs and
shoves are predicted; hit points, another's pool, saves and confusion are
authority-only. Force gestures are JA's hand extends on the torso. In
`kjk play` (`--class jedi` for the dev Guardian): F casts the selected
power ([ and ] select), G grips, R lightning, Q drains, the right mouse
button is the weapon's other mode, L the saber style, 1 and 2 the hand;
`--moves` scripts say `force ROW`, `force pull`, `grip`, `lightning`,
`drain`, `alt`, `style`, `hand N`. `cargo xtask duo --class jedi
--moves-host …` casts over the network, and `cargo xtask duo --stage duel`
fights saber to saber (`--stage duel`: a walled floor, the second seat
facing the first, friendly fire full); `kjk sim replay` lists what each
seat was seen running and what it took (hits, blocks, blows held for its
grace).

### KOTOR's effects (`combat::effects`)
What KOTOR's powers add to JA's (K1's descriptions' numbers): speed
+2/+4/+4 defence, Knight and Master Speed's extra attacks multiplying a
weapon hit's budget (C2); Aura, Shield and Armor +2/+4/+6 defence and
saves; Whirlwind (push's second level) holds (C11) and strikes for a third
of the caster's level unless a Reflex save. KOTOR's own effect powers:
**Valor** (+2/+3/+5 STR, DEX, CON and saves, 20 s, the caster's side
within 20 m; poison immunity from the second level), **Energy
Resistance** (resistance 15 to cold, electrical, fire and sonic for 120
s; the improved one on the side and against poison), **Slow** (−2
defence, Reflex and attack for 30 s, Will save), **Affliction** (a point
of STR, DEX and CON every 3 s to 7, slowed: movement halved; Fortitude DC
20), **Plague** (a point a second to 12, slowed, no save), **Supress** and
**Breach** (end the target's first- and second-tier powers, or all). One
sheet modifier (`effects::sheet_now`, with the items') feeds the pipeline,
the saves and the powers. Minds slow foes, dispel buffed ones and keep
Valor and Energy Resistance up.

Scripts' lasting effects (`Lasting`, 24 slots, `script::effects`) fold
into the same sheet; speed effects scale movement, extra attacks the
weapon budget, Force resistance effects `resists`, immunity effects the
holds (the crowd control's too) and poison. Of the bonuses (or the
penalties) one spell gives to the same thing only the strongest counts
(`Effects::counted`): Master Valor links the same +5 saves twice, and a
recast does not stack; different spells' effects do. Effects applied in
an impact, or in code it deferred (`DelayCommand`, `AssignCommand`),
carry its spell row (`GetEffectSpellId`, `GetHasSpellEffect`, dispel).

### KOTOR's rolls (`world::powers`: `power_dc`, `resists`)
- **DC** (the engine's `GetSpellSaveDC`, which `k_sp1_generic` asks): 5 +
  level + WIS + CHA + Force Focus (+1, +2, +4: feats 88 to 90), for every
  power that allows a save.
- **Force Resistance** (`ResistForce`, before the save): one under Resist
  Force or Force Immunity (absorb 1 or 2) has 10 or 15 + its level, and
  resists unless the caster's d20 + level reaches it. It guards crowd
  control, Slow's chain and Whirlwind; JA's absorb still weakens push,
  grip, lightning and drain; Supress and Breach are never resisted.
- **Immunity feats** (the Sentinel's): Fear (98), Stun (99), Paralysis
  (100: Stasis, Mass Stasis) before any roll.
- KOTOR's impact numbers, read off `k_sp1_generic` by `kjk script impacts
  k1` (for the balance pass): Wound and Choke 3 ticks of ⅔ level a second
  (Choke also −4 STR, DEX, CON for 24 s), Kill 3 ticks of ⅙ of the
  target's most HP; Shock, Lightning, Storm level d6 (at most 10 dice, half
  on a Will save; Storm also drains as many FP); Drain Life and Death Field
  level d4 (at most 10); Disable and Destroy Droid level and level d6;
  Push and Wave knock down 2 s / 6 s (Reflex: none); Whirlwind
  ⅓ level a second for 9 s; Stun 9 s, Stasis 12 s, Droid Stun 12 s, Fear
  6 s (C11 caps all at 3 s); Cure 5 and Heal 10 + level + WIS + CHA to
  the caster and the party; Energy Resistance 15 against five types for
  120 s. Push level, Wave ⌊1.5 × level⌋, Choke ⌊⅔ level⌋ a tick, fitted
  from casters of levels 5, 10 and 15.

### TSL's Forms, costs and regeneration (`combat::forms`, `ModuleData::power_cost`, `force::regenerate_tsl`)
Where TSL's rules meet our fight, TSL's win (the owner's call,
2026-10-19); every number is its executable's
(`context/artifacts/i35/combat-force.md`).
- **Forms** are `spells.2da` rows 258 to 268 (`formmask` 0x40): known as
  powers are, granted at Jedi levels 11 to 14 by the base class's table
  (Guardian Shii-Cho, Makashi, Soresu, Force Focus; Consular Shii-Cho,
  Force Focus, Makashi, Soresu; Sentinel Shii-Cho, Soresu, Force Focus,
  Makashi: `levels::granted_powers`, with the prestige classes' powers)
  and by the Masters (`GrantSpell`); the one in use is the sheet's
  `form` (`CurrentForm`), picked with no action and no cost (`Action::Form`,
  the HUD's Form slot, `SetCurrentForm`). A lightsaber Form counts
  only with a lightsaber in the right hand; a Force Form always. Where our
  fight has the executable's counterpart: to hit in `damage::odds`
  (Shii-Cho +1, Makashi +3 against a lightsaber, Shien +2, Niman +1);
  defence against an attacker that is the defender's own target or
  another (`Fighter::target`: Shii-Cho +3 against others, Soresu +2 against
  its target, Ataru +3 / −2, Shien −5 against its target, Niman +1, Juyo
  −2 / −4), in the pipeline, a blade's turn (`world::sabers`) and a scene's
  round alike; the weapon in hand (`world::in_hand`: Makashi +3 damage,
  Shien's critical multiplier +1, Soresu's threat range a step narrower,
  Ataru's a step wider, its deflection Makashi −5, Soresu +4, Ataru −4,
  Shien +2, Niman +1); Juyo +4 to a critical's confirmation and an extra
  attack a round, multiplying a weapon hit's budget as Speed's do. A Force
  power's damage, duration and saves under a Form are its impact script's
  (`k_sp1_generic` asks `IsFormActive`).
- **Costs** (`ForceRules::cost`, `CSWSpell::GetAdjustedForcePoints`): a
  party member's power of a side (`goodevil` `G`, `E`) × `forceadjust.2da`'s
  multiplier at its alignment's tenth, one above 1 eased 0.05 a point of
  its Charisma modifier to no less than 1, rounded down; then the room's
  Force rating (its ARE `ForceRating`; a place is in the first room, in the
  area's order, with walkable floor under or over it, `ForceRules::rating`;
  outside every room 0, QUIRKS.md Q-0248) a twentieth off for the light (60 or more) or on for the dark (under 50);
  at least 1; × 1.2 under Force Potency or Mastery. The cast pays it,
  `GetHasSpell` and the minds ask it, `GetSpellForcePointCost` reads it
  (under Force Body the share its Force points pay). K1 keeps the rows' own costs
  (its executable loads `forceadjust.2da` too [verify whether it uses it]).
- **Force point regeneration** (`HandleFPRegeneration`): only the party's
  Force users regenerate (its other creatures never do), a rate of
  `regeneration.2da`'s out-of-combat base (1.0), or in a real fight (fake
  combat is none) the in-combat base (0.1; Niman with a lightsaber 0.3 of
  the out-of-combat one, Force Affinity 0.6), + WIS modifier × `wismodbonus`
  (0), + 0.25 with Regenerate Force Points (feat 143), + each
  `EffectFPRegenModifier`'s percent, + 0.5 under Force Focus; the room's
  rating takes that share off for one under 41, adds it for one over 59;
  the pool and its temporary points fill at that rate in `fpregentime`
  (90 s), kept by hundredths with the executable's accumulator
  (`ForceState::fp_carry`), within 0 and the pool (a negative rate drains).
- `kjk sim probes --only "tsl force"`: the Forms' table, the costs, the
  rooms, the regeneration, the grants, and the routines on the proving
  ground (16 checks).

## Natural healing (`world::powers`: `regenerate`, `regenerate_k1`)
Only the party's living members heal (both executables test the creature's
in-party flag and that its hit points are above 0), players of every seat
and companions alike, in combat or out by the game's own rows:
- **K1** (`HealthRegeneration`, its `regeneration.2da` rows 0 in combat
  and 1 out of it, column `healthregen`): that percent of the most hit
  points a second, at least 0.0001 a second, in whole points (the
  executable keeps the fraction). K1 ships 0 in both: a point every
  10,000 s, so no healing in play. Its `forceregen` (1 % of the most FP a
  second out of combat, the floor in it) is not read: C13 keeps JA's.
- **TSL** (`Regeneration`, `CSWSCreature::HandleHPRegeneration`): see
  `SCRIPTS.md` (`DisableHealthRegen`); every `TimePerHP` (6 s) a rate of
  the out-of-combat base 1 (in combat 0) + CON modifier × 0.3 + Treat
  Injury rank × 0.1 points.
- `kjk sim probes --only regeneration`: K1's shipped rate, nothing in a
  minute, a 1 % table's share, its in-combat row, TSL's healing, each on
  both seats of a co-op party (5 checks).

## Stamina and feat specials (`combat::specials`; C7, I22 section 4)
One second bar (`Sheet::pool`): a Force user's FP, anyone else's stamina,
100 + 10 × CON modifier, refilling as FP does (20 s). It pays the saber
specials and the blaster feat specials, 15% each (one a 3 s round
sustained). The alternate button takes the special the gun suits (else
another known): **Power Blast** (pistols; JA's Bryar charge): hold up to
1 s, one shot on release, +5/+8/+10 damage at −3, paced by its whole
time; **Rapid/Multi Shot** (rifles and the rest; the E-11's fast fire): a
press queues a burst of 2 (3) shots 150 ms apart, the gun back one refire
short of them, −4/−2/−1 to hit and defence for 3 s; **Sniper Shot**
(disruptors; the zoom): held, movement halved and no jumping, each aimed
shot's threat range ×2/×3/×4 and defence −5 for 3 s. Without a special
or the bar the alternate fires the primary. Creatures use theirs
(`ai::choose_special`). **Sneak attack** (+1d6 a tier) lands on a target
hit from its rear 90° (unless it has Uncanny Dodge), held (stunned,
knocked down, gripped) or unaware (a creature with no target), within
10 m; and the first blow out of stealth within 1.5 s of the attack that
ended it, on a target that had not perceived the attacker (the attacker
stays hidden to perception until then: KOTOR's round begins in stealth). Whoever lands one is told "Sneak
Attack!" (the game's line 1459). KOTOR's numbers come from the feats'
`dialog.tlk` text. `kjk combat
specials k1`: 18 checks. `kjk combat damage k1 end_m01ab`: the type step
(the Starboard Section's assault droid resists 15 energy: a disruptor does
0.02 HP a hit, a sonic pistol 0.97).

## Stealth and mines (`world::covert`, `world::traps`)
**Stealth mode** (X, `buttons::STEALTH`) turns on with base Stealth ranks
and a stealth unit worn, out of combat, and ends on an attack, a power, a
grenade, combat or the unit taken off, and, without a word, on a
conversation beginning with it (both games' executables end both its
parties' stealth; TSL's `SetKeepStealthInDialog` keeps it:
`docs/SCRIPTS.md`); in it the body walks. The
character wears KOTOR's stealth shell (`fx_tex_stealth`, its meshes drawn
again 2 cm out along their normals and added over the body,
`stealth_view`) and the HUD's stealth toggle (`TB_STEALTH`) is lit.
**Laying a mine** (O, `buttons::LAY_MINE`) sets the best trap kit the
party carries in 2 s, standing still. A trap the party has detected
(Awareness) is drawn as its `traps.2da` model posed as its own `detect`
clip ends: the dome over its 1.5 m trigger raised and lit, the model's red
for a hostile trap, green for the party's own (`made::track_mines`); the
mine itself is grenade-sized. Disarm and Recover are on the use menu.
A mine keeps the Demolitions its setter had as it set it (TSL's
`OwnerDemolitionsSkill`, the total rank then: `TrapState::owner_demolitions`):
its trap script reads it (`GetOwnerDemolitionsSkill`), and a mine set on a
door blasts by it, whatever the setter has since.
**Perception** runs at KOTOR's cadence (each creature looks every 300 to
699 ms while its AI runs, every 4 s while a conversation holds it); a
stealthed character is seen only past the spot check and heard only past
the listen check (Awareness against Stealth, rolls held 20 s; walls stop
listening). `kjk combat stealth k1`: 4 checks against `tar_m09aa`'s Sith
(seen plain at 10 m, unseen stealthed on the same spot for 5 s, the
cadence, the opener's sneak attack); `kjk script traps k1`: the traps.

## Items (`combat::items`; I22 sections 5 and 7)
KOTOR's consumables, four kinds carried (`Sheet::items`: creatures from
their inventory), what was used and what runs in `Fighter::pack`; the
numbers are K1's item descriptions. **Medpacs** (3, `buttons::MEDPAC`)
heal 10/20/30 + WIS + 1/2/3 × Treat Injury after 1.5 s less 100 ms a rank
(at least 0.5 s), the weapon idle; droids use **repair kits** (15/25/35 +
INT + 1/2/3 × Repair) and no medpac. An **antidote** comes first while
poisoned or weakened; a **squad stim** heals the user's side within 20 m
by its points (`items.squad_stim.points`). **Energy shields** (5) absorb their points of the damage types they
name (the hit's type, `Assessed::damage_type`) for 200 s or until spent.
**Stims** (4) for 120 s: adrenals +4/+6 STR, DEX or CON, battle stims
+1/+2/+3 to hit and damage and +8/+16/+25 vitality, a pool taken before
hit points and gone at the end.
One shield and one stim at a time; items are used on a press. Minds heal
below 35% (an antidote when afflicted), raise a shield and take a stim in
a fight (`ai::choose_item`), never before they are aware. `kjk combat
items k1`: 12 checks.

## Grenades (`combat::grenades`; I22 section 7)
KOTOR's grenades thrown as JA's thermal detonator: the grenade button
(`buttons::GRENADE`, T in `kjk play`, `grenade` in `--moves`) charges
while held (0.15 to 1 over 900 ms; a gun stops firing meanwhile) and
throws the first kind still carried on release: 900 units/s × strength
along the view plus 120 up, falling, bouncing at 0.65 and resting on a
floor, bursting 3.5 s after the throw or on touching a body. The blast is
KOTOR's (the item descriptions' numbers, keyed by `spells.2da` rows 87–95,
TSL's launched rows as theirs): everyone within 4 m in line of sight
takes its typed damage (`Source::Blast`, guards and friendly fire's area
rule apply; the thrower too) halved by a Reflex save (DC 15; a natural 20
saves, a 1 fails), and its secondary unless the save spares it: frag 20
piercing, thermal 60 energy and a knockdown, plasma 36 fire, ion 15 (45 on
droids), sonic 20 and −6 DEX for 30 s, CryoBan 20 cold and paralysis,
concussion a stun, adhesive entanglement (no save), poison 4 every 3 s for
30 s (DC 25); holds are C11's 3 s and a second's immunity. Sheets carry
two kinds (creatures from their inventory; the dev soldier three frag and
a thermal, the dev Jedi two frag); fighters count throws and carry poison
and weakness. Creatures throw at targets 4–12 m off now and then
(`ai::choose_grenade`). `kjk combat grenades k1`: 16 checks.

How `kjk play` shows a throw (from the snapshots alone; the survey is the
journal's `grenade-visuals/SURVEY.md`):
- The thrower plays KOTOR's grenade throw (`spells.2da` `castanim`
  `throw`: `animations.2da`'s `throwgren`) over its upper body
  (`torsoupr_g` down; legs and torso aim stay JA's; `acting::Throw`): the
  charge draws the hand back in 450 ms and holds it there, the release
  brings it over the head in 90 ms as the sim throws, and the clip plays
  out (KOTOR's own pace is 550 and 270 ms; the throw is on the button's
  release, so the clip is fitted to it). Humanoid bodies only: droids' own
  `throwgren` is not played.
- The grenade is its kind's `spells.2da` `projmodel` (`w_fraggren_001`,
  `w_thermdet_001`, …; `grenade_view`), from the thrower's hand as drawn
  onto its way over 120 ms, rolled along its way at most 2 turns a second,
  lying where its box rests. JA's thermal glow (its `thermal_w_2` stage,
  yellow, 0.6 ± 0.4 three times a second) is drawn on it as a sprite, three
  times as fast over the last half second (ours).
- Its sounds (`power_fx`): JA's `thermal/charge` as the charge starts and
  `fire` as it leaves, `thermloop` held while it lives, `warning` half a
  second before the burst; each bounce and the landing KOTOR's
  `grenadesnd.2da` sound for the surface met (`fs_metal_droid1`); the
  burst its visual effect's KOTOR sound and model (the impact script's
  `VFX_FNF_GRENADE_*`, `script_fx`); JA's explosion and shockwave are no
  longer drawn over it.

## Sabers (`combat::saber`, the step in `motion::weapon`)
JA's move table (`saber::moves`: 162 moves in JA's numbering, their
quadrants, holds, blocking and chains; the transition and chain-angle
matrices) and the saber's state (`SaberState`: lit, style, move, chain
counter, blocked state, weapon change, knockdown, lock time) drive
JA's lightsaber weapon logic inside move_body: a move is picked when
the weapon timer runs out, from the attack button, the movement keys, the
style's chain limit and the transition picker; each move's animation is
its style's (groups of 77 in `motion::Anim`), on the parts JA puts it on,
held one frame short (`HOLD_MOVE`, the fast and strong styles' transitions
at 1.5 and 0.75) or whole. The specials start as JA's do (lunge, flip,
death from above, back attacks, cartwheels and butterflies, the pair's
and the staff's jump and spin attacks, the staff's back-flip, katas,
kicks, stab-downs, the roll stab, mirror attacks), paying JA's cost as a
share of the pool (I22 C7: a kata half of max FP, the forward and back
specials a quarter, the side ones a tenth; without the Force from
stamina, and an empty bar pays for nothing),
and move the body as JA's saber-move and kata movement commands do; the
attacks slow the runner (JA's client speed adjustment). Swords, vibroblades and
staves use the same tables (C9). KOTOR's feats open the styles (C8):
lightsaber or melee proficiency medium, Critical Strike medium 2 and 3,
Flurry fast, Power Attack strong; two blades fight in the dual style, a
double-bladed weapon in the staff style. The style follows what the hands
hold every step (`saber_step`), however they changed (the equip screen, a
swap, a script): a second blade put in hand fights dual at once, taken
away the blade goes back to a single style.
- The saber lights on the first press of attack after it is drawn and
  goes out when another weapon is taken up; `Command::weapon` picks the
  hand (JA's 200 ms down, draw, 250 ms up; never mid-move).
  `buttons::SABER_STYLE` cycles the styles (JA's attack-style cycle,
  queued while a move plays); a staff puts its second blade out or back;
  a pair stays dual (the owner's rule, 2026-10-23: JA's key would put its
  second blade away).
- Knocked down (`SaberState::knock_down`, what kicks and pushes call),
  the body lies, crouched and still; once up a side, forward or back key
  rolls up, jump gets up with the Force (from jump level 2 the acrobatic
  get-up), and after 4 s it gets up by itself; the weapon rises 250 ms
  after.
- JA's synced rolls (the chain limits, a bounce's new quadrant) are a
  hash of the tick and the character. A creature that flinches drops to
  the fast style (a pair or a staff keeps its own).

### Weapons out and away (`Fighter::sheath`, the step in `motion::weapon`)
KOTOR holds a weapon out only in a fight: out while the fighter is in
combat (`combat_until`, 8 s after its last hit or blow, reone's
`kDeactivateDelay`) or fights someone, and put away after. A blade in hand
(a lightsaber out, a sword or vibroblade to its sheath) goes once nothing
holds it; an attack, at someone or not, holds it out 8 s too
(`SaberState::out_until`: a swing at no one is no fight, so no battle
music). Not while it swings, changes weapons, throws its saber or is down.
A lightsaber in hand lights as its wielder's fight begins (a target, or
in combat), before its first swing, so a Jedi closing in turns bolts.

How it looks, JA's way: a blade put away plays the style's put-away
flourish (`LS_PUTAWAY`: `BOTH_STAND2TO1`, the pair's `BOTH_S6_S1`, the
staff's `BOTH_S7_S1`) as the saber turns off, and one lit from off its
draw flourish (`LS_DRAW`: `BOTH_STAND1TO2`, `BOTH_S1_S6`, `BOTH_S1_S7`),
the first swing after it; an attack cuts a put-away short. A gun put away
is lowered (`TORSO_DROPWEAP1`, 200 ms) and raised again before it fires
(`TORSO_RAISEWEAP1`, 250 ms: a holstered gun does not fire); put away by
its player or by the game's 8 s, its torso then follows its legs, and the
weapon's timer runs out as JA's always does (a conversation holding the
world past the 8 s with a shot's timer still running left the torso on
that shot's frame). `kjk play`
carries a player character's weapon not in hand on its body
(`weapons::Stow`; everyone else's is hidden, as KOTOR's are):
a lightsaber clipped to the belt at its side's hip (`pelvis_g`), a pistol
in a holster on its side's thigh (`rthigh_g`, `lthigh_g`; any gun of a
pair), going there as a put-away ends and back to the hand as a draw
starts; others are hidden (`weapons::armed`). A holstered pistol is drawn
and holstered by hand (`draw_anim`, view only): the hand reaches its
holster, takes the gun and brings it up to aim over the raise's 250 ms,
and carries it down into the holster over 600 ms, the torso holding its
ready pose while the gun is in hand (each arm its own gun for a pair, the
rig's `Aim` with `Stowed`). Who says what is out (`combat::Sheath`):
- **Fight**: the game, as above.
- **Holstering**, then **Holstered**: the player put it away with the
  holster key (H, `Action::Holster`; the console's `ui holster`): at once,
  though its 8 s still run, or as a swing under way ends; away even in a
  fight, until the player attacks (a click draws, held it swings or
  fires) or takes out a weapon. JA has a saber toggle, KOTOR none.
- **Lit**: a script holds the saber lit (`SetLightsaberPowered` with its
  override) until a script lets go; a scene's blows light it as a fight.

`kjk sim probes`' `sheath` checks (the approved test `holster_weapon`).

**The weapon swap** (TSL's second weapon set, in both games;
`WORLD-STATE.md`): the sets exchange at once, with no animation of their
own (TSL's `AIActionSwitchWeapons` completes the update it starts; the
weapon models swap). A player's swap in a fight is TSL's combat round
action of 1000 ms (`CSWSCombatRound::AddSwitchWeaponsAction`), the swap at
its start: here the weapon waits `combat::SWAP_MS` (1000 ms) after what it
was doing (`Fighter::weapon_wait_ms`), as an item's use waits. Out of a fight,
and a script's swap (`ActionSwitchWeapons`, which attacks on after it), it
is instant.

### Blades (the step in `world::sabers`; I24 part 2)
- **Where**: each character's copy of JA's skeleton (`saber::blades`,
  `docs/SKELETON-SIM.md`) follows its animations, posed with the server's
  look (`saber::look`); `kjk play` draws the blades there (QUIRKS.md
  Q-0083); a lit weapon's blades
  are placed when someone or a bolt is near (`single_1`; `dual_1` for a
  double-bladed one; swords and staves the same tags, 32 and 28 units).
- **Cuts** (C5): an attacking blade (an attack, or a special inside its
  animation-power window) is swept from last tick's place (`saber::sweep`:
  every 8 units, each point traced to where it went, cut into arcs) through the
  bodies its holder may harm (a player's as it saw them, C18); each body
  it cuts is owed one roll of the weapon at most per move, the share its
  sweep is of `NOMINAL` (160 units) and half at least (`LEAST_SHARE`: a
  graze still tells), delivered as one hit when the blade leaves it. Its
  cadence (C2) is the time since its holder's last attack began, its
  move's hold at least and 750 ms at most (a first attack's; KOTOR's round
  over P): a chain's swings are a third of a second, the return and
  transition between them less so, and the pace once counted the hold
  alone, two fifths of what the blows came at (`Swing::begin`; the owner's
  balance of 2026-10-15). The roll is the style's KOTOR feat
  attack (`Kit::swing`, as `Special::shot` shapes a blaster's): Power
  Attack +5/+8/+10 damage at −3 to hit in the strong style, Critical
  Strike's threat range ×2/×3/×4 in the medium, Flurry −4/−2 to hit in
  the fast; none in the dual and staff styles. Kicks (`saber::kick`): 2 or 3 JA
  points, a toss, a 700 ms knockdown for the busy and the bladeless.
- **Blocks** (C6, `saber::defence`): a holder not attacking turns blows
  within its arc, and the blow it meets is held (no crush of a block by
  state, QUIRKS.md Q-0060; a second blow while its parry plays still breaks
  it) (Jedi Defense ±49.5°/69.5°/87°; a proficient melee
  wielder at 1). A blade's blow is turned (by state, or by the blades
  meeting) only on KOTOR's chance to miss, the attack with what it swings
  against the defender's defence (`damage::chance`), drawn once a swing a
  body and kept on the swing (`sabers::Step::turned`, `Swing::verdict`; a
  body the blade only cut has had no draw, so its blade meeting the swing
  later still draws): JA turns
  every such blow, and a level-1 soldier against the Endar Spire's Sith
  had a third of its blows stopped (the owner's balance of 2026-10-15).
  Bolts (±26°/53°/72°, the deflect property a level per
  5, one less rising or backing off): absorbed at 1, deflected at 2,
  reflected at 3 (the bolt its blocker's); level 0 turns none. A bolt that
  strikes a lit lightsaber's blade is turned with no arc or state test,
  deflected at level 0 (QUIRKS.md Q-0047); an unblockable one (the ion
  rifle's) bursts on it, as in JA. Melee blades turn no bolts and stop none:
  only lightsabers' blades are in the bolts' traces.
- **Clashes** (`saber::clash`): two blades' sweeps meeting, or a block,
  run JA's tree (knockaways, broken parries, bounces, deflections, the
  both-attacking contest) with its disarm odds; 10% of clashes (5%/30% of
  blocks) try a lock (`saber::lock`: presses, offence + 1 frames per 25 ms,
  super-breaks, knockdowns, C22's kill for creatures). Throws
  (`saber::throw`): Throw Lightsaber's row and cost, out, back, caught,
  knocked down by a better defender. All of it on the authority (C20).

```bash
kjk combat roster k1 end_m01aa
kjk combat fight k1 end_m01aa --seconds 20 --hostile end_sith2 --ally end_trask
kjk combat force k1    # each power at end_m01aa's Sith soldier, checked
kjk combat saber       # each style's moves, specials, knockdowns, checked
kjk combat duel        # cuts, blocks, bolts, clashes, locks, throws, kicks, roll stab, cost
kjk combat duel --only bolts,blade_bolts   # some sections; bouts kept as *.kjkr
kjk combat duel-lag    # C18 and C19 at 0 to 350 ms: disagreement, grace, rewinds
```

## Drawing a fight (`kjk play`)
Render reads the presented snapshots only. `cast` puts every character
with a body in the scene where its snapshots have it: the play body and
creatures with KOTOR's humanoid skeleton (`asset_ja::rig::is_humanoid`: the
`B` bodies and the Sith soldiers' full bodies) play JA's animations through
the rig, as the player does; droids and beasts play their own KOTOR clips
(standing, walking, running, dying then dead). The scene's creatures are
those bodies (one body per character). The nearest 16 within 30 m of the
camera are posed each frame. `combat_fx` draws bolts with their weapon's
JA shot effect (`effects/blaster/shot.efx`, `bryar/shot.efx`, …; lines and
sprites, `asset_ja::fx`) and plays an effect on each new hit event, at
the body node for its location, by the weapon that struck (the event's
`weapon`): a blaster's impact, or a blade's (saber or melee) JA saber
blood sparks, as the cgame's `EV_SABER_HIT` on a person picks them by the
blow's damage in JA points (1% of the struck's hit points): under 5
`blood_sparks_25_mp` once, under 20 `blood_sparks_50_mp` once, else
`blood_sparks_mp` three times; a contact (below) the full sparks once, as
JA's client does. The sparks bounce off the floor and come to rest on it
(Q-0076, fixed; JA's fly through it). Dumps count the physics traces
(`physics_traces`, the most in a frame, the slowest frame's ms).
Powers, blasts, fists and batons play none here. A block plays where it
happened (the event's point: where the blades met in a clash, the
defender's blade nearest the blow in a block by state, the bolt): JA's
`saber/saber_block` for a blade, once a clash, with the clash flare
(`blade_fx`: `gfx/effects/saberFlare`, 150 ms from 50 ms before it, drawn
over the screen in JA, a sprite at the clash here, as large on screen),
and KOTOR's `VFX_COM_BLASTER_DEFLECTION` (`v_blastdef_imp`) for a bolt.
A blaster's hit on a body plays KOTOR's `VFX_COM_BLASTER_IMPACT`
(`v_blaster_imp`) there, as its executable plays it on a bolt's arrival
(`blaster_view`; `context/artifacts/kotor-blasters/SURVEY.md`). Dumps count them
(`combat_fx.blade_blocks_seen`, `bolt_blocks_seen`). Effects run the
effects their runners name and lay their decals as marks (100 at once; one
pushed out fades over 1 s); see `FX-PRIMITIVES.md`. Deaths are the
snapshot's `BOTH_DEATH*`, played to the end and held.

**Contact sparks (C20).** A joined player sees its own swing ahead of the
host's snapshots (its predictions); waiting for the hit's event would put
the sparks a round trip after the blade went through. So each prediction
`play_view` follows sweeps the player's blades from the one before,
through the others as they are drawn when it shows, by the authority's
own sweep (`sim::seen_cuts`: the same segments, trace box and walls; the
test `kjk combat duel-lag` holds against the authority's verdicts). Each
body met once a swing plays the blood sparks at once where the blade met
it; the hit event that follows plays none again (`CONTACT_MATCH_MS`).
Only sparks: the flinch, the numbers and the HP wait for the snapshot, and
a blade or guard that turns the blow on the host shows as the block it
is. Dumps count them with the own bolts' impacts (`combat_fx.contacts_shown`,
and of them `contacts_confirmed` by a hit and `contacts_unconfirmed`, by
a block or by none), and the hits
played (`bolt_hits_seen`, `blade_hits_seen`).

**Sounds** (`sound`; how they are heard: `SOUND.md`). What plays when:

| when | sound | where |
|---|---|---|
| a lightsaber's hit on a body (and a contact spark) | JA's `saber/saberhit1–3`, one by the hit's numbers | where it struck |
| any other blow's hit on a body (blades, staves, fists, claws, batons; and a contact spark) | KOTOR's (`weapon_sounds`): `weaponsounds.2da`'s row by the striker (its appearance's `appearancesndset.2da` `weapon`, else its weapon in hand's `baseitems.2da` `weaponmattype`, else 0), column by what the struck wears (`forcefield` behind a shield, else its sound set's `armortype`, else its body armour's base item's `armortype`, else `leather`), `0` or `1` at random: a vibroblade on a Sith soldier `cb_ht_bladeleth1` | where it struck |
| a saber lights; one goes out (alive) | its saber's `soundOn` / `soundOff` (`single_1`, `dual_1`: `saberon`, `saberoff`) | with it / where it was |
| a lit saber in hand, alive (every frame) | its saber's `soundLoop` (`saberhum4`), held | where it stands; the player's at the camera |
| a new attack, not a kick | one of `saberhup1–8` (any other weapon: its row's `swingshort0–2`, `cb_sw_blade1` for a vibroblade), weapon slot | where it starts |
| an animation passes a keyframed sound (`animevents.cfg`) | its sound: whooshes by weight (`saberhup1–3`, `4–6`, `7–9`), spins, body falls, fists' and melee swings; none of the `*` voices. Held by any other weapon than a lightsaber, a whoosh is its row's `swingshort` (fast) or `swinglong`, a spin its `swingtwirl`; JA's fists' and kicks' `melee/swing` are row 0's (`cb_sw_unarmed1`) | with it (a whoosh: where it starts) |
| a blow turned by a blade (`Parried`; a clash's two events once) | one of `saberblock1–9`; a blow not of a lightsaber its row's `parry0`/`parry1` (the effect `saber/saber_block` with it) | the event's point |
| a shot (below: a joined player's own at once) | KOTOR's: its base item's `ammunitiontypes.2da` row's `shotsound0` (a Power Blast's `shotsound1`: `cb_sh_blast1`, `cb_sh_medbl1`, `cb_sh_hvybl1`, `cb_sh_sonbl1`, `cb_sh_pwrblast1`), weapon slot, at gain 0.8 to 1 and rate 0.92 to 1.08, each shot's by a hash of the shooter and the moment; its row's muzzle flash model (`v_muzflash_01`) on the gun, and a feat's visual there (`blaster_view::feat_row`) | the shooter / the gun's muzzle |
| a disruptor's shot (its weapon timer starting over in an attack) | its row's bolt drawn as a tracer to the body struck, else to the level, where the wall impact plays | from where a rifle's bolt leaves |
| a blaster's hit on a body | its row's `impactsound0` (`cb_ht_blastleth1`, …; K1's Sonic row names a file K1 lacks: TSL's `cb_ht_hvyblleth1`) | where it struck |
| a bolt turned by a blade (`Deflected`: sent on or absorbed) | KOTOR's blaster parry `cb_pa_blast1` | the event's point |
| a bolt gone in its life, with no hit or deflection of its shooter's that tick | `VFX_COM_BLASTER_IMPACT_GROUND` on a floor, `VFX_COM_SPARKS_BLASTER` on the rest (KOTOR's misses meet nothing: the nearest it has), and its impact sound's surface family (`cb_ht_blast` + `metal`, `wood`, `stne`) | where its last way, traced with its box against the level, stops |
| any effect played (impacts, flashes, …) | its own `sound` primitives, one file each, nobody's (a grenade's are left out: KOTOR's to come) | where the effect places them |
| push / pull cast (a throw's wait starting, hand out) | JA's `force/push`, `force/pull`, body slot | the caster |
| speed, sight, mind trick, rage start; protect, absorb start | JA's `speed`, `see`, `distract`, `rage`; `protect`, `absorb` | the caster |
| speed, protect, absorb, rage, sight running (every frame) | their loops (`speedloop`, `protectloop`, `absorbloop`, `rageloop`, `seeloop`), held | the caster |
| lightning, drain start (a channel) | `force/lightning`, `force/drain`, body slot | the caster |
| a lightning hit; a drain hit; a hit on the protected | one of `lightninghit1–3` one time in three; `drained`; `protecthit` | the struck |
| held in a grip; a Force jump starts | `force/grip` every second held; `force/jump` | the gripped; the jumper |
| a heal (the power button pressed with Cure or Heal selected, paid) | `force/heal`, item slot | the caster |
| a KOTOR-only power cast (stun, Valor, Resist, Slow, Supress / Breach: hand out, paid) | KOTOR's `v_useforce` (`spells.2da` `castsound`) | the caster |
| an item used | medpacs, antidotes, stims KOTOR's `gui_inject`; repair kits `gui_upgrade`; a shield `gen_shieldbluon` (Sith, Mandalorian: `…redon`), its loop while it lasts, `…off` when it ends | the user |
| a KOTOR clip a body plays (a script's animation, a round's swing and its answer) passes one of its MDL events (`clip_events`, the K1 executable's hooks) | `SwingShort`, `SwingLong`, `SwingTwirl`: its row's `swingshort0–2`, `swinglong0–2`, `swingtwirl0–2`; `HitParry`: `parry0/1`; `Clash`: `clash0/1`; the row its weapon in hand's `weaponmattype` (a lightsaber's row 1 too, fists 0: `WeaponSounds::in_hand`), weapon slot. `Contact`: `visualeffects.2da` 4011 (`v_parrspr_imp`) at its `lightsaberhook` node, left there. `hit` is the round's landed blow below; `snd_Footstep` a footstep where the walk steps nothing (`ambience::ClipSteps`, `SOUND.md`). Every body with KOTOR clips, a droid's or a beast's too, and a cutscene's stunts | with it / the hook |
| a scene's melee blow lands (one of the two fighting a round) | KOTOR's hit, its attacker's row in hand (a lightsaber's `cb_ls_hitsoft1`/`hithard1` too) by what the struck wears; the blow carries its attacker's weapon (`Source::Script`'s `shot`), so its blood shows as a fight's | where it struck |
| a melee attack begins (a scene's swing, or JA's saber or melee move entering an attack, `grunts::swung`); a landed blow on one still standing | its sound set's attack grunt (slots 9–11) / pain grunt (12–13) at random (`grunts`), the player's own always, others one time in five, voice slot [verify: two flags of the executable's attack that skip them are not read] | with it |
| a hit with no weapon (a script's `EffectDamage`, a power's, a blast's) on one standing | the body's damage clip once over its pose, the wield's `g<wield>d1`, a creature model's `cdamages` (`acting::Flinch`), not where JA's pain plays nor on a player's own body; no blood, sound or grunt (the executable's come at a weapon attack's `Hit` event); its spasm for electrical and dark-side damage not drawn (the event carries no type) | — |
| a hit dealt by the player's own character, or on it | its damage in whole hit points floated red over the struck for 1.5 s (`floaty_view::Damages`), as K1's Floating Numbers | — |
| a visual effect with `ShakeType` 2 begins (`VFX_IMP_SCREEN_SHAKE`) | the player's camera shaken from `ShakeDelay` for `ShakeDuration` (`script_fx::Shake`), `20 (1 + cos(2π r / T)) sin(2π r / 30)` degrees on pitch and yaw, each turned back or not at random [verify the size on screen]; no other shake | — |
| a creature dies | its sound set's death line (the UTC's `SoundSetFile`, `soundset.2da`, the SSF's 16th strref, the talk table's sound), voice slot | where it falls, going with it |
| a grenade's charge starts; it is thrown | JA's `thermal/charge`; `thermal/fire` (KOTOR's `spells.2da` names none), weapon slot | the thrower |
| a grenade lives (every frame); half a second before it bursts | JA's `thermal/thermloop`, held; `thermal/warning` | where it is |
| a grenade bounces or lands | KOTOR's `grenadesnd.2da` sound for the surface met (`fs_metal_droid1`) | where it bounced |
| a grenade bursts | its visual effect's KOTOR sound (`cb_gr_fragment`, `cb_gr_thermal`, …; JA's explosion sound is left out) | where it burst |
| a lit lightsaber's blade meets the level (the blade as drawn, traced hilt to tip each client frame, as the cgame traces the blade it draws: alone or in a fight) | sparks `sparks/spark_nosnd` a client frame; two client frames running, one of `saberhitwall1–3`, at most every 100 ms a blade (a melee weapon's blade meeting the level shows and sounds nothing: only its blows and parries sound, owner 2026-10-10) | where it meets |
| a lit lightsaber's blade drags along the level itself (met two client frames running; not a door, a placeable or a body) | no sound of its own: JA's burn (`gfx/damage/rivetmark`, 10 s) and orange glow (`gfx/effects/saberDamageGlow`, 1.5 s) laid along the way it dragged, a client frame's strip each, onto the level's faces (`saber_marks`; QUIRKS.md Q-0280) | where it dragged |
| a roll starts (move_body's `ROLL`: crouch pressed on the run, or a hard landing) | the body's whoosh, KOTOR's `cb_sw_unarmed1` (row 0's swing; the roll is ours, KOTOR has none), body slot; the roll stab after it is a new attack | the roller, going with it |

**Muzzle flashes and own bolts (`shots`, QUIRKS.md Q-0070).** A shot is a
blaster holder's torso starting (or starting over) its attack from one
snapshot to the next; its row's muzzle flash model plays at the gun's
muzzle as drawn (the KOTOR model's `bullethook`, following the gun; past
the gun hand for a model without one) when the view reaches it, and the
bolt's own light (its model's, lit for its first third of a second) lights
the scene around it. Bolts are drawn leaving that
muzzle straight to their way 256 units out, where the gun is aimed
(QUIRKS.md Q-0085); where they hit is the sim's. A gun out is held up and
aimed that way (`cast::gun_aim`): turned upright down the line, the other
hand under the barrel at the foregrip read from the model, each shot
kicking up and back by its blaster (`weapons::handling`); while it is put
away the torso plays the legs' part. Drawing it blends the torso up in
50 ms and eases the aim in (60 ms a step, so most of it within 180 ms),
but a shot is aimed in full at once, always leaving along the barrel;
putting it away blends and eases out over 250 ms. A joined player's own
shots come from its
predictions, at once. Each bolt the prediction fired flies from the muzzle
at own time; the host's copy (advanced by the view lag, C18) stands where
it does when the remote view shows it, takes over, and any distance left
closes with a 50 ms half-life (a copy is its bolt when it left the same
muzzle the same way within half the shot's cadence). An own bolt the
host's snapshots lack four ticks on was not fired there: it stops and
vanishes. Every own bolt stops at the first wall or body it meets as
drawn; a body's impact plays then, and the hit event that follows plays
none again. A bolt the host holds on a remote player's body for its grace
(C19) is not drawn while held; a block that turns it plays its deflection.
Dumps count `flashes_own`, `flashes_others`, `own_bolts`, and of them
`own_bolts_handed`, `own_bolts_met` (a body), `own_bolts_unfired` and
`own_bolts_stopped` (a wall, or a body the host met that the view never
did).

**The powers' effects (`force_view`, QUIRKS.md Q-0071).** From each
character's Force state as the view shows it, every frame: lightning
(`effects/force/lightning.efx`, `lightningwide.efx` at level 3) from the
left hand along the view while a lightning channel runs, ending at walls;
grip's red puffs at the hand while a grip runs; push's and pull's
distortion (`scene_fx::distort`) for 500 ms from the cast, which is a push
or pull hand whose wait started with it. The own character's come from its
predictions at own time, at once; everyone else's from the snapshots. A
power needs a module (its cost is `spells.2da`'s): none casts on the duel
floor. Dumps count the frames drawn (`force_fx.lightning_frames`,
`grip_frames`, `distortion_frames`).

```bash
kjk play --stage end_m01aa --arrive wp_end_weld01_02 --hostile end_sith2 \
  --hostile end_sith3 --ally end_trask --moves '200ms yaw 186; 12s attack yaw 186'
```
The left mouse button fires in a window; `attack` in a `--moves` segment
holds the trigger. A press something took (a movie skipped, a line
skipped or a reply picked, the click that takes the cursor, a button
still held as the last screen closes: Take All) does not:
it reads as up until it is let go (`game::swallow`), so the click
that ends the opening crawl does not swing, nor its Space jump; a joined
player's too, whose input play holds while a movie is up as the host's
(`sim_host::PlayerInput`). The HUD (`hud`, its own camera over the scene, KOTOR's
640 × 480 layout) shows the player's HP and second bar (FP blue, stamina
gold), a shield's points left, the selected power's KOTOR icon
(`spells.2da` `iconresref`), and its items as slots by kind with a pip a
use left.

`weapons` puts each character's weapons in its hands: a creature's UTC
items, a player's sheet's base items (model variation 1 until the
inventory). The weapon of the hand out hangs at the right hand's `rhand`
node (JA's `*r_hand` bolt on `--body ja`), two blades in both hands; a
blade weapon is gripped JA's way (`asset_ja::rig::Grip`). A K1 hilt's blade
planes give way to JA's blade (glow sprites and core, `single_1`'s or
`dual_1`'s length, the planes' colour), lit as the sim lights it
(`blades::slots`), growing and trailing per drawn frame; a thrown saber
flies, spinning, where the snapshots have it. Dumps list each character's
weapons and lit blades (`characters.roster`), `blade_ends` the blades
drawn.

The characters' weapons follow their sheets in the snapshots (`armory`): a
scene is composed with every seat holding the local sheet's weapons and
every creature its template's, and a character whose snapshot sheet names
others (a joined player loads before it knows the host's sheets; later, a
weapon equipped; a creature's script drawing the blade in its pack, as
K1's `k_ai_master` has a human gunner do with its foe within 3 m) has its
models read on
a thread, spawned (`scene::Spawner`) and hung in its hands
(`cast::Cast::rearm`), the old ones and their blades retired. Who holds
which model is `holdings::Holdings`: the scene's composed weapons are read
once, and from then on weapons only change hands, so a world starting over
in the same scene (a joined player's swap to the host's world) gives each
character what it held, never a retired model. Dumps show it (`armory`:
characters armed again, those that drew, the last load's time).

A creature whose weapons change as it stands in the scene (not as it is
first seen there) plays its new wield's draw, `g<wield>w1`
(`acting::Draw`), view-side, its weapon in hand from the clip's start.
That is the K1 exe's equip handler: as a weapon slot's item changes it sets
the slot, works out the wield, and plays its animation 10246, which its
table of wield types turns into `animations.2da` rows 91, 132, 173, 214,
222, 236 and 244 (`g1w1` … `g7w1`), on a body that has the clip (K1's
humanoids lack `g3w1` and `g4w1`: a two-handed sword or a double blade is
drawn with no clip); an unarmed body has none, so an unequip that leaves the hand empty plays
nothing. The exe attaches the weapon by no clip event (the clips'
`draw_weapon` is in no handler; its `equip` and `unequip` are the weapon
model's own clips, played as it is hung or taken off). The sim's action
does not wait for the draw. Not here: a draw at a fight's start (the exe
plays none found), the handler's two skip conditions [verify], players'
characters (they draw as JA's do, `holster`).

```bash
kjk play --stage end_m01aa --class master --moves '300ms; 200ms attack; 1s; 1200ms alt; 4s'
```
`master` is the dev Jedi who also throws its saber (`alt`); a class's
`:N` sets its Jedi Defense tier (`--class jedi:3`).

`kjk play --replay FILE` draws a recording on its own stage and seats
instead: a probe's bout (`kjk combat duel` writes the bouts it keeps
beside its facts, on the probes' floor, the `ring` stage) or a session's
`inputs.kjkr` made off a module; `--seat N` watches it from seat N's
player (the other side of a bout). With `camera`, `film` (docs/RUN.md)
and `cargo xtask duo --film S --camera '…'` it films a fight.
