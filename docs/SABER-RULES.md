# Saber combat rules of Movie Duels (behaviour spec)

A behaviour description of the saber combat in the Jedi Academy single-player mod Movie Duels (MD), written so the
rules can be built again from scratch. It describes what happens in play, in plain words; every number is listed
again in the last section ("Numbers") so it can go into a rulebook.

Terms used throughout:

- **Mode.** The "Saber Combat Mode" setting has three values: 0 = stock Jedi Academy ("JKA Mode"), 1 = "MD Mode"
  (MD), 2 = "MD Advanced Mode" (MDA). "Both" below means MD and MDA; mode 0 is the stock game and is only mentioned
  for contrast.
- **The meter.** The pool a saber fighter spends on swings and blocks. In MD it is the Force pool (powers and saber
  share it). In MDA it is a separate pool of block points, and Force is (almost) only for powers. Where a rule says
  "the meter", read "Force" for MD and "block points" for MDA.
- **The mishap count.** A per-fighter number from 0 to 15 that grows with aggressive or failed saber play and makes
  bounces, staggers and disarms more likely. On the HUD it is shown (inverted) as the "stamina" bar.
- **Player / NPC.** "Player" also covers a character the player is controlling.
- **Styles.** Fast (blue), Medium (yellow), Strong (red), Desann, Tavion, Dual, Staff.
- **Master classes.** Sith Lord, Desann, Vader, Kyle, Galen, Tavion, Yoda, Luke (character classes, NPC or player).
- **Difficulty.** 0 = easiest, 1 = second, 2 and above = hard.
- **Running** means the legs play a run animation; anything else (stand, walk) is "walking".

---

## 1. The meters

### 1.1 What exists in each mode

| | MD | MDA |
|---|---|---|
| Saber meter | Force pool, max 100 (or the character's Force max) | block points, 0..100 (max of the character, default 100) |
| Force | the same pool as the saber meter | separate, used by powers, dodges, dashes |
| Mishap count | 0..15 | 0..15 |
| "Stamina" | not a pool: the HUD's stamina bar is the mishap count turned upside down | same |
| Sprint fuel | 0..100, separate (see 2.4) | same |

There is no separate stamina pool in either mode. What the menu calls the "Stamina Bar" is `1 - mishap / 16`.

Block points still exist and still regenerate in MD, but nothing in MD spends them, except a few checks that read
block points in every mode (listed in 1.5).

### 1.2 What drains which meter

| Action | MD drains | MDA drains |
|---|---|---|
| Player starts a new plain attack swing (saber on) | 1 Force | 1 block point |
| Player starts an attack-fake transition (single saber, not Dual) | +1 mishap on every other chained swing, no meter | same |
| NPC starts a Staff/Dual knockaway | 2 Force | 2 block points |
| NPC starts a knockaway parry, back block or blocking animation | nothing | 1 block point each |
| NPC starts a kick | 2 Force | nothing |
| NPC starts an attack-fake transition (not Dual) | +1 mishap | +1 mishap |
| Every attack swing start (player and NPC) | +1 mishap (up to 15) | +1 mishap (up to 15) |
| Blocking a saber swing | Force, see section 4 | block points, see section 4 |
| Deflecting a bolt | Force, see section 7 | block points, see section 7 |
| Taking a kick | 10 Force | 2 (absorbed) or 5 (not absorbed) block points |
| NPC dodge (body dodge of a shot or swing) | 35 Force (needs 50 or 70) | 35 Force (needs 50 or 70) |
| NPC dash dodge (smart NPC, MDA only) | - | 35 Force (needs 50) |
| Force powers | Force | Force |

Master classes pay less in MDA only: every block-point cost paid by a blocker is cut to 80% against sabers and 60%
against blaster bolts (rounded, at least 1). There is no such reduction in MD.

### 1.3 How the meters refill

Gains (both modes put the gain into "the meter"):

| Event | Gain |
|---|---|
| Perfect block (section 3) | +15 |
| Being disarmed (the disarmed fighter) | +20 block points (MDA) or Force (MD) |
| Very low meter "disarm" outcome while still holding the saber (section 4) | +20 |
| Kill (player or NPC attacker, saber/melee/most damage; not Force grip/lightning/drain/crush) | +10, or +5 if the killer is a player holding block |
| Hurting an enemy so much the hit is lethal (player attacker, difficulty 0-1 only) | +5, or +2 when holding block |
| Hurting an enemy (player attacker, difficulty 0-1 only) | +3, or +2 when holding block |
| Landing certain kicks/melee | +2 to the kicker |
| Parrying an attack fake with a perfect or NPC block | +10 Force (both modes) |
| NPC whose attacker only holds block (no attack): attacker NPC regains 20 if at or under 20, else 10 | NPC attacker |
| MDA NPC "meditation" (enemy saber user further than 256, NPC idle, block points under 30) | +25 block points, then not again for 15-25 s |
| Start of a cutscene (player) | meter set to full, mishap set to 1 |

Kill and hurt bonuses also drop a mishap count of 10 or more to 5 (not when the player is holding block).

**Force regeneration (both modes, players and NPCs):** +1 Force every regen tick, the tick being the character's
regen interval (default 100 ms, NPC files can change both interval and amount). No regeneration while any Force power
is running, while playing a Force-using saber animation, or while doing an unblockable attack. Mode 0 also stops it
while a thrown saber flies; MD/MDA do not.

Extra player rules in MD and MDA, checked on each regen tick when the player is not knocked down, walking, on the
ground, not mid-swing (weapon timer done), not bouncing and not stabbing:
- resting (meditation pose): +4 more Force and one mishap step down per tick, meditation look on;
- crouching: +2 more Force and one mishap step down per tick;
- holding the block button: the next tick is pushed back 4000 ms in MD, 2000 ms in MDA;
- cloaked: the next tick is pushed back one regen interval.
Running, swinging and airborne do not stop player Force regeneration in either mode.

Extra NPC rules: in MD, after each tick, if the NPC is attacking, doing a special/spin attack, parrying, returning,
running with a saber, or has its saber thrown, the next tick is pushed back 2000 ms. In MDA only a thrown saber
pushes it back 2000 ms. In both modes Rage recovery halves the rate. On top of that, in MD an NPC with its saber on
gains +1 Force once a second (not while in a heavy bounce, slow bounce, knockdown, broken parry or right after a
block).

**Block-point regeneration, player (all modes, only matters in MDA):** +1 per 100 ms tick, only while the player is
not knocked down, on the ground, walking, not attacking, not in a special/spin attack, parry, return, stab, bounce or
heavy bounce, and not holding (or pressing) block. On a tick:
- resting: +4 more (and meditation look), crouching: +2 more, with no extra wait;
- otherwise the next tick is pushed back: one more regen interval under Rage recovery, else 2000 ms if block points
  are above 75, else 2000 ms if the saber is thrown, else 2000 ms if the player is idle (weapon timer done). So
  standing still gives about 1 point per 2.1 s; resting gives about 50 points a second and crouching about 30.
- master classes wait 20% less between ticks (25% faster).

**Block-point regeneration, NPC (MDA):** +1 per second, not while in a heavy bounce, slow bounce, knockdown, broken
parry or right after a block. (The code means to do it every other second; whether that ever skips is not
determined, see the end.) Plus the meditation bonus above and the event gains.

### 1.4 The fatigued state

- **Set:** in both modes whenever Force is 20 or less (checked every frame); in addition whenever a drain takes the
  meter to 10% of its maximum or less (10 of 100).
- **Cleared:** on a Force regen tick when Force is above 20 (player and NPC, MD and MDA). So in MDA the state follows
  Force, not block points: a block-point drain under 10 sets it, and the next Force tick clears it again if Force is
  above 20.
- **Effects:** no Force roll/acrobatic getup from a knockdown (player); cannot block lunges or spin attacks
  (MD and MDA); an NPC cannot absorb kicks.

A second "drained" state is set whenever a drain leaves the meter under 20, and in MDA also whenever block points
are under 10 at a saber move (cleared there when above 10). It also stops blocking lunges and spin attacks.

### 1.5 Checks that read block points in every mode

These read block points even in MD (where block points stay near full): kata-versus-attack outcome (disarm under 10),
saber wear doubling (20 or less), the standing block lean (needs more than 2 block points and more than 2 Force), kick
absorption (needs 35 block points and 35 Force), clash sound and spark choice (above 50 or a perfect block flag:
bright block effect and sound, else a duller "knock").

---

## 2. Manual block

### 2.1 The button

The "Manual Block/Sprint" control. Menu text: MD/MDA only; hold with attack to control direction; hold when running
to sprint. In mode 0 it does nothing for blocking.

### 2.2 When holding block counts (player, both modes)

Holding block is a block only when all of these are true: saber equipped, ignited or ignitable, not thrown; Saber
Defense at least level 1; meter at least 1 (MDA: block points at least 1, and Force at least 1 in both modes); on the
ground; not resting, not Force-drained/gripped/grasped, not at 1 health, not knocked down, rolling, in a lock break,
special attack, special jump, bounce, knockaway or broken parry; not attacking; not pressing alt attack, Force focus,
lightning, lightning strike, projection, drain, grip or dash. Holding block while the saber is off ignites it (except
Yoda). Holding block while running, without attack or walk pressed, with at least 5 meter, counts as a running block
(and sprints, 2.4).

### 2.3 Holding block alone

- The fighter enters the blocking pose (section 11).
- The fighter's swings do no damage while block (or block plus attack, or the perfect window) is held.
- MD: Force regen ticks are pushed back 4000 ms; MDA: block points do not regenerate at all, Force ticks are pushed
  back 2000 ms.
- The mishap count does not decay while block is held.
- Standing still and pushing a direction leans into a directional standing block (third person).
- In MDA, holding the walk key while blocking and moving marks a "block walk"; the walk key itself is consumed.
  Movement speed while blocking: not determined (no speed factor was found).
- Each saber hit taken while only holding block costs 5 meter (player), see section 4.

### 2.4 Running with block held: sprint

With a saber (and also with fists/melee), holding block while running with more than 15 sprint fuel plays the sprint
run and turns sprinting on. Sprint speed: x1.6 run speed (Vader x1.15, Yoda x1.65; with the Master animation style a
Yoda-style character x1.2, Yoda x1.5, a Vader-style character x1.15). Not available to small battle droids or Jawas,
or while Rage or Speed is on. Sprint fuel: -1 per 150 ms while sprinting; starting a sprint with under 17 fuel costs
10 more; refills +1 per 150 ms (75 ms when resting or crouching, 250 ms while under 15).

kjk does not have sprinting: a block held while running holds the block at the normal run speed.

### 2.5 Holding block with attack: aimed block, perfect window

Pressing attack while block is held does not swing. It turns on the "block plus attack" state, and the first frame
of it starts the perfect block window (section 3). The block direction is chosen from the movement keys (section 11):
back-left, back-right, back, forward-left, forward-right, forward, left, right, or none (top block). Releasing attack
ends the state and resets the window; a new press is needed for a new window.

Holding block plus attack for 3 s or more: the directional "accurate parry" test fails, and bolt block costs double.

Holding walk + block + attack together is a separate "accurate missile block" input; it only has an effect with a
controller (section 7).

### 2.6 The directional "accurate parry" test

A test used for NPCs (and to classify a player's block): the attacker must be in front (within a 0.3 dot cone); not
while kicking, swinging, bouncing, slow-bouncing, ducked or knocked down; for players the block button must be held
and pushed in a direction; the hit point is compared with the pushed direction (right/left against strafe, up/down
against forward/back) and passes when they agree by at least 0.55 (Fast style), 0.35 (Strong style) or 0.40 (others).
Already being in a knockaway passes. An NPC that fails passes anyway with chance 5% x difficulty (0%, 5%, 10%, 15%).
In practice a player's block outcome is decided by the button states (section 4), so this test mostly matters for
NPCs and for bolt costs.

---

## 3. Perfect block

- **Start:** the moment attack is pressed while block is held (or both pressed on the same frame). The window is
  open from then on for: 250 ms at difficulty 0, 150 ms at difficulty 1, 50 ms at difficulty 2 and above.
- **Against bolts:** a 750 ms bolt window setting exists but nothing reads it; bolts have no timed perfect
  deflection (section 7).
- **Rewards when a blockable swing lands inside the window (both modes):**
  - damage of the swing is cancelled; the blocker plays a perfect parry (knockaway-like MD animations, e.g.
    BOTH_K1_S1_TR_MD / BOTH_K1_S1_TL_MD, back hits BOTH_P1_S1_B_);
  - blocker: meter +15, mishap -5;
  - attacker: player loses 10 meter and gains 5 mishap; NPC loses 20 meter and gains 10 mishap;
  - attacker reaction: if the attacker's mishap is 13 or more, a mishap roll (section 5); otherwise a heavy stagger,
    whose heavy-bounce animation plays at 0.55 speed when Saber Attack Speed is MD;
  - player blocker: camera shake (0.45 for 100 ms) and, with "Perfect Block Slowmo Action" on, slow motion
    (time x0.3 for 200 ms); a perfect-block sound; NPC blocker: a taunt/combat bark;
  - bright clash effect; the "perfect block" mark stays on until the blocker lets go of block, bounces or attacks.
- **Against an unblockable attack inside the window:** the attacker is disarmed (subject to disarm protection, 4.6),
  damage cancelled, blocker +15 meter and -5 mishap, attacker -10 meter and +5 mishap, slow motion and sound as above.
- **MDA riposte (MDA only):** when a swing reaches a fighter whose perfect window is open, that hit does no damage,
  the attacker is frozen 350 ms, its combo count resets, it loses 10 armor, and the defender gets a 500 ms riposte
  window: its next attack in that window plays 13% faster.

### 3.1 The perfect parry's animation (both modes)

Read from the source of 2026-11-11.

- **Direction.** Only the point where the blades met decides it, taken relative to the blocker. The attacker's swing
  and the blocker's movement keys do not count.
  - Behind is tested first. The flat direction from the blocker to the point is compared with the blocker's facing.
    A dot product of -0.7 or less is behind, for a blocker with Saber Defense 1 or more.
  - Height is the point's height less the blocker's eye height. Above -5 is the upper band. From -22 to -5 is the
    middle band. Below -22 is the low band.
  - Side is the flat direction from the eyes to the point, compared with the blocker's right. In the upper band,
    above 0.3 is right and below -0.3 is left. In the middle band the limits are 0.1 and -0.1. Between the limits is
    the centre. In the low band, 0 or more is right and the rest is left.
- **Sequence.** Single means every style but Dual and Staff. No case plays a stock knockaway (BOTH_K1_S1_*, K6, K7).

  | Where the blades met | Single | Dual | Staff |
  |---|---|---|---|
  | Behind | BOTH_P1_S1_B_ | BOTH_P6_S1_B_ | BOTH_P7_S1_B_ |
  | Upper or middle, right | BOTH_K1_S1_TR_MD | BOTH_B6_TR___ | BOTH_B7_TR___ |
  | Upper or middle, left | BOTH_K1_S1_TL_MD | BOTH_B6_TL___ | BOTH_B7_TL___ |
  | Upper or middle, centre | BOTH_B1_T____ | BOTH_B6_T____ | BOTH_B7_T____ |
  | Low, right | BOTH_B1_BR___ | BOTH_B6_BR___ | BOTH_B7_BR___ |
  | Low, left | BOTH_B1_BL___ | BOTH_B6_BL___ | BOTH_B7_BL___ |

- **How it plays.** On the torso only. The legs keep their stand, walk or run, and the blocker may move. It is forced
  over what the torso played, from its start, with a 100 ms blend, and it is held for its whole length. The weapon
  timer is set to a random 300 to 600 ms. The MD knockaways and the back parries play at their own rate. The bounce
  sequences (BOTH_B#_) follow the speed rules of 6.4.
- **Not a saber move.** The blocker's saber move stays the ready move, with no start or end quadrant. When the hold
  ends the blocker shows the held-block pose (11.5) if block is still held, else the ready stance. There is no return
  move. Once the weapon timer has run out a new swing may start. It starts as a swing from the ready stance, and it
  replaces the parry even if the parry has not finished.
- **Skipped, with the rewards kept,** when the blocker's saber move is a transition, bounce, knockaway, attack or broken
  parry, and when the blocker is rolling, knocked down, raising or lowering the weapon, or in a heavy bounce or bashed
  animation. A character whose animation set lacks the sequence plays nothing, and its weapon timer is still set. A
  second perfect block during the animation starts it again. There are no crouched, airborne or moving variants.
- **Galen Marek (Master animation style only, 11.6):** BOTH_K1_S1_TL_MD_GALEN, BOTH_K1_S1_TR_MD_GALEN,
  BOTH_P1_S1_B__GALEN, BOTH_P6_S1_B__GALEN and BOTH_P7_S1_B__GALEN take the places of the plain names. A fighter counts
  when its own character style is Galen Marek, or when the player's chosen character style is.
- **NPCs** never enter the perfect state. An NPC's good parry (meter above 75, section 4.1) uses the same table and
  the same timer, except at the centre, where it plays BOTH_P1_S1_T__MD (Dual BOTH_P6_S6_T__MD, Staff
  BOTH_P7_S7_T__MD).
- **Large characters** (Wookiee, super battle droid, assassin droid, hazard trooper, rocket trooper, saber droid and
  Vader) also play BOTH_K1_S1_TR_MD or BOTH_K1_S1_TL_MD for a plain parry met in the upper band to a side, with a
  single style.
- **Against an unblockable attack** (the disarm) the blocker plays no animation. The table is never used against
  bolts.
- Nothing plays BOTH_K1_S1_TL_PB, BOTH_K1_S1_TR_PB, or the _ALT and _OLD pairs.

---

## 4. The block decision when a swing meets a blade

When an attacking blade hits a defending blade, three things are decided: is a saber lock started (section 8), how
the attacker reacts, and how the blocker reacts and pays. A fighter counts as "defending" when it is parrying,
holding block, holding block plus attack, in the perfect window, an NPC (NPCs always count), recently blocked, or
simply standing ready. A defender who is itself swinging is handled as attack-versus-attack (4.4).

Blocker states, in the order they are tested:
1. **perfect**: block plus attack held and the perfect window open;
2. **active block**: block plus attack held, window closed (spamming block+attack);
3. **held block**: block held, no attack;
4. **NPC block**: an NPC in normal saber stance (or an accurate-parry pass);
5. **none**: a player standing ready without holding block.

### 4.1 Blocker side, blockable attack

First a low-meter check (both modes, on the meter): if the meter is 20 or less:
- **10 or less:** an NPC rolls a blocker mishap (5.3); a player is disarmed (4.6). If the blocker still holds a saber
  afterwards it gains 20. Damage cancelled.
- **11 to 20:** a "fatigued knockaway" stagger (a random one of BOTH_K1_S1_T_, _TR, _TL, _BL, _B_, _BR; Dual
  BOTH_K6_S6_*, Staff BOTH_K7_S7_*), cost 4. Damage cancelled.

Otherwise, by state:

| State | Animation | Blocker cost MD | Blocker cost MDA | Other effects |
|---|---|---|---|---|
| perfect | perfect parry | gains 15 | gains 15 | section 3 |
| active block | fatigued parry if meter 50 or less or attacker uses Strong/Desann, else normal parry | NPC 10, player 5 | 5 | NPC attacker +5 mishap; player blocker camera shake |
| held block | fatigued parry if meter 50 or less or attacker uses Strong/Desann, else a bounce-block | NPC 10, player 5 | player 5, NPC 0 | NPC attacker regains 20 if at 20 or less, else 10; camera shake |
| NPC block | attacker Strong/Desann: fatigued parry (no cost); meter 40 or less: fatigued parry; 41-75: plain parry; above 75: "good parry" | 10 / 5 / NPC 15, player 5 | 2 / 3 / NPC 10, player 3 | good parry: NPC taunt; "good parry" sound |
| none | no parry | 10 | player 10, NPC 0 | blocker mishap roll (5.3); damage is NOT cancelled |

In every state except "none" the swing's damage is cancelled. In MDA the blocker costs above are reduced for master
classes (80%).

### 4.2 Blocker side, unblockable attack

- perfect: section 3.
- otherwise: meter under 10: disarmed (4.6) and +20; else a fatigued knockaway and cost 10 (MD NPC 15). The swing
  is not parried: it damages the blocker.

### 4.3 Attacker side (both modes)

In order:
1. **Attacker uses an unblockable attack:** blocker perfect: attacker disarmed, damage cancelled. Otherwise the swing
   goes through.
2. **Blocker is also swinging:** attack versus attack (4.4).
3. **Attacker plays a super-break win** (a lock win, 8.3) and it is blocked: attacker +3 mishap and a heavy stagger;
   blocker -3 mishap.
4. **Attacker is faking (6.2):**
   - blocker perfect or NPC block: attacker +2 mishap, then a mishap roll; blocker -4 mishap and +10 Force;
   - otherwise the fake breaks the guard: attacker -3 mishap and a saber lock is tried (forced lock).
5. **Plain attack against any block state (1-4):**
   - perfect, active or NPC block: if the blocker is an NPC with meter 75 or more and the attacker's mishap is 12
     or more, 20% chance of a heavy stagger for the attacker; else the attacker is "blocked" (its bounce and return
     play at 0.85 speed with Saber Attack Speed MD). A player attacker gets a camera shake (0.45, 100 ms).
   - held block or accurate parry: the attacker is "parried" (bounce and return at 0.90 speed with that option).
   - unless perfect, the attacker then gets the bounce ladder (5.2).
   - blocker -3 mishap.
6. **No block (state none):** if the blocker is itself in an unblockable animation, the attacker gets a heavy stagger
   and the blocker -3 mishap; otherwise the attacker gets the bounce ladder with "both bounce" on, and the blocker is
   staggered.

### 4.4 Attack versus attack

| Case | Effects |
|---|---|
| attacker faking, other not | attacker +3 mishap, other -3; saber lock tried with the faker as lock attacker |
| other faking, attacker not | attacker -3, other +3; lock tried with the other as lock attacker |
| both faking | both +3; lock tried |
| attacker in a kata (special combo) | attacker +3, other -3; other under 10 block points: disarmed and +20; else staggered and pays 10 (master 8) |
| other in a kata | attacker -3, other +3; attacker under 10 block points: disarmed and +20; else staggered and pays 10 |
| neither | both +3, both get the bounce ladder with "both bounce" on |

(The kata cases read block points in both modes and the cost goes to block points, so in MD they cost nothing real.)

### 4.5 Animations used

- **Small bounce:** the style's attack bounce for the current swing.
- **Attack bounce ("both bounce"):** random from BOTH_B#_BL___, _BR___, _TL___, _TR___, _T____, __L___, __R___ for
  the style (# = 1 Fast, 2 Medium, 3 Strong, 4 Desann, 5 Tavion, 6 Dual, 7 Staff).
- **Heavy stagger (attacker) and stagger (blocker):** random from BOTH_BASHED1, BOTH_H1_S1_T_, _TR, _TL, _BL, _B_,
  _BR (Dual and Staff have H6/H7 counterparts); during a kata the whole body plays it. A stagger that is a heavy
  bounce locks the saber for the length of the animation.
- **Stumble (pain stagger, kicks):** random from BOTH_BASHED1, BOTH_PAIN3, BOTH_PAIN2, BOTH_PAIN15, BOTH_PAIN12,
  BOTH_PAIN5, BOTH_PAIN7.
- **Fatigued knockaway:** see 4.1.
- No stagger is played on a fighter getting up from a knockdown or whose saber is thrown.

### 4.6 Disarm

The right-hand saber is knocked out of the hand, thrown upward at 350 units/s (sabers marked not disarmable, saber
droids and cutscenes excepted). The disarmed fighter loses its block states and gains 20 block points.
- **Difficulty 0 and 1: disarm protection.** The first disarm attempt starts a protection window (10 s at
  difficulty 0, 8 s at difficulty 1) and is refused; attempts during the window are refused; an attempt after the
  window has ended (but within the same length again) succeeds. A refused disarm cancels the fighter's attack
  press.
- **Difficulty 2 and above:** the saber drops at once.

### 4.7 Other swing rules in MD/MDA

- A swing deals damage only in a real damaging move: not idle, not in a transition, not in a partial-damage move
  (unless Force Speed is on), not a kick.
- MDA: one hit per victim per swing (katas, kill moves and locks excepted); landing a hit halves the attacker's
  weapon timer (at least 50 ms) so the next chained swing comes sooner; hitting someone in the wind-up of an attack
  ignores armor and cancels that attack.
- A thrown saber that hits a fighter who fails to parry it: meter under 20: disarmed and +10; else a fatigued
  knockaway and cost 10 (MDA block points).
- Damage amounts follow the "Saber Damage" setting (13).

---

## 5. Mishaps and balance

### 5.1 The count

0..15 per fighter (16 is clamped back to 15). It is raised by:
- every attack swing start: +1 (both modes, player and NPC; NPCs chaining out of a bounce also +1);
- attack fakes: +1 (player: on every other chain count; NPC: every fake; never for Dual);
- exchanges (section 4): +3 when your swing is blocked or clashes, +5 (player) / +10 (NPC) when perfect-blocked,
  +2 when your fake is parried; -3 when you block, -5 when you perfect-block, -4 when you parry a fake, -3 when your
  fake breaks a guard;
- running doubles any rise and halves any drop.

### 5.2 The bounce ladder (attacker whose swing was blocked, not perfect)

Not applied to a thrown saber. By the attacker's mishap count:

| Count | Player attacker | NPC attacker |
|---|---|---|
| 15 | mishap roll (certain at 15) | 20%: mishap roll, 80%: heavy stagger; count reset to 1 |
| 12-14 | heavy stagger | small bounce; count reset to 5 |
| 5-11 | small bounce | small bounce |
| 0-4, "both bounce" on | both fighters play an attack bounce | same |
| 0-4 otherwise | the normal blocked bounce only | same |

### 5.3 The mishap roll

Chance = count / 15 (a roll of 1..15 at or under the count). When it hits:
- **attacker:** 50% heavy stagger, 50% disarm; an NPC attacker turns the disarm into a stagger 20% of the time (for a
  parried fake this 20% check is made when the *blocker* is an NPC);
- **blocker:** 50% stagger; 50%: an NPC blocker is staggered (25%) or disarmed and given 20 (75%); a player blocker is
  disarmed.

### 5.4 Levels and what they trigger

| Count | Trigger |
|---|---|
| above 2 | HUD lamp flashes green |
| 4 or more | boss Kyle can no longer grab |
| 5 | small-bounce threshold; reset target after big bonuses |
| above 5 | NPCs may slap/kick you (with your meter at 65 or less) |
| above 6 | HUD lamp blue |
| 8 or more | smart NPCs stop chaining straight out of parries |
| 10 or more | cannot absorb kicks; kill/hurt bonus resets it to 5; HUD lamp magenta above 10 |
| 12 or more | heavy stagger rung; NPC blocks may stagger you (20%); NPCs stop the always-parry habit while attacking |
| above 12 | "attack fatigue": swings play at 0.85 speed (with Saber Attack Speed MD); HUD lamp red with a warning sound; decay doubles |
| 13 or more | perfect-blocked attacker gets a mishap roll instead of a stagger |
| 15 | top rung of the ladder |

### 5.5 Decay

- Player, once a second, when not slow-bouncing, not in a broken parry, not attacking, not in a transition, not knocked
  down, not locked, not right after a block, on the ground and not holding block: -2 if above 12, else -1.
- NPC, once a second under the same conditions: -1 (meant to be half speed, see the end).
- Player resting or crouching (MD/MDA): one step down on every Force regen tick (so it falls fast).
- Kill and hurt bonuses: 10 or more drops to 5.
- Cutscene start: set to 1 (player).

---

## 6. Attacks

### 6.1 Unblockable attacks

These attacks cannot be blocked except by a perfect block (section 3). By animation: BOTH_ATTACK_BACK,
BOTH_A2_STABBACK1, BOTH_CROUCHATTACKBACK1, BOTH_BUTTERFLY_LEFT, BOTH_BUTTERFLY_RIGHT, BOTH_BUTTERFLY_FL1,
BOTH_BUTTERFLY_FR1, BOTH_FJSS_TR_BL, BOTH_FJSS_TL_BR, BOTH_FORCELEAP2_T__B_, BOTH_FORCELEAP2_T__B__PAL,
BOTH_JUMPFLIPSLASHDOWN1, BOTH_JUMPFLIPSTABDOWN, BOTH_JUMPATTACK6, BOTH_JUMPATTACK6_GRIEV, BOTH_JUMPATTACK7,
BOTH_SPINATTACK6, BOTH_SPINATTACK6_GRIEV, BOTH_SPINATTACK7, BOTH_FORCELONGLEAP_ATTACK, BOTH_FORCEJUMPDASH_ATTACK,
BOTH_STABDOWN, BOTH_STABDOWN_STAFF, BOTH_STABDOWN_DUAL, BOTH_A6_SABERPROTECT, BOTH_A7_SOULCAL, BOTH_A1_SPECIAL,
BOTH_A2_SPECIAL, BOTH_A3_SPECIAL, BOTH_A1_SPECIAL_YODA, BOTH_A2_SPECIAL_ANI, BOTH_A6_SABERPROTECT_GRIEV,
BOTH_STABDOWN_WINDU, BOTH_SMASHDOWN_DUAL, BOTH_SMASHDOWN_SINGLE, BOTH_SMASHDOWN_STAFF, BOTH_FLIP_ATTACK7,
BOTH_PULL_IMPALE_STAB, BOTH_PULL_IMPALE_SWING, BOTH_ALORA_SPIN_SLASH, BOTH_A6_FB, BOTH_A6_LR, BOTH_GRAPPLE_FIRE,
BOTH_GRAPPLE_FIRE_GALEN.

While doing one the attacker does not regenerate Force. NPCs do not enter the block stance while doing one.

**Red-glow warning** ("Unblockable Attack Warning", off by default, MD/MDA): an NPC (never the player) playing one of
these animations is drawn tinted solid red, unless cloaked or in a cutscene.

The special combos ("katas": BOTH_A1/A2/A3_SPECIAL and their style variants, the smash-downs, BOTH_A7_SOULCAL,
BOTH_A6_SABERPROTECT) also get +1 power in the clash comparison.

### 6.2 Attack fakes

- Input: attack plus the use button, player only, in the start of a swing or in a transition. One fake at a time.
- Direction: from the movement keys (right = left slice, forward-right = top-left slash, back-right = bottom-left
  uppercut, the mirror for left, forward or back = top slash). No direction, or the same quadrant as the current swing
  end, fakes to the opposite quadrant; a bottom result picks bottom-left or bottom-right 50/50.
- The fake lasts until the fighter leaves its start/transition/attack moves.
- Costs: +1 mishap on every other chained swing (single saber only; Dual fakes are free). Outcomes in 4.3 and 4.4.
- NPCs do not use the player's fake input but have their own fake rule (details not determined).

### 6.3 Chaining limits

The stock chain rules apply in every mode: Rage recovery allows one swing; Rage allows endless chains; a saber's own
chain limit wins when set; Desann, Tavion, Dual and Staff chain freely; Strong (level 3) ends a chain after 1-2 swings
from rest or 2-3 in a chain, and after 1 swing if the next move does not carry the momentum (angle under 135 or over
215 degrees), 2 if it carries it perfectly (180), 3 otherwise; Fast and Medium end spinning chains after 5 (Fast) or
3 swings with growing odds; Medium and Dual end a chain after 2-5 swings at random. The chain count is clamped at 16.

### 6.4 Reduced saber attack speed ("Saber Attack Speed: MD")

Off by default ("JKA"). When on, saber attack, transition and bounce animations play at:

| Style | Speed factor |
|---|---|
| Fast | x1.1 |
| Medium | x1 (times the global saber animation speed) |
| Strong, Desann | x0.95 |
| Tavion | x0.9 |
| Dual | x0.9; its heavy bounces x0.6 |
| Staff | x0.9; its heavy bounces x0.8 |
| Yoda (class) | x1.25 |

and the stock speed-up of transitions (Fast and Tavion x1.5, Strong x0.75) no longer applies. Fatigue factors replace
the style factor (MD/MDA only): mishap above 12 x0.85 (most swings); slow bounce: bounce x0.65, return x0.85; after a
heavy stagger: bounce and return x0.65; parried: bounce and return x0.90; blocked: x0.85; perfect-blocked heavy
bounce x0.55. Force-driven speed overrides still apply.

In MDA, with this option on and Cosmetic Saber Stance off, the animation timing is also slowed when the animations
load: Tavion swings take 20% longer per frame, Dual and Staff 10% longer, and a player landing a hit gets a light
camera shake (0.25, 100 ms).

---

## 7. Bolts and missiles against the blade

### 7.1 Who blocks

- **Player (MD/MDA):** only while holding block, with the saber in hand, not in a heavy bounce or bashed, not in a
  broken parry, kick or grapple, not raising the weapon, not knocked down, not running with Force Speed, and with
  the meter at least the bolt cost (7.2). Bolts from behind can be blocked while holding block.
- **NPC:** always, with the saber on and in hand.
- **Never blockable:** rockets, thermal detonators, trip mines, detonation packs, concussion shots, repeater alt
  fire, flechette alt fire.
- A bolt that touches the blade of a fighter not blocking is still deflected, but wildly (7.4).
- A bolt hitting the blade mid-swing is deflected without interrupting the swing.

### 7.2 Cost of a bolt block

Base 1. Added: Force Speed on +10, running +5, airborne +10, walking and moving +2. Then multiplied: in a broken
parry x1.5, kicking x1.5, a running player x2.5, airborne x2 (Dual x0.5), block plus attack held 3 s or more x2.
Rounded down. A per-weapon table also exists (pistols 4, rifles 2.5, bowcaster 3, disruptor 10, Tusken/DEMP 9,
repeater 1, flechette 2-5.5 by distance, close pistol shots 3/2/1 by distance, x2 (flechette x1.5) when the blocker is
swinging, Fast style -1), but it reads a shooter link that bolts do not carry, so in practice the base applies (not
fully determined). If the meter is below the cost it is emptied instead.

### 7.3 Three deflection kinds

| Input | Aim | Cost |
|---|---|---|
| hold block only | toward the crosshair (where the player looks), with scatter | cost |
| hold block + attack | at the attacker's head (75% the current enemy, else the nearest enemy in a cone), scatter of +-4 sideways and -16..+4 up | cost x2 |
| controller with walk + block + attack | first 1.5 s: like block + attack; afterwards like block only | 2 |
| NPC | at the enemy's chest, same scatter | cost |

MDA costs go through the master-class reduction (60% against bolts). MD costs are taken from Force.

Scatter for "hold block only":
- MD: moving forward or standing: +-(1..5) degrees on yaw and pitch; moving back: +-(0..1).
- MDA: moving forward or standing, or block points under 50: +-(1..3), or +-(3..5) under 25; moving back with 50 or
  more: +-(0..1).
- Low Saber Defense adds up to about two thirds of a degree more.
- (A "good" no-scatter case exists for the first 3 s after the block-plus-attack press, but block-only has no press
  time, so it does not happen; whether this is intended: not determined.)

Scatter for aimed deflections: random +-0.1 per axis when the saber is not idle or the meter is under 50 (MD: under 65,
or +-0.3 when swinging), +-0.3 under 25 (MDA).

### 7.4 Bolt touching an unblocking blade

Deflected toward the shooter (NPC) or toward the crosshair with strength by Saber Defense (factor 2, 3, 10), plus heavy
scatter (+-0.4; +-0.8 if the saber is thrown, +-0.3..0.4 more mid-swing). Costs 12 meter (MDA master classes 7).

### 7.5 Block animations

Meter under 15: a fatigued bolt-block reaction (saber knocked aside, no parry). 15-24: a bounce-direction parry.
25 or more: the normal deflect parry. Skipped while swinging.

### 7.6 Perfect deflection

No timed perfect deflection of bolts was found. The 750 ms bolt window setting is never read.

---

## 8. Saber locks

### 8.1 When a lock starts

On a blade clash, a lock is tried when: both are attacking with equal power, or the attacker is stronger than a
defender, or the defender is stronger than an attacker, or a fake is involved (either side), each passing a chance of
(setting) in 5; or always when forced (a fake that breaks a guard, or boss Kyle against the player: 1 in 3 of his
clashes). "Saber Locks" setting: Never 0, Rarely 1 (20%), Sometimes 2 (40%), Often 3 (60%); default Rarely.

A lock needs: opposite teams, not in a cutscene, both on the ground, height difference 18 or less, distance 8 to 80
units, each within the other's 40-degree view, no pain in the last second, lockable sabers (not lances), not doing a
back stab, already locked or finishing a lock break, not slow-bouncing. Saber droids never lock.

Power of a fighter for this comparison: the power of its swing, +1 in a kata, + the saber's parry-break bonus when
attacking, or + a roll between the saber's parry bonus and its Saber Defense level when defending.

### 8.2 Lock camera ("Saber Lock Camera", off by default)

With it on, during the player's lock the third-person camera blends (250 ms ease) to: angle 40.5 degrees around the
player, pitch -11.25, side offset -25.5, height offset -15.5, distance 82.5, field of view 31; most HUD elements are
hidden; some lock push sounds change. Off: the normal camera, and the player gets pushed grunts while pushing.

### 8.3 Lock result, super breaks, executions ("Saber Lock Executions", on by default)

- A won lock becomes a **super break** when executions are on, the win strength is above 3 and a roll 0..10 is under
  the strength. Against a boss, sub-boss, shadowtrooper or Jedi master it is downgraded to a normal break 80% of the
  time.
- **Execution:** after a won lock, if the winner's Saber Offense beats a roll of 1-2, and the loser is a weak target
  (the player at 25 health or less, any NPC that is not Kyle, Luke, Yoda, Tavion, Alora, Vader or Desann, or an NPC at
  50 health or less), and executions are on, the strength is above 3, a roll 0..25 is under the strength, and (for a
  player loser) health is 10 or less: the loser's right hand is cut off (BOTH_RIGHTHANDCHOPPEDOFF) and the loser takes
  its health + 10 (+1000 for 1000+ health) damage; player winner: slow motion (if the slow-motion option is on) and
  camera shake. Otherwise a disarm is tried: strength + twice the saber's disarm bonus, -2 against two-handed or dual,
  Rage +level, Rage recovery -1 (+1 if the loser is in recovery); a roll 0..10 under it disarms (two-handed sabers
  only half the time).
- The loser cannot act for its lose animation + 500 ms.

---

## 9. Saber wear, kicks, gunner melee, ledge grab

### 9.1 Saber wear and breaking

Present in this source, with no menu entry (console setting, default "everyone"; values off / NPCs only / everyone).
- Only single sabers whose definition names a broken form (breakable staffs and similar); not dual-wielded; never
  for Desann, Tavion, Luke, Kyle, Alora, Vader, Yoda (NPCs).
- Wear 0-100. A blocked swing adds 4 (Strong, Desann, Tavion, specials, unblockables: "heavy"), 2 (Medium), 1 (others);
  a perfect block (block+attack in the window, blockable, more than 20 block points) adds nothing; a hit on the hilt
  adds 20 (40 by a Sith sword). Multipliers: attacker's Sith sword x2, attacker's saber damage scale above 1 x1.5,
  attacker in Rage x1.5, blocker at 20 block points or less x2. Rounded up. No wear while thrown or locked.
- Recovery: after 8 s without wear, -1 per second; once above 75 ("cracked", sparks) it never goes back to 75 or
  below.
- At 100 it breaks at the end of the current swing: sparks and a quick saber-off sound, the pieces become the new
  sabers; on the ground a heavy break knocks the fighter down (strength 300) and knocks the right-hand half away (it
  can be picked up or pulled back), a light break pushes back 150 and staggers; in the air push 300 / 150. Saber
  damage is halved for 1.5 s after a break. The HUD tints the style icon by wear.

### 9.2 Kicks with a weapon in hand

The "Kick" control kicks with any weapon in hand. Outcome on the kicked fighter:
- **Absorbed** (MD/MDA player: holding block, walking, kick from the front, 35+ block points and 35+ Force, mishap
  under 10, Saber Defense, on the ground, not busy): MDA: stagger if block points above 50 else stumble, -2 block
  points; MD: stagger if Force above 50 else stumble, -10 Force.
- **NPC absorb** (an NPC in its kick-block state, which lasts at least 5 s once started; not while fatigued): MDA:
  above 50 block points (or kick resistant) stagger, -2 block points, 20% saber switched off; else a back flip, -2.
  MD: same with Force, -10.
- **Not absorbed:** knockdown (BOTH_KNOCKDOWN1/2; NPCs stay down 1.5 s longer, the player 0.3 s), saber switched off;
  kick-resistant characters only stagger (20% saber off). Drain: MDA -5 block points, MD -10 Force. The kicker gains 2.
- NPC slaps and back kicks (MD/MDA, difficulty 2 and above, 25% when in reach, needing the enemy at mishap above 5 and
  meter 65 or less) do (difficulty + 1) x 2-5 damage and use the same kick outcome.

### 9.3 Gunner melee ("Gunner Melee", off by default)

Strong gunners (storm commandos, and storm troopers/rebels on the enemy team) melee the player at close range in
front: a lunge or a slap doing (difficulty + 1) x 2-5 damage, then the kick outcome above; 10-15 s between lunges.

### 9.4 Ledge grab

"Ledge Grab" (off by default) lets the character grab ledges; no combat rules found.

---

## 10. NPCs

- **Always blocking:** an NPC with a saber counts as manually blocking every frame unless taunting, in a kata, a
  guard, in a cutscene, at 1 health, knocked down, rolling, in a lock break, special attack, heavy bounce, bashed,
  special jump, airborne, saber off or thrown. Even while attacking it keeps parrying (until its mishap reaches 12).
  NPCs block bolts whenever the saber is ready (7.1).
- **Block stance (MDA):** when the enemy is within saber reach (saber length + 1.5 x body width + 16), the NPC enters
  a block stance and leaves it 32 units beyond; in it the NPC faces the enemy and holds position but still parries,
  dodges and attacks.
- **Costs and branches:** section 4 (NPC block row, NPC costs). NPCs never use the player's fake input.
- **Low meters:** MD/MDA NPC acrobats/fencers and any NPC with meter under 30 dodge more (jumps/rolls) when a swing
  would miss narrowly or their saber is away. MDA: an idle NPC far (256+) from a saber enemy with block points under
  30 meditates for +25 (Sith Lord, Desann, Vader, Luke use a quick Force-heal pose BOTH_FORCEHEAL_QUICK, others
  BOTH_MEDITATE_SABER), every 15-25 s. NPCs near a saber enemy (under 100, in front) walk instead of run at 60-70 units.
- **Style choice (MDA, within 200 of a saber enemy, re-rolled every 3-15 s):** against enemy block points above 75 a
  heavy style (if Saber Offense 3+), above 40 Medium, otherwise a fast style. Exact pick: not determined.
- **Retreat (MD/MDA):** within 128 of a saber enemy in a kata (or when its own saber is out of the hand) it backs off.
- **Dodges:** 35 Force each (needs 50/70).
- **Follow-ups (MD only):** Tavion, Alora, Yoda, shadowtroopers, projections, some Reborn and Jedi ranks attack
  straight out of a parry, in Fast style.
- **"Smart NPC" (console only, off by default, MDA only):** sense enemies without line of sight within 3500 units;
  dash-dodge out of katas (up to 2, 35 Force, 10-15 s cooldown); attack straight out of a parry or knockaway when the
  enemy has more than 50 block points and their own mishap is under 8 (Fast at difficulty 2+, else Fast/Medium); keep
  attacking with strafes and occasional lunges, backing off while attacking when at 30 health or under 30 meter;
  switch the saber off when no enemy is left; try jumps when blocked on the way to a goal.

---

## 11. Stances, walks and blocking poses

### 11.1 Cosmetic Saber Stance (the same setting is called "realistic blocking mode")

Shown in the menu only in MDA; On by default. On = character stances ("some stances may make you vulnerable");
Off = "realistic blocking", plain stances. It also matters for NPC stances and walks in every mode, and for the speed
option in 6.4.

### 11.2 Ready stance (player)

- **MDA, cosmetic on:** by style: Dual BOTH_SABERDUAL_STANCE, Staff BOTH_SABERSTAFF_STANCE, Fast BOTH_SABERFAST_STANCE,
  Tavion BOTH_SABERTAVION_STANCE, Strong BOTH_SABERSLOW_STANCE, Desann BOTH_SABERDESANN_STANCE, Medium and none
  BOTH_STAND2.
- **MDA, cosmetic off:** BOTH_STAND1; Sith Lord class BOTH_STAND9; Galen class BOTH_STAND2IDLE2; battle-droid style
  BOTH_STAND1_BDROID (Master style only).
- **MD and mode 0:** the stock stance table, with per-style variants under the Master animation style (for example
  BOTH_SABERDUAL_STANCE_JKA, _GALEN, _MD, _KOTOR, _DF2, _CAL, _GRIEV, _PAL, _MAUL; the same pattern for Staff and the
  single styles).
- Ducked: a crouch stance.

### 11.3 Ready stance (NPC, MDA)

Cosmetic off: in a cutscene the style stances above; otherwise the block stance (BOTH_STAND_BLOCKING_ON_DUAL and
character variants _ANI, _GALEN, _JANGO, _CAL, _GRIEV, _BEN) while in block stance, else a crouched guard
(BOTH_SABERDUALCROUCH, _GALEN, _KOTOR, _DF2; the single and staff equivalents). Cosmetic on: not fully determined
(the style stances, presumably).

### 11.4 Walks with the saber on (single saber, player)

- **MDA, Master style:** holding block uses the character's walk 1, not holding uses walk 2: Anakin BOTH_WALK1_ANI /
  BOTH_WALK2_ANI, Galen _GALEN, Obi-Wan EP3 _OBI3, Kylo Ren _REN, Cal Kestis _CAL, Grievous _GRIEV, Count Dooku _DOOKU.
  Ben Kenobi, Yoda, Rey, Jango, Battle Droid, Palpatine and Maul (_BEN, _YODA, _REY, _JANGO, _BDROID, _PAL, _MAUL) use
  walk 1 also when not blocking if cosmetic is off. Vader always BOTH_WALK1_VADER. Any other style BOTH_WALK1_MDA.
- **MDA, Classic style:** BOTH_WALK1_MDA.
- **MD, Master style:** the character's walk 2 (BOTH_WALK2_ANI, _BEN, _YODA, _VADER, _GALEN, _OBI3, _REN, _REY, _JANGO,
  _BDROID, _CAL, _GRIEV, _PAL, _DOOKU, _MAUL), default BOTH_WALK2_BEN.
- **MD, Classic style:** BOTH_WALK2 (the stock walk).
- **NPCs and cutscenes:** cosmetic off: BOTH_WALK2 (Vader in a cutscene BOTH_WALK1_MDA); cosmetic on: BOTH_WALK1_MDA.
- Dual and Staff walks: not determined in detail.
- Sprint animations: BOTH_SPRINT (and _BEN, _YODA, _VADER, _GALEN, _BDROID, _GRIEV, _PAL, _MAUL), saber sprint
  BOTH_SPRINT_SINGLE_LIGHTSABER and variants.

### 11.5 Blocking poses while block is held

**MD (single saber):** chosen from movement keys (not while jumping): back-left BOTH_P1_S1_TL, back-right BOTH_P1_S1_TR,
back BOTH_P1_S1_T_, forward-left BOTH_P1_S1_BL, forward-right BOTH_P1_S1_BR, forward BOTH_P1_S1_T_, left
BOTH_P1_S1_TL, right BOTH_P1_S1_TR; standing still: with attack BOTH_P1_S1_T_, without BOTH_STAND_BLOCKING_ON (Master
style: _ANI, _BEN, _YODA, _VADER, _GALEN, _REY, _JANGO, _CAL, _GRIEV, _DOOKU). Dual and Staff: the same pattern with
the Dual (P6_S6) and Staff (P7_S7) poses, not verified in detail.

**MDA (single saber):**
- block + attack with a direction: the MD-specific poses BOTH_P1_S1_TL_MD, _TR_MD, _T__MD, _BL_MD, _BR_MD;
- block + attack standing still: cosmetic on BOTH_P1_S1_T__MD; cosmetic off: with walk held BOTH_P1_S1_T_, else
  BOTH_STAND_BLOCKING_ON (Master style: Anakin BOTH_STAND_BLOCKING_ON_ANI, Ben BOTH_SABERFAST_STANCE_JKA_BEN, Yoda,
  Vader, Maul, Mace Windu, DF2, Obi-Wan EP3, Kylo BOTH_SABERTAVION_STANCE_JKA_YODA / _VADER / _MAUL / _MACE / _DF2 /
  _OBI3 / _REN);
- blocking lightning: always BOTH_P1_S1_T__MD;
- block only with a direction: cosmetic off a crouched guard BOTH_SABERSINGLECROUCH (_ANI, _VADER, _OBI, _DF2, _OBI3,
  _REN, _REY variants); cosmetic on BOTH_STAND_BLOCKING_ON_LEFT / _RIGHT (with character suffixes _ANI, _BEN, _YODA,
  _VADER, _GALEN, _REY, _CAL, _GRIEV, _DOOKU);
- block only standing still: cosmetic on BOTH_STAND_BLOCKING_ON; cosmetic off the crouched guard / style stance.
- Dual uses BOTH_P6_S6_* (TL, TR, T_, BL, BR), Staff BOTH_P7_S7_*.
- While the weapon timer runs and block is held, the fighter is kept in the current blocking pose until it may attack.

### 11.6 Animation style (Classic / Master)

A setting (menu location not determined) with two values: Classic (each character its own animations) and Master
(the new animation set). Master needs the Update 9 assets and is forced back to Classic on Update 8. With Master the
player picks a character style: default, Anakin, Battle Droid, Ben Kenobi, Cal Kestis, Clone Trooper, Dark Forces 2,
Count Dooku, Galen Marek, Qui-Gon Jinn, Grievous, Jango, KOTOR, Luke Skywalker, Mace Windu, Maul, Movie Duels, Obi-Wan,
Obi-Wan EP3, Palpatine, Rebels, Kylo Ren, Rey, Vader, Yoda. NPCs take theirs from their character file. A change takes
effect at the next map load; changed mid-map, the game saves and reloads itself once saving has been allowed for 1.5 s
(retrying every 3 s).

---

## 12. HUD

- **Block-point gauge (MDA, main HUD):** shown when not full; colour by block points: under 20 red, under 50 purple,
  under 75 dark purple, else bright magenta; under 7 it flashes with an overload sound every 400 ms. The HUD styles
  ("Hud Mode") draw it in their own way.
- **Mishap lamp:** shown when the count is above 0; flashes on a 600 ms cycle: green above 2, blue above 6, magenta
  above 10, red above 12 (with a warning sound). The Force number is hidden while a mishap or blaster mishap count is
  above 0 (JK2 HUD).
- **Overhead bars** (stacked over characters, faded with distance, 60 units wide at scale 1):
  - "Show Health Bar": health over all characters (red), also while Force Sight is on;
  - "Show Block Point Bar" (MD/MDA, saber users): MDA block points (magenta), MD Force (blue);
  - "Show Stamina Bar" (MD/MDA, saber users): 1 - mishap / 16 (grey).
- Screen tints in MD/MDA: cloak, burning at 15 health or less, stasis, and red at low health (25 or a third).

---

## 13. Settings that change saber combat

| Setting (menu label) | Options | Default | Effect |
|---|---|---|---|
| Saber Combat Mode | JKA Mode / MD Mode / MD Advanced Mode | JKA | everything in this document |
| Saber Attack Speed | JKA Mode / MD Mode | JKA | 6.4 |
| Cosmetic Saber Stance (MDA only) | On / Off | On | 11.1-11.5, 6.4 |
| Perfect Block Slowmo Action | Off / On | On | slow motion on perfect blocks and lock executions (3, 8.3) |
| Unblockable Attack Warning | Off / On | Off | red NPC tint (6.1) |
| Saber Damage | Low / Medium / High / Very High | Medium | see below |
| Saber Lock Executions | Off / On | On | super breaks and executions (8.3) |
| Saber Locks | Never / Rarely / Sometimes / Often | Rarely | lock chance 0/20/40/60% (8.1) |
| Saber Lock Camera | Off / On | Off | 8.2 |
| Gunner Melee | Off / On | Off | 9.3 |
| Saber Throw Return | Off / On | On | thrown saber returns after hitting a wall |
| Saber Wall Bounce | Off / On | Off | saber bounces off walls |
| Saberthrow Type | Vertical / Horizontal | Horizontal | throw arc |
| Saber Ignition Twirl, ignition speeds | | | cosmetic |
| Show Health / Block Point / Stamina Bar | Off / On | Off | 12 |
| Hud Mode | MD Classic / JKII Remastered / MD Vertical / MD Horizontal | MD Classic | HUD look |
| Auto-Health Regeneration | Off / On | Off | +1 health a second with Heal level 3, not while attacking, holding block, raging or poisoned |
| Ledge Grab | Off / On | Off | 9.4 |
| Controls: Manual Block/Sprint, Kick | | | 2, 9.2 |

**Saber Damage** (same in MD and MDA, normal swings): against an NPC with 30 or more health a hit does a fixed 10 / 25
/ 30 (Low / Medium / High) and cannot kill; under 30 health it does 5 / 15 / 35; Very High does 50 to 100. Special
moves do 25 to 75 (Low) or 25 to 100; stab-downs kill (at least 60, or 80 above Low); leap attacks 23-45 / 30-60 /
60-100; katas by style (for example Strong/Desann 30-60 / 40-80; others not determined). At Low, saber damage against
saber users is also scaled by hit location and capped (stock rule). At High and above, NPCs in cutscenes that are not
attacking do no damage.

Not in the menu but present: saber wear (9.1, default on for everyone), Smart NPC (10, off), the perfect block windows
(3), the 750 ms bolt window (unused), a debug tint showing when the player's saber does damage.

---

## Numbers

Format: name — value — unit — what it does — mode.

- saber meter maximum — 100 — points — top of the MDA block-point pool and the default Force max — both
- block point minimum to block — 1 — points — block button needs at least this (and Force at least 1) — MDA (Force: both)
- running block minimum — 5 — points — meter needed for a running block / sprint block — both
- swing cost, player — 1 — points — per new plain attack swing — MD Force / MDA block points
- NPC staff/dual knockaway cost — 2 — points — per knockaway started — both
- NPC knockaway-parry, back-block, blocking-animation cost — 1 — points — per move started — MDA
- NPC kick cost — 2 — Force — per kick started — MD
- master class saber block discount — 80 — percent paid — on blocker block-point costs vs sabers, rounded, min 1 — MDA
- master class bolt block discount — 60 — percent paid — on blocker block-point costs vs bolts, rounded, min 1 — MDA
- master class regen speed-up — 80 — percent of wait — block point regen waits shortened (25% faster) — MDA
- Force regen tick — 100 ms, +1 — ms, points — default interval and amount (NPC files may change) — both
- resting / crouching extra Force — 4 / 2 — points per tick — each also one mishap step down — both
- holding block Force delay — 4000 / 2000 — ms — next Force tick pushed back while block is held — MD / MDA
- NPC Force delays — 2000 — ms — after a tick when busy (MD) or saber thrown (both) — both
- NPC once-a-second Force gain — 1 — points/s — with saber on, not in bounce/knockdown/after block — MD
- block point regen tick — 100 ms, +1 — ms, points — player, walking, idle of attacks, not blocking — MDA
- block point resting / crouching extra — 4 / 2 — points per tick — player — MDA
- block point slow wait — 2000 — ms — added after a tick when above 75, saber thrown, or idle — MDA
- NPC block point regen — 1 — points/s — passive NPC regen — MDA
- NPC meditation — +25 when under 30, enemy 256+ away, every 15000-25000 — points, units, ms — MDA
- perfect block reward — 15 — points — blocker meter gain — both
- perfect block attacker cost — 10 (player) / 20 (NPC) — points — attacker meter loss — both
- disarm compensation — 20 — points — the disarmed fighter, or a low-meter blocker still holding the saber — both
- kill bonus — 10 (5 if the player killer holds block) — points — meter gain — both
- hurt bonuses — 5 lethal / 3 normal (2 if holding block) — points — player hits, difficulty 0-1 — both
- melee bonus — 2 — points — kicker on some kicks — both
- fake parry Force gain — 10 — Force — blocker that perfect/NPC-blocks a fake — both
- spammed-block attacker refill — 20 / 10 — points — NPC attacker gain when the blocker only holds block (20 if at 20 or less) — both
- fatigued thresholds — Force 20 (set at or under, cleared above); any drain to 10% of max — points — both
- drained thresholds — under 20 after a drain; under 10 block points at saber moves (MDA) — points — both
- low meter bands — 20 / 10 — points — at or under 20: fatigued knockaway costing 4; at or under 10: disarm / mishap — both
- active block cost — 5 — points — block+attack outside the window — MDA; MD player 5, NPC 10
- held block cost — 5 — points — block only — MDA player (NPC 0); MD player 5, NPC 10
- fatigued parry meter limit — 50 — points — at or under: fatigued parry for active/held block — both
- NPC parry bands — 40 / 75 — points — fatigued parry / plain parry / good parry — both
- NPC parry costs — 2 / 3 / 10 — points — fatigued / plain / good (player 3) — MDA
- NPC parry costs — 10 / 5 / 15 — points — fatigued / plain / good (player 5) — MD
- unblocked hit cost — 10 — points — blocker not holding block — MDA player, MD everyone
- unblockable not perfect — under 10 disarm (+20), else knockaway cost 10 (MD NPC 15) — points — both
- kata clash — under 10 block points disarm (+20), else stagger and cost 10 — block points — both
- thrown saber unparried — under 20 disarm (+10), else knockaway cost 10 — block points — MDA
- perfect window — 250 / 150 / 50 — ms — difficulty 0 / 1 / 2+, from pressing attack with block held — both
- bolt perfect window setting — 750 — ms — defined, never read — both
- held-too-long limit — 3000 — ms — block+attack held this long: no accurate parry, bolt cost x2 — both
- accurate parry cone — 0.3 — dot — attacker must be in front — both
- accurate parry threshold — 0.55 / 0.35 / 0.40 — dot — Fast / Strong / others — both
- NPC parry fallback — 50 x difficulty — per 1000 — chance an NPC passes a failed accurate test — both
- perfect block slow motion — 0.3 for 200 — time scale, ms — player perfect block and executions, option on — both
- block camera shake — 0.45 for 100 — strength, ms — player blocks and perfect blocks — both
- hit camera shake — 0.25 for 100 — strength, ms — MDA + speed option + cosmetic off — MDA
- riposte — freeze 350, armor -10, window 500, next attack time x0.87 — ms, armor, factor — MDA
- hit-confirm — 0.5, min 50 — factor, ms — attacker weapon timer after landing a hit — MDA
- mishap range — 0..15 — count — clamp — both
- mishap rises — swing +1, fake +1 (player every other chain count), blocked/clash/kata/superbreak-fail +3, perfect-blocked +5 player / +10 NPC, fake parried +2 — count — both
- mishap drops — block -3, perfect block -5, parrying a fake -4, fake breaking a guard -3 — count — both
- running balance factor — x2 rises, x0.5 drops — factor — mishap changes while running — both
- bounce ladder rungs — 5 small bounce / 12 heavy stagger (NPC small bounce, reset 5) / 15 roll (NPC 20% roll, 80% heavy stagger, reset 1) — count — both
- perfect-blocked roll rung — 13 — count — at or above: mishap roll instead of stagger — both
- mishap roll chance — count / 15 — probability — roll 1..15 at or under the count — both
- attacker mishap outcome — 50 stagger / 50 disarm; NPC disarm becomes stagger 20 — percent — both
- blocker mishap outcome — 50 stagger / 50 disarm branch; NPC blocker 25 stagger / 75 disarm (+20) — percent — both
- NPC block stagger chance — 20 — percent — NPC blocker with 75+ meter vs attacker at 12+ — both
- mishap decay — player 1/s (2/s above 12), NPC 1/s — count — when not busy, player not holding block — both
- kill/hurt mishap reset — 10 or more -> 5 — count — both
- attack fatigue — above 12 — count — swings x0.85 (speed option) — both
- HUD lamp — above 2 / 6 / 10 / 12: green / blue / magenta / red + sound, 600 ms cycle — count, ms — both
- block gauge — flash under 7 every 400 ms; colours at 20 / 50 / 75 — points, ms — MDA
- overhead bars — stamina 1 - count/16; width 60 virtual units at scale 1 — fraction, units — both
- NPC slaps/kicks — enemy mishap above 5 and meter 65 or less; 25% when in reach; difficulty 2+ — count, points, percent — both
- slap damage — (difficulty + 1) x 2..5 — health — NPC slap and gunner melee — all modes for gunners
- slap / back kick / gunner lunge delays — 20000-30000 (5000-10000 under 30 block points) / 15000-20000 (5000-10000 under 50 health) / 10000-15000 — ms — both
- kick absorb — needs 35 block points, 35 Force, mishap under 10; stagger above 50 meter else stumble — points — both
- kick drains — absorbed 2 block points / 10 Force; not absorbed 5 block points / 10 Force — points — MDA / MD
- NPC kick-block state — at least 5000 — ms — both
- knockdown hold — 1500 NPC / 300 player — ms — extra time down — both
- saber off on kick stagger — 20 — percent — kick-resistant or absorbing NPC — both
- NPC dodge — 35 Force, needs 50 (or 70) — Force — both
- smart NPC — dash 35 Force (needs 50, 2 dashes, 10000-15000 ms cooldown); sense range 3500; parry follow-up when enemy above 50 block points and own mishap under 8; backs off at 30 health or 30 meter — MDA
- NPC style switch — enemy block points above 75 / 40 → heavy / medium / fast; re-roll 3000-15000 ms within 200 units — MDA
- NPC distances — walk at 60-70 (under 100, in front); back off from katas at 128 — units — both
- NPC block stance — enter at 0, leave at 32 beyond reach (saber length + 1.5 x width + 16) — units — MDA
- disarm — thrown up at 350 units/s; protection 10000 / 8000 ms at difficulty 0 / 1, none at 2+ — both
- sprint fuel — needs above 15; start under 17 costs 10; drain 1 per 150 ms; refill 1 per 150 ms (75 resting/crouching, 250 under 15) — fuel, ms — both
- sprint speed — x1.6 (Vader x1.15, Yoda x1.65; Master: Yoda-style x1.2, Yoda x1.5) — factor — player sprint — both
- speed option factors — Fast 1.1, Medium 1, Strong/Desann 0.95, Tavion 0.9, Dual 0.9 (heavy bounce 0.6), Staff 0.9 (0.8), Yoda 1.25 — factor — Saber Attack Speed MD — all modes
- fatigue speed factors — 0.85 attack fatigue, 0.65/0.85 slow bounce, 0.65 after stagger, 0.90 parried, 0.85 blocked, 0.55 perfect-blocked bounce — factor — with the speed option — both
- cosmetic-off frame slowdown — Tavion 1.2, Dual 1.1, Staff 1.1 — frame time factor — MDA + speed option + cosmetic off — MDA
- chain limits — Strong 1-3, Medium/Dual 2-5, spinning 5 (Fast) / 3 — swings — stock chain rules — all modes
- chain momentum angles — 135..215, 180 — degrees — Strong chain continuation — all modes
- bolt block base cost — 1 — points — before modifiers — both
- bolt cost adds — Speed 10, running 5, airborne 10, walking+moving 2 — points — both
- bolt cost factors — broken parry 1.5, kicking 1.5, running player 2.5, airborne 2 (Dual 0.5), held 3 s+ 2 — factor — both
- aimed deflection cost — x2 — factor — block + attack — both
- controller accurate deflection — 2 points; aimed for 1500 ms — points, ms — walk+block+attack with a controller — both
- unblocking blade deflection cost — 12 — points — bolt hits a blade not blocking (MDA masters 7) — both
- deflection scatter, block only — MD 1..5 (back 0..1); MDA 1..3, 3..5 under 25, back with 50+ 0..1 — degrees — both
- deflection scatter, aimed — 0.1, 0.3 under 25 (MD 0.3 when swinging or under 65) — per axis — both
- aimed head scatter — +-4 sideways, -16..+4 vertical — units — both
- current-enemy pick — 75 — percent — aimed deflection target — both
- bolt block animation bands — 15 / 25 — points — fatigued / bounce / normal deflect — both
- lock chance — setting / 5 (0, 20, 40, 60); boss Kyle forces 1 in 3 — percent — per eligible clash — all modes
- lock conditions — distance 8..80, height difference 18, view cone 40, no pain for 1000 — units, degrees, ms — all modes
- lock camera — angle 40.5, pitch -11.25, side -25.5, height -15.5, range 82.5, FOV 31, blend 250 ms — degrees/units/ms — all modes
- super break — strength above 3 and roll 0..10 under it; bosses downgraded 80% — roll, percent — all modes
- execution — strength above 3 and roll 0..25 under it; victim player at 25 (and 10) health, NPC at 50; damage health + 10 (+1000 at 1000+) — all modes
- lock disarm — roll 0..10 under adjusted strength; loser stunned for its animation + 500 ms — all modes
- saber wear levels — breaks at 100 (end of swing), cracked above 75 (sparks, recovery floor) — wear — all modes
- saber wear gains — block 4 heavy / 2 Medium / 1 others; hilt hit 20 (40 Sith sword); x2 Sith sword, x1.5 damage scale, x1.5 Rage, x2 at 20 block points or less — wear — all modes
- saber wear recovery — 1 per 1000 ms after 8000 ms — wear — all modes
- saber break — knockdown 300 (heavy, ground), push 150 / 300, damage halved 1500 ms — strength, units/s, ms — all modes
- saber damage normal hit — 10 / 25 / 30 non-lethal (30+ health), 5 / 15 / 35 (under 30), 50-100 Very High — health — both
- saber damage special — 25-75 (Low) / 25-100 — health — both
- stab-down minimum — 60 (Low) / 80 — health — both
- leap attack damage — 23-45 / 30-60 / 60-100 — health — both
- auto health regen — 1 per second — health — option, Heal 3 — both

---

## Not determined

- Whether the NPC once-a-second regen of block points (MDA), Force (MD) and mishap decay really skip every other
  second: the code checks the parity of the game clock in milliseconds, which may always be even.
- Movement speed factor while holding block (none found).
- Whether the per-weapon bolt cost table ever applies (bolts do not seem to carry the shooter link it reads).
- Whether the "good" (no-scatter) block-only deflection case can ever happen.
- Use of the saber-versus-saber block cost table (Fast 12, Medium 15, Strong 17, Desann 16, Tavion 14, Dual/Staff 13,
  with lunge/spin x0.75, roll stab x2, jump attack x4, flip x2, fakes x1.25-1.35, running attacker x1.5 (Dual x3),
  accurate parry /3 (Fast /3.25), from behind x1.5-2): written, but not reached by blade clashes in this source.
- The NPC fake rule (when and how NPCs fake).
- Exact heavy/fast style picks of NPC style switching (MDA).
- NPC ready stance with cosmetic stances on (MDA), and Dual/Staff walk and MD block-pose details.
- The menu location of the Classic/Master animation style setting.
- The list of kick-resistant characters.
- Kata damage for styles other than Strong/Desann.
- Whether the player pays any meter for their own kicks (no cost found).
- Lightning blocking with the saber (an MDA cost exists; its amount was not traced).

---

## kjk mapping

What kjk builds of this spec, where, and what it leaves out. kjk offers two saber rules: JA's (the default) and
MDA (`SaberRules::DuelAdvanced`). MD, the spec's mode 1 (blocks paid from the Force pool), is not built: where the
spec says "both" or gives MD and MDA numbers, kjk plays MDA's. A snapshot, replay or setting that named MD (the
number 1, the name `md`) plays MDA. Every number is the rulebook's chapter `duel` (`data/rules/duel.toml`); every
path runs only under MDA, never under JA's rules. The probes are `kjk sim probes --only "duel rules"`
(`sim::trace::duel_probes`).

Terms. **The meter** is `Duel::points`, the block points; the Force, where a rule reads it, is counted in points of
1% of the pool (KOTOR's pools are not 100). **Player** is a seat's character, anyone else a creature. **Difficulty**: the sim has no
KOTOR difficulty setting yet (`GetGameDifficulty` answers Normal), so the duel modes read difficulty 1, KOTOR's
Normal (`duel.difficulty.level`): the 150 ms perfect window, the 8 s disarm guard, the 5% lucky parry, hit
rewards on, creature slaps off. **Masters** (the spec's master classes) are KOTOR's own: a character with Master
Jedi Defense (`duel.master.defense`, the top Jedi Defense tier) or of level 20 or more (`duel.master.level`).
Sheets keep no classes, so TSL's Jedi Master and Sith Lord prestige classes count through those (their holders
reach Master Jedi Defense and the late levels); K1 has no prestige classes. Masters pay less and refill sooner
(a Jedi with Advanced Jedi Defense takes half of that, with Jedi Defense none), their hilts never wear, and a lock's super break against one is often held to a plain break.

| Spec | Built | Where | Left out, and why |
|---|---|---|---|
| 1.1-1.2 meters, costs | block points; a player's plain swing 1, every swing a mishap, a creature's two-blade knockaway 2, its parries and knockaways 1; kicks taken; master's price (80%, bolts 60%) | `duel::meter::{started, spend, price}`, `motion::saber::begin_saber_move` | NPC dodges and dash dodges: kjk's creatures have no dodge to charge |
| 1.3 refill | block points (a player's tick and waits, crouching, masters; a creature's second), the Force a held block lets through (2000 ms), a thrown saber's creature's Force held, a flying saber no longer holding the Force, an unblockable swing holding it; gains: perfect, disarm, kill, hits (difficulty 0-1), kicks, fake parried, a blocked creature's refill, a scene's start (block points and mishaps only) | `duel::meter::{refill, rewarded, scene}`, `world::sabers::duel::duel_upkeep`, `world::powers` | resting (KOTOR has no meditation pose for a player), cloak's delay, Rage recovery's half rate of the Force; the scene's start refills no Force (KOTOR's Force, so a conversation would refill it), and in a scene every duel rule stands aside: it plays by JA's rules under every mode (`scene::Held::saber_rules`, Update 8's cinematic blocking taken further) |
| 1.4 fatigue | fatigued (Force at or under 20%, or a cost to 10), cleared on the duel's 100 ms tick; drained; no acrobatic get-up, lunges and spins unblocked, no kick absorbed | `duel::meter::{settle, spend}`, `motion::weapon::get_up`, `duel::exchange::resolve` | |
| 1.5 block points | kata clash, wear doubling, kick absorption | as above | the standing block lean and the clash sound (poses and sounds, section 11-12) |
| 2.1-2.3 the button | `holds::BLOCK`: a player's block while it may hold one (a lightsaber with Jedi Defense 1, or a melee blade it is proficient with: kjk's vibroblades and swords, which Jedi Academy has not; meters, on the ground, not busy), the saber lit for it, no swing and no damage while held, mishaps not falling | `duel::can_hold`, `motion::saber::duel_block`, `world::sabers` contacts | block walk (the walk key consumed); the blocking poses are the stance sets' (11) |
| 2.4 sprint | nothing: a block held while running blocks at the normal run speed, with at least 5 points in the meter | `motion::saber::duel_block`, `duel::runs_blocking` | the whole sprint, by the owner's choice: its speed, its fuel and its animations |
| 2.5 aimed block, perfect window | attack with block held aims the block, the press opens the window, a release ends it; held 3 s doubles bolt costs | `duel::Duel::guard`, `duel::window` | the controller's accurate missile block (no controller distinction in the sim) |
| 2.6 accurate parry | a blow is blocked from the front (dot 0.3); a creature parries one from aside by luck (5% a difficulty step) | `duel::in_front`, `world::sabers::duel::duel_guard` | comparing a player's pushed direction with the hit point (its block is decided by its buttons, as the spec notes) |
| 3 perfect block | +15 / -5 to the blocker, -10/-20 and +5/+10 to the attacker, a roll at 13 mishaps else a heavy stagger, the knockaway parry; against an unblockable blow the attacker disarmed; the riposte (attacker held 350 ms, chain reset, the defender's next attack in 500 ms quicker); `Duel::perfect_at` for the view's slow motion and shake | `duel::exchange`, `world::sabers::duel::duel_apply`, `duel::play_speed` | the riposte's armour loss (KOTOR has no armour pool to take from); the slow motion, shake, sound and bark are the view's to draw from `perfect_at`; 3.1's animation as the sim plays it: the sim's perfect parry is JA's knockaway as a saber move, where 3.1 plays a torso animation over the ready move with a 300 to 600 ms weapon timer (Movie Duels' knockaways play over the torso, see 11) |
| 4 the block decision | a creature's guard holds only on KOTOR's chance to miss (the attacker's attack bonus against its defence, one draw a swing a body, as under JA): KOTOR decides whether, the duel how and at what cost; the five states in order, the low-meter checks, every row's costs and parries (weary, plain, good), held block's refill to a creature, no block's cost and roll; the attacker side's order (unblockable, two swings, super break, fake, plain, none); attack versus attack with fakes and katas; the bounce, stagger and knockaway moves picked at random as the spec lists them; disarms thrown up at 350 with the difficulty guard, a refused disarm cancelling the swing | `duel::exchange::{resolve, ladder, mishap}`, `world::sabers::duel::{duel_meet, duel_took, duel_disarm}` | the fatigued parry plays the plain parry (kjk has no weary parry animation); camera shakes (the view's) |
| 4.7 other swing rules | the hit-confirm (the striker's weapon timer halved, at least 50 ms) and a blow on a wind-up cancelling it; a thrown saber not parried: disarm under 20 (+10), else knocked aside for 10 | `world::sabers::{confirmed, throw_hit}`, `world::sabers::duel::duel_thrown_hit` | a wind-up hit ignoring armour (no armour); partial-damage moves (kjk's sweep decides what deals damage) |
| 5 mishaps | the count, its rises and falls, running's factor, the ladder, the roll, decay, the reward calm, a scene's calm | `duel::meter::shift`, `duel::exchange`, `duel::meter::refill` | the HUD lamp (section 12); boss grabs, creature slaps (difficulty 2+), smart creatures (off) |
| 6.1 unblockable | the spec's list as kjk's moves, `Duel::warn` while one plays, no stance or Force refill during one, the kata power bonus (JA's already) | `duel::unblockable` | the red tint is the view's, from `warn` |
| 6.2 fakes | attack and use in a start or transition, keys or the opposite quadrant (a bottom either side), a mishap every other swing (never a pair's), ending when the swing does | `motion::saber::duel_fake` | creatures' fakes (not determined) |
| 6.3 chains | JA's, unchanged | | |
| 6.4 attack speed | the option (off, `duel.speed.option`): style paces replacing JA's transition paces, attack fatigue, the bounces' paces by why they bounced | `duel::play_speed`, `Duel::pace` | MDA's frame slowdown and hit shake with cosmetic stances off (section 11's setting) |
| 7 bolts | a player turns bolts only holding block with a lightsaber (a melee blade turns none), from any side, with the meter to pay; a creature as under JA (its bolt level from Jedi Defense and Deflect, its arc, its pause between bolts); the cost and its factors; toward the crosshair with scatter, back at the shooter's head (block and attack, twice the cost), a creature's at the chest; a blade not blocking turns them wildly for 12 (7 a master's); the poses by the meter | `duel::shots`, `world::sabers::duel::bolt_block` | the per-weapon table (not determined), the 75% current-enemy pick (the shooter is always the target), Saber Defense's extra scatter, the controller's accurate block |
| 8 locks | the setting's chance (Rarely, 1 in 5) on clashes and blocks that would try one, a fake's forced lock; super breaks and executions by the spec's rolls (masters and weak losers by KOTOR's health percent), else a disarm by strength; the loser held 500 ms longer | `world::sabers::duel::{duel_meet, duel_super_break, duel_lock_won}`, `world::sabers::end_lock` | the lock camera (off, the view's); boss Kyle's forced locks; the hand cut off (the kill is made, the dismemberment is the view's) |
| 9.1 wear | creatures' only (`duel.wear.who` 1; the spec wears everyone's, but a player's hilt would want repairs kjk has not got); a double-bladed lightsaber stands for the breakable staffs (KOTOR names no broken forms); blocked blows wear it by style, rage and a spent blocker; mending after 8 s, never back under cracked; at 100 it breaks once its swing ends: one blade and a single style, pushed back and staggered, until it is switched off (put away, or kicked out) and lit again | `duel::meter::{wear_of, wear}`, `world::sabers::duel::{duel_wear, duel_break}`, `motion::saber::duel_broken` | hilt hits (no hilt collision), the Sith sword and damage-scale factors (no such weapons), a heavy break's knockdown, the half knocked away, damage halved after a break |
| 9.2 kicks | `holds::KICK` kicks with any weapon (a lit blade's kick move, else the kick on the whole body); absorbed (stagger or stumble, small drains, a creature's saber out 20%) or floored with the saber out and the larger drains, a creature then guarding 5 s; the kicker +2 | `motion::weapon::duel_kick`, `world::sabers::duel::duel_kicked` | slaps and back kicks (difficulty 2+), kick-resistant characters (not determined) |
| 9.3-9.4 gunner melee, ledge grab | | | gunner melee is off by default; ledge grab has no combat rules |
| 10 creatures | they block whenever they can (and keep parrying while swinging until 12 mishaps), turn bolts, back away from a kata within 128; the block stance (a blade, 1.5 half widths and 16, left 32 farther; its ground held only within that reach, so a creature pushed out steps back in; none while it hunts its foe along a route, with no straight way to it), meditation for +25 every 15-25 s; none of it in a scene (see 1.3) | `world::sabers::duel` (`Man::guards`), `world::minds` (`Duelling`) | the meditation pose (no such animation in kjk's set), walking near a foe, the style choice (not fully determined), dodges, follow-ups of named characters, smart creatures (off) |
| 11 stances, walks, blocking poses | under MDA the sim plays Movie Duels' stance sets (`sim::skeleton::stance`, loaded by `game::stance_sets` from the skeletons Movie Duels offers and `data/duel_stances.toml`). A fighter with a lit blade stands, fidgets and walks in its set's sequences. Its set is its table row's (KOTOR's Jedi `_humanoid_kotor`, as Movie Duels' own models of them), or the one a player picked for it on the character page (`saber.stance.<who>`, sent to the host as `Action::PickStance`). A held block without attack plays the set's `BOTH_STAND_BLOCKING_ON` (`_DUAL`, `_STAFF`) standing still, and its `_FORWARD`, `_BACK`, `_LEFT` or `_RIGHT` guard over the torso while its holder moves (a move with a sideways part takes the side's); the legs walk on. With attack held it plays the parry toward the move (back or forward left `TL`/`BL`, right `TR`/`BR`, straight `T_`, sideways `TL`/`TR`), and standing still the top parry; two blades `P6_S6`, a staff `P7_S7`. A fighter without a set plays JA's own parry. A perfect parry (3.1) with one blade, met at the top left or the top right, plays Movie Duels' knockaway (`BOTH_K1_S1_TL_MD`, `BOTH_K1_S1_TR_MD`) over the torso, the legs in the held block's pose while the block is held. The view draws what the sim plays. Under JA's rules no fighter stands in a set | `sim::skeleton::stance`, `sim::world::stances`, `game::stance_sets`, `ui::screens::pages::Character` | per-character walk 1 / walk 2 by blocking (11.4); the Classic/Master animation style setting (11.6); of 3.1: the direction from where the blades met, the bounce poses of a blow met at the centre or low and of two blades and a staff (JA's knockaway plays there), the back parry (kjk blocks no blow from behind, 2.6), the weapon timer of 300 to 600 ms, a creature's good parry and the large characters' plain parry |
| 12-13 | the options page offers JA's and MDA, and Movie Duels' hilts only with Movie Duels installed beside Jedi Academy (greyed without it, "Needs Movie Duels installed beside Jedi Academy"; a saved duel mode then plays JA's rules) | `ui::screens::options`, `game::settings::movie_duels`, `vfs::Install::movie_duels` | the HUD (`game::duel_view`), the remaining settings: the view's or not in the sim's scope; the settings with options are rulebook rules (`duel.locks.setting`, `duel.locks.executions`, `duel.speed.option`, `duel.wear.who`) |
| kjk's own, not the spec's | under MDA only: a blow of two blades or a double-bladed one carries the second blade's own attack (KOTOR's second attack a round: the staff's whole, the pair's 60%, `saber_moves.styles.second_attack`), and a blade's graze (under `combat.flinch.blade_share` of a cut) flinches only by JA's odds; both by the pair's rules, JA's in a scene | `world::sabers::land` (`damage::Second`), `world::harm::hurt` | under JA neither: JA's blows and flinches are main's |

The rules above are kjk's own code from this spec. The sim's body, skeleton, `animation.cfg` and blades are read
from Jedi Academy's `base/` alone (`viewer_ja::Files::base`). Movie Duels' folder lends the view its hilts. Under MDA it
also lends the sim its stance sets, whose frames follow the skeleton's own (`docs/VFS.md`; the probe `kjk sim probes
--only "mod apart"`). Under JA, the default, nothing of this spec plays: the m6 trace is main's field for field.
