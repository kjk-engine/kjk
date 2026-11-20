# Stuns that last as long as KOTOR's

Date: 2026-11-19. Status: approved by the owner on 2026-11-19. The approach and the struggle bonus were picked by the owner. Everything marked "my call" is the writer's proposal.

## Why

Today kjk holds every stun, stasis, fear, sleep, droid stun and Force Whirlwind for at most 3 seconds. After a hold ends, the target is immune to the next one for 1 second. This is rule C11, written as the rulebook entries `force.crowd.hold_ms` ("Maximum Stun Time") and `force.crowd.immune_ms` ("Stun Immunity").

The cap makes every power the same. Stun lists 9 seconds and Stasis lists 12, but both hold for 3. A player who spends more Force points on a stronger power gets nothing for it. A Discord player measured this and reported it as a bug (thread 1572169341817061376, "Duration of crowd control abilities e.g. stun, stasis is not as per description").

The cap was added for a good reason. In real-time combat, a character who stands helpless for 9 or 12 seconds feels much worse than in KOTOR's rounds, especially when that character is the player. Players also chain stuns on one enemy until it dies. This design keeps those protections and gets rid of the blanket cap.

## Goals

- Each power keeps its own duration from its description and its script. Stasis then holds longer than Stun, as in KOTOR.
- A strong target breaks free sooner than a weak one. The target's own saving throw decides this, using numbers the games already have.
- Nobody can be held again and again until they die, whether that is the player or an enemy.
- A hold never ends sooner than it does today. The first chance to break free comes after one round of 3 seconds, which is today's whole hold.
- The rules apply the same way to the player, companions and enemies.

## Non-goals

- KOTOR's states stay as they are. These are TSL's cutscene holds, Force Crush, mind tricks and the other effects in `combat::effects::state`. They already last their whole time.
- Covering the ears after Force Scream stays as it is: 2 seconds, then 4 seconds of immunity. That was the owner's decision on 2026-11-17.
- Knockdowns, Slow, confusion and choke are not holds in this sense. They do not change.
- The power descriptions in the game do not change. With this design they are true again.

## Anti-goals

This change fails, even if it works, in any of these cases:
- A boss or a Jedi stays helpless for the full 12 seconds of Stasis in most fights.
- The player is often stunned for 9 seconds by ordinary enemies.
- Holds feel random to the point of being useless. For example, a stun on a weak enemy ends after one round most of the time.

## Approaches considered

1. **Keep the blanket cap (today).** It is simple and protects against long holds. It throws away the difference between the powers, and the descriptions are wrong.
2. **One scale factor for every hold**, for example 50% of the original duration. Stun would last 4.5 seconds and Stasis 6. It keeps the order between the powers, but it ignores who the target is. A level 20 Jedi stays held as long as a Sith trooper does.
3. **The original duration as the maximum, a saving throw each round to break free, and shorter repeated holds.** This is the approach the owner picked. The original's own numbers decide everything: the power's duration, the caster's save DC and the target's save bonus. It needs only a few new numbers of kjk's own: the round length, and the numbers for repeated holds.
4. **Damage ends every hold.** This suits fear and sleep, and kjk already ends fear on damage. For stun and stasis it would make the power almost useless, because the player stuns an enemy in order to hit it.

## The rules

### 1. A hold lasts as long as its effect

A hold lasts the duration that its script, power, grenade or weapon property gives. The 3-second cap is removed. For safety, one new rulebook number, "Longest Hold", stops a hold at 30 seconds. My call: this protects against a mod or script that applies a stun lasting thousands of seconds. No hold in the unmodded games comes near it.

### 2. A saving throw each round to break free

Every 3 seconds, which is one KOTOR round, a held target rolls a new saving throw. It rolls against the same DC and with the same save that the hold first allowed. If the save succeeds, the hold ends at once. The first roll comes 3 seconds after the hold took hold, so no hold ends sooner than it does today.

The save and the DC are always the ones that first allowed the hold. My call: this keeps the original's own choice of save for every source, rather than a list of kjk's.
- **A scripted power.** `k_sp1_generic` rolls `FortitudeSave`, `ReflexSave` or `WillSave` before it applies the effect. The script engine records the save kind and the DC of the last save that the target failed against that caster in the same script run. `world::harm` receives them together with the effect.
- **A scripted hold with no save call before it** uses the caster's power DC, `force::save_dc`, which is KOTOR's `GetSpellSaveDC`. It uses Fortitude for stun, paralysis and droid stun, and Will for fear and sleep, as the rulebook's crowd entry says.
- **Force Whirlwind** on kjk's own path uses Reflex against the caster's power DC, as `powers::whirlwind` already rolls.
- **A weapon's on-hit property** uses the save and the DC that `gear::on_hits` already rolls. For example, stun and sleep use Will, and paralysis uses Fortitude.
- **A grenade or a mine** uses the Reflex save and the DC that `grenades::effect` already rolls.
- **A hold that allowed no save at first**, such as the adhesive grenade's entangle, gets no saves each round. It lasts its whole duration, as in the original.

`gear::Struck::Hold` and `grenades::Secondary::Held` carry the hold today, and they must also carry the save and the DC.

Each later save is easier. The second roll of a hold gets +5, the third gets +10, and so on. This is the owner's decision of 2026-11-19: the target struggles harder the longer it is held. Some examples:
- A level 8 player with Fortitude +6, stunned by a level 12 Dark Jedi (DC about 22), stays stunned for the full 9 seconds about 37% of the time. Without the bonus, it would be 56%.
- A boss with Fortitude +10, held by a level 12 caster's Stasis (DC about 23), stays held for the full 12 seconds about 2% of the time. Without the bonus, it would be 22%.

A target with a natural 20 always breaks free, and a target with a natural 1 never does. This is how `combat::check` already treats every save.

### 3. Repeated holds get shorter

The game counts how often each character was held recently. A hold "counts as recent" if it ended less than 10 seconds ago.
- The second recent hold lasts half of its duration.
- The third recent hold lasts a quarter of its duration.
- A fourth hold does not take hold. The character is immune until 10 seconds have passed since its last hold ended.

The 1 second of immunity after each hold stays. All of these numbers go in the rulebook. My call on the numbers: 10 seconds, one half, one quarter, refused after three. They are a starting point for playtests.

A shortened hold never lasts less than 3 seconds, which is today's hold, unless its full duration is shorter than that. My call: this keeps the goal that no hold ends sooner than today. For a 3-second hold, such as Force Whirlwind, only the refusal after three holds has any effect.

The per-round save still applies to a shortened hold.

### 4. Holds that stay as they are

Some holds keep exactly today's behaviour. My call: these are not combat holds, and changing them could break a scripted scene or a chain route.
- A permanent hold that a script applies (`DURATION_TYPE_PERMANENT`), such as a scene's stasis.
- A hold whose creator is not hostile to the target, such as a story script or a cutscene.
These holds roll no saves, do not count as recent holds, and are never refused.

Stun, fear and droid powers on kjk's own path, used where no impact script runs (`Cast::crowd`), keep their 3 seconds and roll no saves. They have no duration of their own. They do count as recent holds.

### 5. What the player sees

When a held character breaks free by a saving throw, the combat feedback shows the save in the same form as KOTOR's other saving throw lines. When a hold is refused because the target was held too often, the feedback shows "Immune", as it already does for the 1-second immunity.

## What changes in the code

- `ForceState::hold` in `crates/sim/src/combat/force.rs` takes the hold's DC and save kind, as well as its duration. It stores when the next save is due and how many recent holds the character had.
- A step system rolls the due saves for every held fighter, with the sim's dice. This happens on the authority only, as `powers::saves` already does.
- The callers pass the DC and the save: `world::harm` for script effects, `powers::whirlwind`, `world::struck_by` for on-hit properties, and `grenades::afflict`.
- `ForceState` is part of the snapshot, so its encoding changes. This needs a snapshot format bump and a migration that reads old saves. An old save's running hold keeps its end time and gets no new save rolls. The new fields of an old save read as 0.
- The sim hash changes, so the branch needs a bumps file.
- The rulebook replaces "Maximum Stun Time" with the new rules: "Saving Throw Each Round" (3 seconds), "Struggle Bonus" (+5 for each earlier roll of the same hold), "Longest Hold" (30 seconds), "Recent Hold" (10 seconds), and the two fractions with the refusal after three. "Stun Immunity" stays. The crowd entry's summary text changes to describe all of this. `docs/COMBAT.md` describes C11 again.

## How to check it

- Probes for each rule. A Stun on a target with a very low Fortitude lasts its full 9 seconds. A target with a very high Fortitude breaks free after 3 seconds. The second and third holds are shorter. The fourth is refused. A fear still ends on damage.
- A probe that a hold never ends before 3 seconds have passed.
- A probe that the second and third rolls of a hold get +5 and +10.
- A measurement for the anti-goals. Run many staged holds with the two examples above, the player against a Dark Jedi and a boss against a caster, and report how often each hold goes its full length. The results should be near 37% and 2%. If they are far off, the builder reports this and does not change the rules without the owner.
- The rulebook check (`kjk rules`) shows the new rules with their texts.
- The approved tests that measure hold times change their output, and the owner approves them.
- A K1 chain, because companions and enemies stun each other in many routes.

## Left to the builder

- Where the due saves are rolled in the step, and how they line up with `combat::effects::settle`.
- The exact form of the feedback line, taken from the game's own talk-table lines for saving throws.
- Whether Force Whirlwind's lift and spin look right when the hold ends early. If not, the spin ends with the hold, as it does today.
