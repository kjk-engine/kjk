# Scripts (I16; `script_ncs`, `fmt_odyssey::ncs`, `sim::script`)

KOTOR's logic is NWScript compiled to NCS, run by the engine's VM. We run
the games' own compiled scripts; nothing of them is in the repo.

## Bytecode and routines
- `fmt_odyssey::ncs`: `NCS V1.0`, `0x42`, length, then big-endian
  instructions (45 opcodes) as typed `Op`s; written back byte for byte.
  Every `.ncs` of every layer: K1 12 893, TSL 5 120.
- `script_ncs::catalog`: routines (numbered as `ACTION` names them),
  constants and engine structures from the install's `nwscript.nss`
  (K1 772 routines, TSL 877; K1 spells one prototype `INT`/`OBJECT_ID`).
  Some calls pass fewer arguments than the prototype (older prototypes:
  K1 `ActionStartConversation` 11 of 12) or more.

## The VM (`script_ncs::vm`)
- Typed cells (shared strings; a vector is three floats), a base pointer
  at the globals' end, a return stack. `ACTION` pops the prototype's
  arguments (first on top), fills missing ones from defaults (OBJECT_SELF:
  the caller), drops extras. `STORE_STATE` saves globals, top locals and the
  resume point (16 bytes on) for an `action` argument; `resume` runs it.
- Strings compare without case (`EQUALSS`, `NEQUALSS`): K1's scripts
  test tags both ways (`GetTag(o) == "t3m4"` in the party include, the
  data's "T3M4"; `!= "Bastilla03"`), so the game's compare must ignore it
  [verify in the game]. `GetTag` gives the data's capitals, an object's
  and an item's alike (`ItemFacts::cased_tag`; the store's templates
  keep it, `templates-v9`).
- Faults end the run: underflow, wrong type, bad jump, division by zero,
  more than 131 071 instructions. About 13 ns an instruction.
- 131 071 is KOTOR's own limit (its machine stops a run at the next
  instruction), and it stops the games' endless loops: k_ai_master's
  commoner search never steps past a hostile-faction creature its
  commoner regards as neutral beyond 20 m (a Dreshdae citizen while the
  medallion's murderer fights the player 24 m off). Such a stop is the
  game's, so the reports list it apart from the faults, as a runaway
  (the sweeps among them). KOTOR counts a run and those it starts
  (`ExecuteScript`) together; here each counts its own.
- A routine handed an argument of another type than its prototype's ends
  the run too, as KOTOR's machine ends it (Q-0324: TSL's floating mines,
  whose compiled `break` leaves a local on the stack). That is the game's
  error, so the reports list it apart as well, as aborted (not in the
  sweeps' faults).

## In the sim (`sim::script`, `world::scripts`)
- A module's plan carries a `Library` (`Level::with_scripts`, in its
  digest): the scripts its module, area and objects hook, and those they
  name in strings (`ExecuteScript`), with the catalog.
- Authority only, last in the step; randomness from the authority stream.
  `State` (globals, objects' indexed locals, events waiting, deferred
  code) is not in snapshots: a save carries it; an install carries it on
  and drops what the module left waiting.
- The dead act on nothing (as reone's object update does): a dead character's
  queue does not move (it waits, should the character rise), a scene's
  round not yet begun goes with its attacker's or its target's death, and
  no scripted animation (queued, at once, a DLG gesture, a scene shot's
  reaction) takes the place of its death's clip, nor do K1's
  self-queuing idle loops (the Taris drunks' `k_ptar_drunk_ud`) or the
  Endar Spire's bridge and duel rounds.
- Location, effect and spell event values (`EventSpellCastAt`) are
  handles into tables in `State`, so deferred code and a save keep them.
  Once a table holds over 1024, the step's scripts start by dropping the
  values no deferred code holds (before any runs: a running script's
  stack holds values too).
- Effects (`script::effects`): every `Effect…` makes a value the scripts
  keep (a link its parts; one the sim does not carry out yet names its
  routine). Applied, they wait for the next step's damage pipeline: at
  once damage (script hits, never filtered), death, heals and FP; lasting
  modifiers (abilities, defence, saves, attack, damage, resistance and
  immunity by type, immunities, speed, extra attacks, skills, Force
  resistance) in 24 slots on the character (`combat::effects::
  Lasting`, read by `sheet_now`); holds (stun, paralysis, fear, sleep,
  choke, droid stun, whirlwind) as C11's unless an immunity keeps them
  off; visual effects change nothing. `GetFirst/NextEffect`,
  `GetEffectType`, `…Creator`, `…SpellId`, `RemoveEffect`,
  `ClearAllEffects` (in the scripts' order), `GetHasSpellEffect`;
  `SetEffectIcon` keeps the effect (the icon is the views'). An effect
  applied in a power's impact, or in code the impact deferred, carries its
  spell row (the deferred code runs as the impact: a save carries the cast
  with it, KJKV 8). `kjk script effects k1`: 8 checks.
- KOTOR's states (TSL's executable's `SETSTATE` effect,
  `combat::effects::state`): `EffectCutSceneParalyze`, `…Stunned`,
  `…Horrified` (the plain state with its cutscene flag: no immunity but
  sleep's keeps it off, no 3 s cap, no immune second), `EffectCrush`
  (15), `EffectDroidConfused` (16), `EffectMindTrick` (18) and
  `EffectDroidScramble` (19) last in a slot (`Modifies::State`). Refused
  by their link's immunities (`gameeffects.2da`'s row for the state: a
  mind trick by mind, stun or dazed immunity, a scramble by stun or dazed)
  and their own (a mind trick's mind immunity, a droid's confusion's 33),
  never a cutscene's. The highest running state holds (`ForceState::state`,
  settled each step, `world::settle_effects`): its taking hold clears its
  bearer's actions and shows its visual for its time (stun, mind trick,
  scramble: 2002; a droid's stun 1007), its end runs the end-of-round
  script (states 1, 3 to 10, 14 to 19); all but confusion and fright hold
  it still. A mind trick or scramble breaks on any damage, another's attack
  reaching it, and a hostile it perceives within √10 m not in stealth.
  `GetEffectType` reads the executable's table (24, 25, 35, 29, 27, 30, 79,
  80, 81; 0 for choke, horror, whirlwind, crush), `GetIsLinkImmune` a
  state's link. The plain holds stay C11's (their K1 and TSL callers'
  timings stand; [verify] moving them onto the state with the chains);
  each shows its state's visual while it lasts as the state does
  (`Hold::state`, drawn by `script_fx::new_holds`): a stun 2002 (its
  `v_imp_stun`), a droid's stun 1007, a paralysis, horror, sleep,
  whirlwind or choke (`EffectChoke`, the gas traps': no immunity keeps it
  off) nothing (so `k_ptar_dueldlg`'s second of paralysis, which
  holds the player while Taris's duel announcer begins, is silent). A
  paralysed body's clips stop where the hold found it, and only while the
  hold lasts (`cast::held_since`).
- TSL's other effects: `EffectBlind` (it neither sees nor hears: its
  perception skipped; the 50 % miss chance and visual 5002 not kept
  [verify: who rolls it]), `EffectTrueSeeing` and `EffectPsychicStatic`
  (kept only: nothing to see through; inert in the executable),
  `EffectFury` (its bearer's weapon kills counted from 0, each +1 damage
  up to 6, `Effects::fury`), `EffectForceSight` and Fury's video rows on
  the controlled player's view (4; 7 to 9 by spell row,
  `Snapshot::video_effect_for`), `EffectForceBody` (a cast pays 50 / 40 /
  30 % of its cost from FP and as much from HP, castable only with more
  than one HP left after, `combat::effects::pay`), `EffectBodyFuel` (FP
  full each combat round's end, timed from its start [verify]),
  `EffectFactionModifier` (its bearer, never a player, of an NPC faction
  while it lasts, actions cleared; back as it ends, Force, Beast and Droid
  Confusion's globals set to 0), `EffectVPRegenModifier` (natural healing
  doubled per one, QUIRKS.md Q-0242), `EffectFPRegenModifier` (kept for
  TSL's FP regeneration, not yet kept), `EffectRegenerate` (its HP every
  interval from its start; the interval within 32.7 s). `RemoveEffectByExactMatch`
  takes off the first lasting effect of the kind and first two numbers
  given, nothing else compared; `GetIsImmune` reads immunity effects and
  worn items alone (an immunity to all counts; no feats, no mind umbrella;
  types 1 to 33).
- TSL's Mind Trick, Confusion and Fury, where their impact scripts run,
  are their scripts' (TSL's rules over JA's toggles, the owner's call): the
  one aimed at (`world::powers`, the crowd control's reach) or the caster
  takes the cast; JA's mind trick and rage stand in for K1's rows and a
  stage without scripts. Force Sight keeps JA's sight.
- Visual effects (`EffectVisualEffect`, `EffectBeam`) applied to a
  character become events on its fighter (`Outcome::Visual`,
  `combat::fighter::Visual`: the `visualeffects.2da` row, how long, a
  beam's source in `by` and body part), for the views; `ApplyEffectAtLocation`'s at their place
  (`Event::at`), on the caller's character (a module's or area's own at a
  place: not yet).
- Minigames' routines (`SWMG_*`) go to `script::minigame::call` first
  (`MINIGAMES.md`: the 25 K1 calls and their families, TSL's own
  (`SetJumpSpeed`, `PlayerApplyForce`, `DestroyMiniGameObject`,
  `GetSwoopUpgrade`, `GetTrackPosition`, `SetFollowerPosition`), the
  course's hooks as events); routed by name, they are not in the bound
  count below. `SWMG_SetPlayerInvincibility` and
  `SWMG_GetPlayerInvincibility` stop their script, as the executables do
  (QUIRKS.md Q-0257): no fault. The game's clock: `GetTimeHour`, `…Minute`, `…Second`,
  `…Millisecond` from sim time and the IFO's `Mod_MinPerHour` (2 where
  unsaid; the day from hour 0 [verify]); `EndGame` loses the game with its
  death screen, else ends it (`WORLD-STATE.md`, "The game's end");
  `FloatToString` with no decimals keeps its point ("4.": the swoop's
  heartbeat cuts it off) [verify in the game]. `CutsceneAttack` (C16): the
  attacker faces its target for the round, a hit's damage applied so; a
  gunner fires it (`EVENTS.md`, scripted blows), at a door or placeable
  too, aimed 0.6 m above where it stands [verify] (the Endar Spire's
  room 5: a Sith shoots the fuel cylinder its script then blows up).
  `EffectForcePushed` and `EffectForcePushTargeted` applied outside a
  cast's impact script (a scene's blast, a story's shove) knock their
  target down and throw it away from the location given, else from whoever
  applied it, at JA's first-level push strength (`world::pushed`; K1's own
  delivery unread [verify]); in a cast's impact the shove is the power's or
  grenade's own delivery and the effect does nothing more. While a conversation
  holds the world, a party member's attacks, powers and throws are its
  `CutsceneAttack`'s alone (scene mode's S4, `scene::ROLLS`: KOTOR rolls
  no party attack in a conversation; that this takes in powers and throws
  is [verify]).
- Items, inventories, equipment and gold (`script::items`, I19):
  `WORLD-STATE.md`.
- Names (the whole-game sweep's most reached gaps): a plan reads the
  install's talk table (`dialog.tlk`, `vfs::resolver::TalkTable`, read
  once a base). `GetModuleName` is the IFO's `Mod_Name` as the game shows
  it (K1's own text, `end_m01aa`; TSL's talk-table line, `101PER`: TSL's
  scripts compare it); `GetName` an object's (a creature's first and last
  names, another's `LocName` or `LocalizedName`), the area's `Name`, an
  item's; a player's character has none yet. `GetStringByStrRef` reads
  the talk table (in the library's digest).
- TSL's number globals hold a signed byte: `IncrementGlobalNumber` and
  `DecrementGlobalNumber` past −128 or 127 leave the global as it was
  (the game warns). `RemoveHeartbeat` takes an object's heartbeat away
  (kept in the scripts' state, KJKV 12). `GetObjectPersonalSpace` is a
  creature's `appearance.2da` `perspace`.
- Locals (`Get/SetLocalBoolean`, `…Number`) are each object's; the
  module's and the area's are each module's own (their ids are one across
  modules). `ResetDialogState` keeps nothing.
- `ChangeToStandardFaction` sets the fighter's faction;
  `ChangeFactionByFaction` moves every non-party character of one
  faction to another (Korriban's academy turning on a spy);
  `AdjustReputation` changes how two factions stand, both ways (a door's
  or placeable's own `Faction` counts: Tatooine's "Sandperson Faction"
  placeable); `GetIsDead` of no object is TRUE, as Aurora's; `CancelCombat`
  leaves it with no one to fight and out of combat; `SurrenderToEnemies`
  makes the caller neutral and cancels its combat (Calo Nord's surrender
  before his detonator scene); `SetMinOneHP` sets the
  sheet's flag (the sheet rides in the snapshot from then on).
  `SetLightsaberPowered(who, override, powered, transition)` lights the
  character's blade when both `override` and `powered` are set and puts it
  out otherwise (the game then decides again, and out of a fight that is
  unlit [verify]); a lit blade shows the saber in hand outside a fight
  (`weapons::armed`), growing as any blade does (Darth Bandon, unarmed
  until `ActionEquipItem` takes the double-bladed saber from his pack,
  half a second later lit, in the Endar Spire's last stand).
  Alignment is the sheet's too (the template's `GoodEvil`, 0 dark to 100
  light; a made character 50): `GetGoodEvilValue`, `GetAlignmentGoodEvil`
  (light from 70, dark to 30), `SetGoodEvilValue`, and `AdjustAlignment`
  (toward a side, or toward 50 and no further; TSL's party others move
  with the player's character unless its fourth argument is TRUE).
  `GetLastHostileActor` is the last to attack the object or cast a harmful
  power at it, never a dead or destroyed one. `GetPartyLeader` is
  party member 0: the companion made leader, else the first seat's
  character (`WORLD-STATE.md`: each seat still moves its own body); `GetUserActionsPending` is
  always false (a seat's commands act at once, never queued: C14).
- Skills: TSL's `GetSkillRank` is the rank as its executable reads it
  (`CSWSCreatureStats::GetSkillRank`): the stored rank with its effects,
  worn items and feats, plus the key ability's modifier (`skills.2da`'s
  `keyability`, `stealth::total`); 0 for a skill only the trained use that
  the character has no ranks in (Stealth counts one rank with Force
  Camouflage known), 0 for a skill it has no row for, −1 for anything not
  a character. `GetSkillRankBase` is the stored rank alone. K1's stays the
  rank its sheet holds as its effects leave it, −1 for anything not a
  character or a skill it has no row for [verify K1's executable: TSL's
  adds the ability]. `GetHasSkill` a rank above 0 (reone, xoreos).
  Computer terminals: the games' own scripts price and take spikes
  (`DIALOGUE.md`, "Computers").
- Movies and module changes (I20): `PlayMovie` and `StartNewModule`'s
  `sMovie1..6` are told to every seat as a screen (`SOUND.md`, "Movies");
  `StartNewModule` asks for the module change as a door's link would (the
  first seat asking, for everyone), arriving at the tagged waypoint.
- Fades: `SetGlobalFadeOut` and `SetGlobalFadeIn` (wait, length, colour)
  set the screen's fade everyone sees (`snapshot::Fade`, KJKS 34): from
  the call the screen holds as covered as it was for the wait, then
  ramps from there to the colour (out) or from there to clear (in) over
  the length, and stays so: a fade in over one already lifting goes on
  from where the screen is, never back to black (xoreos restarts it from
  the colour: a flash the owner saw at the Dantooine council's end, where
  `k_pdan_vandar01`'s deferred fade in follows the montage's first
  line's). The game's HUD hides while a fade covers any of the screen
  (`ui_view`), as it does in a conversation. The engine lifts a covering fade (an out) of its
  own accord, fading in from where the screen is over 1 s [verify the
  length]: a module loads under black (the "LevelLoad fade") and, once
  its arrival's scripts have run, that or their own fade out is lifted
  (`snapshot::WorldFadeIn`; `end_m01aa`'s own fade in, a 3 s hold, stands;
  `tar_m03af`'s `k_ptar_postswoop` cut to black after a race is lifted);
  `HoldWorldFadeInForDialog` there defers it to a conversation's start
  (`ebo_m41aa`: black until Carth speaks); a conversation starting (not
  a bark or a one-liner) lifts one too (`k_ptar_postrace`'s fade to
  black before Brejik's scene). TSL's `SetFadeUntilScript` locks the
  fade standing: only `SetGlobalFadeIn` lifts it. [verify in the game]
  the moments: after the arrival's scripts, at a conversation's start.
  DLG nodes' own fades (`FadeType`): `DIALOGUE.md`, "Fades"; scripts and
  nodes set one fade, the last set wins. The game draws it over the scene, under the HUD and
  the conversation's bars, darkening as the game's blend on
  gamma-encoded values does (`fade_view`).
- Video effects: `EnableVideoEffect` (both games) keeps a
  `videoeffects.2da` row on everyone's screen until `DisableVideoEffect`
  (`Snapshot::video_effect`, KJKS 59; kept across modules, as the
  routine's "apply indefinitely" reads [verify]). Neither reone nor xoreos
  implements them; the columns are read by name and by TSL's executable,
  which names `filmnoisetex` and carries the grey's channel weights
  (0.3086, 0.6094, 0.0820). `enablesaturation` pulls each pixel toward its
  grey by `saturation` and multiplies by `modulationred`/`green`/`blue`
  (TSL's `_pc` columns), K1's security camera and T3-M4's and HK-47's
  freelooks blue or red; `enablescannoise` adds the procedural film noise
  (`filmnoisetex.txi`: random channels scaled to 0.02). The launcher
  grades the scene camera's frame in its stored values, before the fade
  and the screens (`video_effect`); the noise's scale on the screen and
  its order after the grade are [verify]. TSL's Force views
  (`enableclairvoyance`, `enableforcesight`, `enableclairvoyancefull`,
  `enablefury`) are carried but not drawn. TSL's prologue (`001ebo`)
  turns on T3-M4's freelook (row 1) as it opens.
- Animations (`asset_odyssey::creature::SCRIPTED_CLIPS`): the scripts'
  `ANIMATION_*` numbers are the same in both games (K1's stop at 32 and
  120), so one table names their clips; a body without the clip plays
  none. reone names most; the rest come from `animations.2da`'s rows
  whose name or description match, checked against the humanoid
  supermodels' clips (`s_male02`, `s_female02`…): `DEAD_PRONE` (30)
  `dead3`, the dead loop added with `KNEEL_TALK_ANGRY` and `_SAD` (31, 32:
  `kdtlkangry`, `kdtlksad`, rows 373–385 [verify]; TSL's medbay body lies
  on its bed in it); `SIT_AND_MEDITATE` (35) `meditatesit`;
  `MEDITATE_STAND` (46) `meditatestand`; `CHOKE_WORKING` (45) `choke`
  (TSL's looping choke, 116 being the fire-and-forget one too);
  `PAUSE_SCRATCH_HEAD`, `PAUSE_BORED` (102, 103) `pausesh`, `pausebrd`;
  `DIVE_ROLL` (123) `diveroll`. [verify]: `SIT_CHAIR` (36) `sit` (rows 21
  `Sit_Loop` and 456 `Sit_To_Meditate` share the name, the models hold
  one), `SIT_CHAIR_DRINK` (37) `drink` and `SIT_CHAIR_PAZAK` (38)
  `cardplay` (no description; no model holds them), `RAGE` (41)
  `forcerage` (a fire-and-forget row), `THROW_HIGH`/`LOW` (117, 118)
  `throwgren`/`throwgren1`, `FORCE_CAST` (121) `castout1`, `GET_LOW` and
  `GET_MID` (10, 11) the pick-ups `getfromgnd` and `getfromtbl` (rows 40,
  41), `PAUSE_DRUNK` (13) `pausepsn` (row 15 `Idle_Poisoned`, NWN's name
  for it). `READY` (19) is the body's wield's ready stance (`g<wield>r1`,
  a creature model's `creadyr`, as reone; drawn by the view,
  `game::acting::stance_of`). Where no row matches, both executables'
  `PlayAnimation` (read off the Mac builds) still picks one: `WORSHIP`
  (4) meditates, `VICTORY2`/`3` (109, 110) are `VICTORY`, and `CUSTOM01`
  (119) plays row 346's `usecomp`, which only the Taris rancor's model
  holds (it eats from the bone pile so). `CHECK_BODY` (33),
  `SIT_CHAIR_COMP1`/`2` (39, 40) and `SCREAM` (124) play nothing. The
  `kdtlk*` clips are in no model of either install. A creature model
  (`appearance.2da` `modeltype` S or L) is asked for each clip by its
  creature name first (`cpause1`, `cdie`, `cdead`, `cvictory`), as reone's
  `getFirstIfCreatureModel` asks, then by the shared name where its chain
  lacks that (Q-0136; `asset_odyssey::creature::creature_model`).
- Locks (`script::locks`): `GetLocked`, `SetLocked` (doors and
  placeables), `GetLockKeyRequired`, `GetLockLockable`, `GetLockUnlockDC`,
  `GetLockLockDC`, `GetLastUnlocked`, `GetLastLocked`, `ActionUnlockObject`,
  `ActionLockObject`, `GetIsDoorActionPossible` and `DoDoorAction` (open,
  unlock where the caller's Security reaches, and bash what blows may
  break; knock not yet): `WORLD-STATE.md`, Locks.
- The combat AI's (`k_ai_master`, `k_inc_generic`; the playtests' most
  reached gaps): `GetLastAttackAction` is `ACTION_ATTACKOBJECT` for a
  creature's last attack (a blow, a combat feat's too) and
  `ACTION_CASTSPELL` for a Force power, noted as the queued attack or
  talent begins (`ACTION_INVALID` before any); `GetLastForcePowerUsed`,
  `GetLastCombatFeatUsed` their rows (−1 for none, a plain blow none).
  `GetTotalDamageDealt` is the hit points the caller's last damaged event
  took. `GetSpellTarget` is whom the creature's power under way is for,
  else whom its last power reached (companions turn on the leader's).
  `ActionMoveAwayFromObject` queues a run to a place `fMoveAwayRange` from
  what it flees, picked as it is asked for: straight away, else turned
  30° at a time, else half as far, the first a way leads to; nothing when
  it is that far already. `ActionFollowLeader` runs a companion back to
  its following distance of the leader (its mind follows on); others
  stay. `ActionEquipMostDamagingMelee`, `…Ranged` equip at once the
  weapon with the best average blow (dice and damage bonus) of that kind
  the caller wears or carries, else of the other kind. `EffectAssuredHit`:
  `EVENTS.md`. `kjk script fighting k1`: 9 checks. `PlaySound` is heard
  at the caller by every player (`SOUND.md`, "Scripts' sounds").
- TSL's own (the 2026-10-17 sweep): `GetIsXBox` is false (the PC build:
  TSL's tutorials pick their mouse-and-keyboard lines by it), the rumble
  routines do nothing, `SetForceAlwaysUpdate` and the AI levels change
  nothing (every object and mind is stepped every tick here),
  `EnableRendering` on changes nothing (off is still a gap: no body is
  hidden yet), `RemoveEffectByID` takes off the lasting visuals of that
  `visualeffects.2da` row (`k_fab_fade_sp`'s fade-in shimmer; [verify]
  other effects' IDs), `EffectDisguise` lasts as a `Modifies::Disguise`
  (drawn as that appearance, read by `GetAppearanceType`),
  `HasLineOfSight` traces sight between the two places as given (the
  executable adds no height: scripts pass feet), 0.1 m clear of the floor
  they stand on, through the doors as they stand (`View::sees`; [verify]
  that its walkmesh and rooms' line-of-sight faces block as our solid
  geometry does), and `GetRandomDestination` picks as the executable does
  (`script::random_destination`): whole metres off along each axis within
  the range, on the floor there; while a body cannot walk straight to it
  (`View::walks`, `walk::straight`), three quarters as far, and under
  2.5 m turned a quarter either way or back at the first length (one in
  three each), 22 lines at most, given up under 1 m: the creature's place;
  (0, 0, 0) for one not a creature (QUIRKS.md Q-0249: a range of 0).
  `SetHealTarget` and `GetHealTarget` are a creature's only.
- TSL's party routines (`script::party`, read from TSL's exe): influence
  (`GetInfluence`, `SetInfluence`, `ModifyInfluence`) is one per
  companion of 12, set or moved within 0 to 100. It starts at
  `influence.2da`'s base (50) when the companion is made available
  (Kreia's -1 until set). It drives the companions' alignment, and TSL's
  `AdjustAlignment` moves only the one named (`WORLD-STATE.md`, "The party
  table"). Puppets
  (`AddAvailablePUPByTemplate`, `…ByObject`, `AssignPUP`,
  `SpawnAvailablePUP`, `AddPartyPuppet`, `GetPUPOwner`, `GetIsPuppet`,
  `SavePUPByObject`, `ActionFollowOwner`) are made available, given to a
  companion, made and joined, and a puppet follows its owner until its
  queue is cleared (`Action::FollowOwner`, `EVENTS.md`); `SetForfeitConditions`
  rules are broken by equips, item uses and a player's Force powers
  (`WORLD-STATE.md`, "Forfeit conditions"); `CancelPostDialogCharacterSwitch`
  has nothing to cancel (a companion's player speaks through it; TSL's
  hand-over to the player's own character for an object whose
  `PartyInteract` is off is not made here [verify]); `DisableHealthRegen` holds off the party's
  natural healing (`world::powers`: each `regeneration.2da` `TimePerHP`,
  the out-of-combat or in-combat base, the Constitution and Treat Injury
  (a droid's Repair) bonuses, more with the Regenerate feat);
  `SetOrientOnClick` false keeps a creature from turning to whoever talks
  to it; `IsMeditating` is false (only the meditative stance, which no
  action here takes, sets it).
- TSL's `ActionSwitchWeapons` (the party AI's `GN_CheckRangedPreferred`
  in `k_inc_generic`, compiled into 63 scripts: in Ranged Support it
  swaps to the second set's blaster when not hit by melee lately, to its
  blade when it is): queued, and when it comes to the front the caller's
  whole queue is cleared (what was queued after it too), its weapon sets
  exchanged at once (`WORLD-STATE.md`, "The second weapon set") and, in
  combat with a target, an attack on that target queued again, as the
  executable's `AIActionSwitchWeapons` does; no animation, no wait. A
  companion a seat drives takes no swap from its own AI. `GetItemInSlot`
  reads slots 18 and 19 (`INVENTORY_SLOT_RIGHTWEAPON2`, `…LEFTWEAPON2`).
  `kjk sim probes --only "weapon sets"`.
- Casts, skills and surrenders (TSL's executable, both games' scripts):
  `ActionCastSpellAtObject`, `…AtLocation`, `ActionCastFakeSpellAtObject`
  and `…AtLocation` queue a cast (`Action::Cast`; a row past `spells.2da`
  or a path type past 4 queues nothing; a fake one needs its caller
  commandable no more than knowing the power): the caster turns to its
  target and holds its hand out for the row's `conjtime` + `casttime`;
  then a real one is paid (Force Body's split, at its adjusted cost, what
  it lacks by then forgiven; none for `bCheat`) and its
  impact script runs at the target, a fake one does nothing more. A real
  one by a player, by one not commandable, or of a power it cannot cast
  (but a cheat's), does nothing. The row's own animations, visuals and
  projectile are not drawn [verify]. `ActionSurrenderToEnemies` queues the
  surrender `SurrenderToEnemies` makes, when it is reached; neither does
  anything for a player's character (the executable). `ActionUseSkill`
  acts at once, as the executable's: Stealth turns stealth mode off, or on
  where the player's stealth key would (ranks, a stealth unit, out of
  combat); the other skills are a counted gap. `FaceObjectAwayFromObject`
  turns its facer at once. `GetLastWeaponUsed` is the item its last attack
  that reached its target struck with (its right hand's, else its own
  weapons', else its gloves'; [verify] an off-hand blow), kept as the
  attacked event is raised; a door or placeable struck runs its
  `OnMeleeAttacked` first (the Peragus medbay door's plasma check).
  `GetBlockingDoor` is the shut door a creature's walk ran into, as its
  `OnBlocked` runs (`EVENTS.md`); `GetBlockingCreature` and
  `GetFoundEnemyCreature` are none: the walker steps round bodies itself,
  so `k_def_pathfail01` never runs. `GetInventoryDisturbItem` is the
  item last put into the container (one taken out is not kept).
- TSL's combat and Force routines (`script::force`, `script::traps`; its
  executable's commands, `context/artifacts/i35/combat-force.md`): the
  spell book (`GetSpellBaseForcePointCost`, the row's `forcepoints`;
  `GetSpellFormMask`; `GetSpellAcquired`, known whatever it costs, where
  `GetHasSpell` asks it be paid for at its adjusted cost; `GrantSpell`,
  known from now), the Forms (`IsFormActive`, a lightsaber Form only with a
  lightsaber in the right hand; `SetCurrentForm`, a power it knows;
  `COMBAT.md`, "TSL's Forms"), `IsInTotalDefense` (no stance is ever taken
  here: false), bonus Force points (`AddBonusForcePoints`,
  `SetBonusForcePoints`, `GetBonusForcePoints`: the sheet's
  `BonusForcePoints`, added to a Force user's most FP, told to a player's
  character in the status summary as its executable does, QUIRKS.md
  Q-0247), the story's stat changes on the stored sheet
  (`AdjustCreatureAttributes`: the score a byte that wraps, Q-0250, a
  Constitution change moving the most and the current hit points, a
  Wisdom one a Force user's most FP, by the modifier's change a level;
  `AdjustCreatureSkills`, none below 0, Q-0251;
  `Modify…SavingThrowBase`: the sheet's `fortbonus`, `refbonus`,
  `willbonus`, read from templates too), `GetSkillRankBase`,
  `GetRacialSubType` (−1 for an object not a creature, 0 for none),
  `IsRunning`, `GetCombatActionsPending` (anything in its queue), fake
  combat (`SetFakeCombatState`: out of a real fight, held until let go;
  weapon out, no combat round, in combat to `GetIsInCombat` unless its
  second argument asks for real combat; `Fighter::fake_until`),
  `CreatureFlourishWeapon` (once a second: the blade lit, fake combat for
  8 s), `ForceHeartbeat` (its heartbeat script at once, its period from
  then: `State::beats`), mines (`GetOwnerDemolitionsSkill`, the
  Demolitions its setter had; `SpawnMine`, a mine of a `traps.2da` row at
  a place, its DCs the row's and those given, the creator's or hostile,
  seen by no one; `DetonateMine`, taken away, the blast its caller's
  script's) and `EffectFPRegenModifier` (`COMBAT.md`).
- Presentation and flow (I35, `script::presentation`, from TSL's
  executable and K1's where it has the routine): `RoundsToSeconds` is `n`
  × 3.0 (not the nss's 6.0: QUIRKS Q-0238), `TurnsToSeconds` ten rounds,
  `HoursToSeconds` the IFO's minutes; `AngleToVector` the unit vector of
  degrees; `GetGameDifficulty` 1 (Normal) until a difficulty option
  exists; `IsMoviePlaying` 0 (the games' movies hold the script). TSL's
  `QueueMovie` keeps the movie and its own skip in a queue no module
  change or save touches (the client application's), queueing `MalMov12`
  or `MalMov14` raises `GBL_GAME_COMPLETE` by one; `PlayMovieQueue` tells
  every player the queue in order as one run and empties it (its
  `nAllowSkips` never read: Q-0237; `SOUND.md`, "Movies"). `NoClicksFor`
  holds off a click on a placeable and a click to talk (doors not) until
  its game time is up (`world::uses`; not saved). `ActionBarkString`
  queues the caller's bark (`Action::Bark`), refused while a conversation
  holds the world; `BarkString` is dropped then too (both games'
  executables). A conversation beginning takes its owner and listener out
  of stealth (both games) unless TSL's `SetKeepStealthInDialog` holds (not
  saved). `DisplayFeedBackText` floats its `feedbacktext.2da` line over the
  object for 5 s (`Progress::feedback`). `ShowTutorialWindow` shows a
  `tutorial.2da` row's window once (`Party::tutorials`, the exe's
  `PT_TUT_WND_SHOWN`; Q-0239), not while a conversation holds the world,
  not where `Message_PC0` is blank; each player's "Tutorial Popups" option
  (`feedback.tutorials`, on) keeps it off its screen. TSL's `DisableMap`
  jams the map (`Party::no_map`, saved), `SetDisableTransit` sets the
  area's `DisableTransit` (saved, else the ARE's), `SetReturnStrref` the
  map page's return button (`k_sup_guiopen`, which the host runs each
  time a seat opens an in-game screen, `Action::ScreenOpened`, sets it
  anew; KOTOR does not save it, ours does, as the snapshot carries all
  three: `Screens`). K1's `ShowTutorialWindow` takes a window number, 0 to
  2, for its rows 9, 40 and 42 (its executable's table). `ChangeObjectAppearance`
  gives a creature not in a disguise an appearance for good and stands it
  where its body fits; `DuplicateHeadAppearance` the head of the other's
  appearance (`appearance.2da` `normalhead`); both ride the fighter
  (`combat::Look`) and `GetAppearanceType` reads the first.
  `PlayOverlayAnimation` plays only 123, the dive roll (`animations.2da`
  `diveroll`, the exe's overlay id 10509 mapped), at once.
  `YavinHackDoorClose` shuts a door at once, no event; TSL's
  `SoundObjectSetPosition` moves a sound object, `SoundObjectGetPitchVariance`
  reads its UTS's `PitchVariation`; `DisplayDatapad` shows the item's
  `DescIdentified` in a message box to every player and sets its local
  boolean 94 (no pause). `SetInputClass`, `UnlockAllSongs`, `PauseGame`,
  `EnableRain` and `ShowDemoScreen` change nothing (a pause is a player's
  own here: the co-op break is never a script's).
- The UTE encounters' routines, both games (`COMBAT.md`, "UTE
  encounters"): `GetEncounterActive`/`SetEncounterActive` (made active, it
  spawns at once if a qualifying creature already stands in it; not
  un-started), `Get/SetEncounterSpawnsMax` (`Respawns`),
  `Get/SetEncounterSpawnsCurrent` (resets done),
  `SetEncounterDifficulty` (`encdifficulty.2da`'s value for the row; no
  table 0, a row past it no change) and `GetEncounterDifficulty` (the
  value); an object that is no encounter reads 0 and takes nothing.
  `GetIsEncounterCreature` is 1 for a creature an encounter made while its
  link stands (not across a save, QUIRKS.md Q-0261); `GetEnteringObject`
  in an encounter's script its last to come in; `GetFirst/NextInPersistentObject`
  on an encounter the area's objects of the type over its outline.
  `kjk sim probes --only "ute encounters"`.
- What runs them, and the action queues: `EVENTS.md` (I17). 503 routines
  bound in K1 and 598 in TSL by `CreateItemOnFloor`, as `kjk script coverage` counts them,
  then `SetItemNonEquippable` (a player's equip of the item refused; one
  worn stays worn [verify]) and `YavinHackCloseDoor` (the door shut at once,
  no walk to it)
  (globals, locals, objects by tag, party, positions, locations, HP,
  deferral, events, actions, perception, factions and characters, the
  combat AI's, effects, items, sounds, movies, fades, module changes,
  conversations, locks, random and dice, strings, names); every other
  routine is a gap: its type's empty value, counted.

```bash
kjk script check k1                     # every script decodes, writes back
kjk script dis k1 k_pend_door02 --module end_m01aa
kjk script run k1                       # all of them on stand-in routines
kjk script probes                       # the VM's probes (test script_vm)
kjk script coverage k1 end_m01aa --seconds 20 [--trace]
kjk script impacts k1                   # k_sp1_generic per power, recorded
kjk script sweep k1 [--seconds 10]      # every module: gaps ranked, faults
kjk script effects k1                   # the effects engine end to end
kjk script items k1                     # items end to end (WORLD-STATE.md)
kjk script equip k1                     # taking and equipping end to end
kjk script stores k1                    # stores end to end
kjk script traps k1 [module]            # mines, Awareness, Demolitions end to end
kjk script party k1                     # the party table end to end
kjk script security k1 [module]         # locks and Security end to end
kjk script journal k1                   # the journal end to end
kjk script xp k1                        # experience end to end
kjk script levels k1                    # levels end to end
kjk script routines tsl 101per          # the sweep's most reached, end to end
kjk script fighting k1                  # the combat AI's routines, end to end
```
