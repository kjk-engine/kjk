# Objects' events and action queues (I17; `sim::script`, `world::scripts`)

What makes the games' scripts run, and what their `Action…` calls do.
Authority only; the state (queues, who did what last, trigger occupants)
travels in saves and recordings (`script::State`).

## Events (`world::scripts`)
- **Install**: on a module's first install each creature's `ScriptSpawn`
  in the module's order; then `Mod_OnModLoad`; per player by seat
  `Mod_OnClientEntr` and the area's `OnEnter` (entering: the player). What
  the left module had waiting is dropped.
- **Leaving**: as the party goes (the ready check's ask through an exit,
  or a script's `StartNewModule`), per player the area's `OnExit`, then
  `Mod_OnClientLeav` (`GetExitingObject`: the player), in the step it
  goes, while its module is still there (as xoreos leaves an area; `script::
  leaving`). An ask asked again runs them once.
- **Heartbeat** every 6 s: area and module at the period's start, each
  object at its own moment (by id), not the dead.
- **Triggers**: any living character coming into or leaving one runs
  `ScriptOnEnter` / `ScriptOnExit`; a trigger that leads out runs no enter.
  In is over its outline (even-odd, any shape) and on its floor: of the
  walkable floors over the spot, the one nearest the outline's points'
  heights (a floor alone there is its, however high the outline floats);
  off the navigation mesh, within 2 m of the points' heights (QUIRKS.md
  Q-0023, Q-0024).
- **Doors**: `OnOpen` when a player, a creature or a script opens one,
  `OnFailToOpen` for a locked one used, a container too (locked as
  placed, or as `SetLocked` left it: `ObjectState::lock`, which
  `GetLocked` reads and routes go by); the opener is `GetLastOpenedBy`'s.
  `OnUnlock` when a key, Security (`WORLD-STATE.md`, Locks) or a script's
  `ActionUnlockObject` unlocks a door or placeable (`GetLastUnlocked`),
  `OnLock` for `ActionLockObject` (`GetLastLocked`).
  A creature opens a door by running into it only while walking a route
  (its path goes through), never while fighting in place; it is the
  opener. A shut door it runs into runs its `ScriptOnBlocked`
  (`GetBlockingDoor`: K1's `k_ai_master` opens it, or bashes a locked one,
  and signals user event 1009): once as the door opens for it, every
  second while it stays shut [verify the executable's retry]; a party
  member's not while the world is held.
- **Scripted blows** (`CutsceneAttack`): the attacker stands facing its
  target for the round, playing the `animations.2da` row the script names
  as given (K1's executable plays any row, `runds` too), its target the
  table's answer to it (`combatanimations.2da`); a hit's damage lands half
  a round in [verify: the executable lands it at once], so what the scene
  sets meanwhile (`SetMinOneHP` off) holds when it lands. A gunner fires each blow as one shot timed to
  meet the target just before then: at the chest for a hit, whose bolt
  carries the damage (the hit shows its gun's impact and the body falls
  the way it was shot), past the shoulder for a miss, which lands nothing.
  A script's bolt is the script's: no blade turns it, it harms no one else,
  and a blow its bolt did not bring (a wall in the way) lands when due all
  the same. A save carries them (KJKV 15, the result KJKV 35); a module
  change drops them.
- **Assured blows** (`EffectAssuredHit`): not parried while it lasts; an
  attack or talent a script asked for lands as a scripted blow (its
  weapon's average damage, at the swing's `hit1`; a gun's half a round
  on) once its target is in reach
  (3 m for a blade), unless the attacker's own swing struck it meanwhile:
  KOTOR's round decides the hit, and here bodies may not close (a gunner
  backing off a blade, the Lower City's gang scene). It is the round's one
  attack: once it struck its target the attack shoots no more before its
  round ends (Taris's duel: Gerlon's one shot leaves Duncan in the dead
  pose his surrender plays; probe `scene` `assured_shot_once_a_round`).
  It plays as the round's attack, as K1's executable picks it
  (`sim::combat::rounds`, COMBAT.md "Rounds between creatures"): its feat's
  swing or its round's set by both sides' weapons (a gun its first shot
  clip), its target answering the hit with `combatanimations.2da`'s clip
  (the Lower City's gang scene: the baton Vulkar's Power Attack `g1a1`, the
  blades' `c2a*`; probes `scene` `assured_blow_answered`,
  `round_clips_by_wield`).
- **Damage** `ScriptDamaged` at most every 500 ms (C15), **death**
  `ScriptDeath`; the attacker is `GetLastDamager`'s / `GetLastKiller`'s.
  A dead creature not of the party is taken away after its appearance's
  `destroyobjectdelay` (3 s where blank), fading out over its
  `fadedelayondeath` (1 s where blank [verify]), unless
  `SetIsDestroyable(FALSE)` keeps it (read when it is due). Gone, it is no
  longer valid to scripts (Taris's Mission waits for the rakghoul's;
  `GetNearestCreature` passes it over: Tatooine's bantha waits for no foe
  near; `GetIsDead` still holds it dead, as Aurora's does any invalid
  object: Korriban's tomb opens on Yuthura's death if Uthar's, faded, is
  dead). One that held what drops or gold leaves its remains where it
  lay once the fade is over: a placeable with no tag, "Remains", holding
  them (`WORLD-STATE.md`, "Remains").
- **Players in scenes.** A script's move on a player held in a
  conversation (a cinematic: Hendar's gate) walks it as a creature
  (`world::Driven`); outside one, a player's legs are its seat's. A move
  fails for want of a way only after half a second (a gate the scene opens
  in the same step is open by then).
- **Items**: an item the party (its inventory, what its members wear)
  comes to hold runs `Mod_OnAcquirItem` (`GetModuleItemAcquired`), one
  gone from it `Mod_OnUnAqreItem` (`GetModuleItemLost`).
- **User-defined**: `SignalEvent(o, EventUserDefined(n))` queues o's
  user-defined hook with `n`, run after the current script.
- **Perception** `ScriptOnNotice`, at KOTOR's cadence: each living
  creature every 300 to 699 ms (drawn each time) while its AI is active,
  every 4 s while a conversation holds it, against every other character.
  Within its hearing range (`ranges.2da` `secondaryrange`, by its UTC's
  `PerceptionRange`; K1's default row: 20 m) it hears; within its sight
  range (`primaryrange`), with nothing solid between its eye and the
  other's chest (doors as they stand), it sees, whichever way it faces
  (xoreos; reone's 120° field of view left the Upper City's interrogation
  commander, facing his prisoner, blind to the party behind him, so his
  AI never fought back: T15) [verify in the game]. A stealthed character
  is heard only past the listen check and seen only past the spot check
  (Awareness against Stealth with rolls held 20 s, `sim::stealth`). Each one newly heard or seen, or no longer, runs the
  hook: `GetLastPerceived`, `GetLastPerceptionSeen` (`…Vanished`,
  `…Heard`, `…Inaudible`). Who it sees and hears now answers
  `GetObjectSeen`, `GetObjectHeard` and `GetNearestCreature`'s perception
  criterion (whether the *target* perceives the creature, as NWScript
  says; reone asks the reverse).
- **Attacked** `ScriptAttacked`: a character hit, at most every 500 ms
  per attacker (C15), before its damaged event (`GetLastAttacker`).
- **Spell cast at** `ScriptSpellAt` (C15): a power that reached the
  object (struck, caught, healed or blessed by it; signalled before its
  resistance and save, as KOTOR's impact script does), at most every
  500 ms per caster: `GetLastSpellCaster`, `GetLastSpell` (the
  `spells.2da` row), `GetLastSpellHarmful`.
- **End of round** `ScriptEndRound` (I22 C14): a creature in combat
  (`GetIsInCombat`: 8 s after its last hit or attack), every 3 s from
  when it entered it. K1's `k_def_combend01` asks `k_ai_master` for the
  round. A script's attack with a blade or fists on a target in its
  reach is an attack every step, blow or none (reone runs a round for
  it), as a gun's every shot is: a duel no blow lands in for a while (the
  Endar Spire's two Jedi, a scene holding them apart, then each Feared by
  the other) still has its rounds, and its AI attacks again.
- **Shouts** (`SetListening`, `SetListenPattern`, `SpeakString`): each
  listener within the volume's reach (whisper 3 m, talk 20 m, shouts the
  area) whose pattern fits (exact, `**` any run) runs `ScriptDialogue`
  with `GetListenPatternNumber` and `GetLastSpeaker`: allies answer "I was
  attacked", "I am dead" (`k_def_dialogue01`).
- Conversations (I18): `DIALOGUE.md`; a player's use runs a creature's
  `ScriptDialogue` with pattern −1, a placeable's `OnUsed`.
- Which field each holder names its script under, per event (UTC, UTD,
  UTP, UTT, UTE, UTM, ARE, IFO): `script::hooks::Hook::label`.

## Action queues (`script::actions`)
- `Action…` appends to the caller's queue (not while `SetCommandable`
  FALSE); `ClearAllActions` empties it (pending `DelayCommand`s stay);
  `AssignCommand` runs its code for the target after the running script.
- Each step the front action runs and the next starts once it is done
  (16 a step at most): code (`ActionDoCommand`) runs; waits (`ActionWait`,
  `ActionPlayAnimation`'s time, the object playing it meanwhile: SKELETON.md,
  Idles) time out. An animation's time is KOTOR's script semantics (S7),
  everywhere, a conversation holding the world or not: a looping one
  given a duration lasts exactly that; any other one plays its clip once at
  its speed (`Library::clip_ms`, the clip's length on the body's model, ÷
  |speed|; 1 s where the model has no length for it); a looping one of the
  low range (`ANIMATION_…` 0–32) given −1 plays at once, outside the queue
  (as a queued one would be taken: not while uncommandable), and loops on
  until something else plays, a move it starts included (the Endar Spire's
  Dark Jedi scene); one of 30 s or more drops the same one queued after it
  (KOTOR compares the clip and its speed, the queue its clip and time
  [verify]). `PlayAnimation` plays at once, its lengths the same, a −1
  looping until stopped [verify]. A move queued after an animation waits
  for it; moves (`ActionMoveTo…`, forced:
  a jump after the timeout) end within range across the floor, meanwhile
  the object's mind routes there (`world::minds`, walking unless run); a
  move that is not forced fails as it starts when no way leads there
  through the doors as they stand (a locked one), judged from where its
  walker would step back on to the walkmesh when it stands off it
  (`walk::leads`: the Sith officer `sithtalk` in Taris's Upper City), and the
  queue goes on [verify against the game]; each move is a walk of its own,
  its bumps and stalls counted afresh (`Walker::begin_anew`; a forced
  move's walk taken up again counts only its bumps afresh);
  jumps put it there; doors (`ActionOpenDoor`, `…Close`) and locks
  (`ActionUnlockObject`, `ActionLockObject`: no check, the scripts
  decide) within 2 m;
  `ActionStartConversation` brings the speaker within 4 m first.
- `ActionRandomWalk`: random points within 5 m, one after another (a new
  one after 8 s if not reached), until cleared.
- TSL's `ActionFollowOwner(range)`, a puppet's (the executable's follow
  action): held while its owner stands within the range, or it has none;
  farther, a run to where the owner stands and the follow again go behind
  what waits; until cleared.
- An unbound `Action…` routine still takes its place, completing at once.
- Walks of objects: `GetFirst/NextObjectInArea`, `…InPersistentObject`
  (who stands in a trigger), `…InShape` (sphere, cube, cylinder, cone;
  line of sight not tested yet), `GetNearestObject`; type filters are
  `OBJECT_TYPE_*` bits.
- A player's moves are dropped (a seat's legs are its own, C14).
- `ActionAttack(target, passive)` (C14: the script chooses what, the
  mind how): to the end of the creature's round, the mind fights that
  target while it lives, stays hostile and is in reach (else its own
  choice); a passive one faces it without swinging. Fighting beats a
  script's move. A blade's or fists' attack keeps the creature in combat
  while the target stands within 3 m (from its second step: the mind
  first meets the target as a foe newly spotted); a gun's keeps it so by
  its shots.
- Talents (C14): a power (`spells.2da` row) or feat. `GetCreatureTalent…`
  return those it has and can use now (a power it knows and has the FP
  for) whose category code lies within the asked one (`code & mask ==
  code`: the AI's masks, `0xf8ff` the buffs, `0x1104` the melee feats),
  the best the highest `maxcr` within the cap. `ActionUseTalentOnObject`
  lasts to the end of the round; the mind uses it once within the power's
  reach (`ai::power_reach`): a power pressed (grip, lightning and drain
  held), a ranged feat as the gun's special, a melee feat as its saber
  style (C8). Classes (`GetLevelByClass`, `GetClassByPosition`, the class
  criterion) come from the template's `ClassList`.

## What scripts read of characters
Faction (`GetStandardFaction`; `GetIsEnemy`, `…Friend`, `…Neutral` by
`repute.2da`), race, hit dice, powers known (`GetHasSpell`), party
(`IsObjectPartyMember`), whom it fights (`GetAttackTarget`,
`GetAttemptedAttackTarget`), and `GetNearestCreature` by race, player,
class, reputation, life, perception and effects had or lacked.

```bash
kjk script coverage k1 tar_m02aa --seconds 30     # Taris: 190 moves walked
kjk script coverage k1 tar_m02aa --seconds 24 --walk 130.7,95.4 --frames DIR
                                                  # who perceives whom, 200 ms
kjk lifecycle cycle k1 tar_m02aa tar_m02ab        # run, replay, save: same
```

## Where dialogue (I18) meets this
- `ScriptDialogue` is raised by shouts, with `GetListenPatternNumber`
  the pattern's number and `GetLastSpeaker` the speaker
  (`script::engine::Engine::speak`), and by a player's use with −1
  (KOTOR's "clicked", no pattern).
- `ActionStartConversation` brings the speaker within 4 m
  (`script::engine::CONVERSATION_RANGE`), then asks for the conversation
  (`script::dialogue`); `GetIsInConversation` and
  `GetIsConversationActive` read the conversations under way.
- `kjk script sweep k1` measures what the whole game's scripts still call
  unbound; `kjk script coverage <game> <module> --trace` shows one module's
  calls in order.
