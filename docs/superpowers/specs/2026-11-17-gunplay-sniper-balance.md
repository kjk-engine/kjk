# Sniper Shot balance

The owner approved the gunplay design on 17 November 2026 on the condition
that Sniper Shot is balanced. This document reports the measurement that the
design asks for, the three checks, the one rule that was changed, and a
verdict on when Sniper Shot is worth using.

The measurement is a section of the combat bench. Anyone can run it again
with `kjk combat duel --only sniper_balance`. It takes about 40 seconds.

## What was measured

A soldier with no Force powers fires at a dummy that stands still. Each case
lasts 60 seconds of game time and is repeated with 8 different random seeds.
The numbers below are the averages.

- The soldier holds one blaster pistol, a pair of blaster pistols, or a
  blaster rifle.
- The soldier fires in one of these ways:
  - Plain fire from the hip. The player holds [Attack].
  - Plain fire while aiming. The player holds [Aim] and [Attack].
  - Power Blast at tier 1, 2 or 3. The player holds [Feat attack] until the
    charge is full, lets go to fire, and presses it again. This is measured
    from the hip and while aiming.
  - Sniper Shot at tier 1, 2 or 3. The player holds [Feat attack] until the
    shot is steady, lets go to fire, and presses it again. This is measured
    from the hip and while aiming.
- The dummy has low, middle or high Defense. Low Defense means a plain shot
  hits 95% of the time. Middle means 75% and high means 40%.
- The dummy stands 6 m or 30 m away.

A feat attack is paid with stamina. The soldier has 120 points of stamina
and each feat attack uses 18 of them. Stamina refills completely in 20
seconds. When the stamina runs out, holding [Feat attack] fires plain shots
until there is enough stamina for the next feat attack. The numbers are what
a player gets by keeping the button held for the whole minute. They include
those plain shots.

The table has these columns:

- **Damage a second** is the dummy's lost hit points divided by 60.
- **Stamina a second** is the stamina the feat attacks used, divided by 60.
- **Damage per point** is all the damage dealt in the minute divided by the
  stamina used. It is blank for plain fire, which uses no stamina.
- **Gain over plain aimed** compares the damage a second with plain fire
  while aiming, for the same gun, Defense and range.

## The three checks

All three checks pass with the tuned values.

1. **Sniper Shot beats plain fire from the hip against high Defense at
   30 m.** This passes for every gun, every tier, from the hip and while
   aiming. The smallest margin is a blaster rifle firing Sniper Shot 1 from
   the hip, with 1.10 damage a second against 0.52 for plain fire from the
   hip.
2. **Sniper Shot does not beat Power Blast against low Defense.** This was
   checked for every gun and tier, at both ranges, comparing Sniper Shot
   and Power Blast fired the same way (both from the hip, or both while
   aiming). It passes everywhere by a wide margin. Power Blast deals between
   2 and 6 times the damage a second of Sniper Shot of the same tier in
   these cells.
3. **One Sniper Shot deals more than plain aimed fire deals in the same
   time.** The time a Sniper Shot takes is its steadying time plus the gun's
   time between shots. The check is made against high Defense at 30 m, for
   Sniper Shots fired from the hip and while aiming. This is how the plan
   reads the design's wording "damage per point of the second bar stays
   above a plain aimed burst of the same length", because plain fire uses no
   stamina. It passes for every gun and tier. The smallest margin is a
   blaster rifle firing Sniper Shot 1 from the hip. One shot deals 1.22 hit
   points on average, and plain aimed fire deals 1.11 hit points in the
   700 ms the shot takes. That margin is about 10%.

One Sniper Shot against high Defense at 30 m, after tuning:

| Gun | Way of firing | Damage of one Sniper Shot | Time it takes | Plain aimed fire in that time |
|---|---|---|---|---|
| one pistol | Sniper Shot 1, hip | 2.79 | 1150 ms | 2.29 |
| one pistol | Sniper Shot 1, aimed | 2.77 | 1050 ms | 2.10 |
| one pistol | Sniper Shot 2, hip | 3.36 | 1150 ms | 2.29 |
| one pistol | Sniper Shot 2, aimed | 3.33 | 1050 ms | 2.10 |
| one pistol | Sniper Shot 3, hip | 3.46 | 1150 ms | 2.29 |
| one pistol | Sniper Shot 3, aimed | 3.52 | 1050 ms | 2.10 |
| a pair | Sniper Shot 1, hip | 4.39 | 1150 ms | 3.10 |
| a pair | Sniper Shot 1, aimed | 4.30 | 1050 ms | 2.83 |
| a pair | Sniper Shot 2, hip | 5.20 | 1150 ms | 3.10 |
| a pair | Sniper Shot 2, aimed | 5.13 | 1050 ms | 2.83 |
| a pair | Sniper Shot 3, hip | 5.93 | 1150 ms | 3.10 |
| a pair | Sniper Shot 3, aimed | 5.78 | 1050 ms | 2.83 |
| a rifle | Sniper Shot 1, hip | 1.22 | 700 ms | 1.11 |
| a rifle | Sniper Shot 1, aimed | 1.19 | 600 ms | 0.95 |
| a rifle | Sniper Shot 2, hip | 1.49 | 700 ms | 1.11 |
| a rifle | Sniper Shot 2, aimed | 1.37 | 600 ms | 0.95 |
| a rifle | Sniper Shot 3, hip | 1.46 | 700 ms | 1.11 |
| a rifle | Sniper Shot 3, aimed | 1.45 | 600 ms | 0.95 |

The damage of one shot is measured by firing Sniper Shots only while the
stamina can pay for them, about 26 shots a minute, over 8 seeds. The
averages move by about 5% from seed set to seed set. That is why the rifle's
tier 3 shot reads a little lower than its tier 2 shot.

## Rules changed from the design

Only one rule now differs from the design.

| Rule | Design | Now | Why |
|---|---|---|---|
| Sniper Shot steadying from the hip (`feats.specials.sniper_shot.steady_ms`, first value) | 500 ms | 350 ms | With 500 ms, check 3 failed for the blaster rifle at tier 1 from the hip. One shot dealt 1.21 hit points, and plain aimed fire dealt 1.34 hit points in the 850 ms that the shot took. |

The steadying time while aiming stays at 250 ms. The critical threat range
(2, 3 and 4 times) and the bonus to hit (+4, +6 and +8) are unchanged. The
shared cost of a feat attack (15% of the maximum stamina) is unchanged, and
no separate cost for Sniper Shot was needed.

Shortening the steadying time from the hip had one side effect in the
table. A blaster rifle that keeps [Feat attack] held from the hip at 30 m
now deals 1.10 damage a second against high Defense, down from 1.97 with
500 ms. Most of that damage comes from the plain shots fired while the
stamina is empty, not from the Sniper Shots. A rifle's spread and kick
settle after the rifle has not fired for 450 ms. A 500 ms steadying let
them settle before each Sniper Shot, so the plain shots after it started
accurate again. A 350 ms steadying is too short for that. Players who fire
a rifle at 30 m will aim, and with aiming this row is unchanged (1.82
before and 1.93 after).

## What the table shows

- **Sniper Shot pays off only against high Defense at long range.** There,
  keeping [Feat attack] held while aiming deals 4% to 40% more damage a
  second than plain aimed fire. Against high Defense at 6 m, Sniper Shot
  while aiming ranges from 5% less to 18% more than plain aimed fire.
  Against low Defense at 6 m it is worse than plain aimed fire at every
  tier, by 4% to 13%. Against low Defense at 30 m it ranges from 7% less to
  10% more.
- **Against low Defense, Sniper Shot's bonus to hit does nothing.** A
  plain shot already hits 95% of the time, which is the most any attack
  can. Only the wider critical threat range counts there.
- **At 6 m the spread barely matters.** A blaster pistol's bolt has no
  spread of its own, and the dummy stands still, so at 6 m almost every
  plain shot goes where it is aimed. Sniper Shot's lack of spread only
  shows at 30 m, where the spread and kick that build up under sustained
  fire make plain shots miss. Plain fire from the hip at 30 m loses about
  two thirds of its damage compared with plain aimed fire.
- **Every feat attack uses the same stamina a second, 7.80 points.**
  Stamina comes back at 6 points a second, and every feat line can fire
  faster than that. So every feat line fires about 26 feat attacks a
  minute, and damage per point of stamina follows damage a second.
- **Power Blast deals far more than Sniper Shot everywhere.** It deals 1.4
  to 3.9 times the damage a second of plain aimed fire, at every Defense and
  range. A charged Power Blast counts its charge as part of the attack's
  time, so it deals more damage for the time spent charging. A Sniper
  Shot's steadying time does not count, so it is time in which the gun does
  not fire.
- At 6 m, plain fire from the hip deals 6% to 12% more than plain aimed
  fire. This was not looked into here.

## Verdict

Sniper Shot now meets the three conditions the owner set. It is worth using
in one situation: against an enemy with high Defense at long range, fired
while aiming. There it adds up to 40% to plain aimed fire. Up close, or
against an enemy that is easy to hit, a player does as well or better with
plain aimed fire.

Sniper Shot is never the strongest feat attack for damage. Power Blast deals
1.4 to 6 times as much damage a second at the same tier, fired the same way. The gap comes from how
the time of each attack is counted, not from the numbers. Closing it with
the steadying time, the threat range or the bonus to hit would take values
far from the design, such as a critical hit on nearly every Sniper Shot.
This was not tried, because the owner's conditions do not ask Sniper Shot
to match Power Blast.

If the owner wants Sniper Shot to compete with Power Blast, there is a
structural option, which was not built. The steadying could overlap the
gun's time between shots instead of adding to it. The player could begin
holding [Feat attack] while the gun is still cooling down after the last
shot, so a chain of Sniper Shots would fire as fast as plain shots. Each
Sniper Shot would then be a plain shot that is more accurate and more likely
to be a critical hit, with nothing lost. Another option is to count the
steadying time as part of the attack's time, the way Power Blast counts its
charge, so that a steadied shot deals more damage.

## The full table

Damage is in hit points. Stamina is in points. The range is the distance between the soldier and the dummy.

| Gun | Way of firing | Defense | Range | Damage a second | Stamina a second | Damage per point | Gain over plain aimed |
|---|---|---|---|---|---|---|---|
| one pistol | plain, hip | low | 6 m | 4.06 | 0.00 |  | +6% |
| one pistol | plain, hip | low | 30 m | 1.10 | 0.00 |  | -63% |
| one pistol | plain, hip | middle | 6 m | 3.18 | 0.00 |  | +6% |
| one pistol | plain, hip | middle | 30 m | 0.86 | 0.00 |  | -63% |
| one pistol | plain, hip | high | 6 m | 2.73 | 0.00 |  | +6% |
| one pistol | plain, hip | high | 30 m | 0.75 | 0.00 |  | -63% |
| one pistol | plain, aimed | low | 6 m | 3.83 | 0.00 |  | +0% |
| one pistol | plain, aimed | low | 30 m | 2.99 | 0.00 |  | +0% |
| one pistol | plain, aimed | middle | 6 m | 3.00 | 0.00 |  | +0% |
| one pistol | plain, aimed | middle | 30 m | 2.31 | 0.00 |  | +0% |
| one pistol | plain, aimed | high | 6 m | 2.57 | 0.00 |  | +0% |
| one pistol | plain, aimed | high | 30 m | 2.00 | 0.00 |  | +0% |
| one pistol | Power Blast 1, hip | low | 6 m | 6.97 | 7.80 | 0.89 | +82% |
| one pistol | Power Blast 1, hip | low | 30 m | 6.86 | 7.80 | 0.88 | +130% |
| one pistol | Power Blast 1, hip | middle | 6 m | 5.24 | 7.80 | 0.67 | +75% |
| one pistol | Power Blast 1, hip | middle | 30 m | 5.13 | 7.80 | 0.66 | +122% |
| one pistol | Power Blast 1, hip | high | 6 m | 4.35 | 7.80 | 0.56 | +69% |
| one pistol | Power Blast 1, hip | high | 30 m | 4.28 | 7.80 | 0.55 | +114% |
| one pistol | Power Blast 1, aimed | low | 6 m | 6.97 | 7.80 | 0.89 | +82% |
| one pistol | Power Blast 1, aimed | low | 30 m | 7.01 | 7.80 | 0.90 | +135% |
| one pistol | Power Blast 1, aimed | middle | 6 m | 5.24 | 7.80 | 0.67 | +75% |
| one pistol | Power Blast 1, aimed | middle | 30 m | 5.25 | 7.80 | 0.67 | +127% |
| one pistol | Power Blast 1, aimed | high | 6 m | 4.35 | 7.80 | 0.56 | +69% |
| one pistol | Power Blast 1, aimed | high | 30 m | 4.37 | 7.80 | 0.56 | +119% |
| one pistol | Power Blast 2, hip | low | 6 m | 9.18 | 7.80 | 1.18 | +140% |
| one pistol | Power Blast 2, hip | low | 30 m | 9.01 | 7.80 | 1.16 | +202% |
| one pistol | Power Blast 2, hip | middle | 6 m | 6.88 | 7.80 | 0.88 | +130% |
| one pistol | Power Blast 2, hip | middle | 30 m | 6.74 | 7.80 | 0.86 | +192% |
| one pistol | Power Blast 2, hip | high | 6 m | 5.71 | 7.80 | 0.73 | +122% |
| one pistol | Power Blast 2, hip | high | 30 m | 5.61 | 7.80 | 0.72 | +181% |
| one pistol | Power Blast 2, aimed | low | 6 m | 9.18 | 7.80 | 1.18 | +140% |
| one pistol | Power Blast 2, aimed | low | 30 m | 9.18 | 7.80 | 1.18 | +207% |
| one pistol | Power Blast 2, aimed | middle | 6 m | 6.88 | 7.80 | 0.88 | +130% |
| one pistol | Power Blast 2, aimed | middle | 30 m | 6.87 | 7.80 | 0.88 | +197% |
| one pistol | Power Blast 2, aimed | high | 6 m | 5.71 | 7.80 | 0.73 | +122% |
| one pistol | Power Blast 2, aimed | high | 30 m | 5.72 | 7.80 | 0.73 | +186% |
| one pistol | Power Blast 3, hip | low | 6 m | 10.65 | 7.80 | 1.36 | +178% |
| one pistol | Power Blast 3, hip | low | 30 m | 10.45 | 7.80 | 1.34 | +250% |
| one pistol | Power Blast 3, hip | middle | 6 m | 7.98 | 7.80 | 1.02 | +166% |
| one pistol | Power Blast 3, hip | middle | 30 m | 7.81 | 7.80 | 1.00 | +238% |
| one pistol | Power Blast 3, hip | high | 6 m | 6.62 | 7.80 | 0.85 | +157% |
| one pistol | Power Blast 3, hip | high | 30 m | 6.50 | 7.80 | 0.83 | +226% |
| one pistol | Power Blast 3, aimed | low | 6 m | 10.65 | 7.80 | 1.36 | +178% |
| one pistol | Power Blast 3, aimed | low | 30 m | 10.63 | 7.80 | 1.36 | +256% |
| one pistol | Power Blast 3, aimed | middle | 6 m | 7.98 | 7.80 | 1.02 | +166% |
| one pistol | Power Blast 3, aimed | middle | 30 m | 7.95 | 7.80 | 1.02 | +244% |
| one pistol | Power Blast 3, aimed | high | 6 m | 6.62 | 7.80 | 0.85 | +157% |
| one pistol | Power Blast 3, aimed | high | 30 m | 6.61 | 7.80 | 0.85 | +231% |
| one pistol | Sniper Shot 1, hip | low | 6 m | 3.40 | 7.80 | 0.44 | -11% |
| one pistol | Sniper Shot 1, hip | low | 30 m | 2.93 | 7.80 | 0.38 | -2% |
| one pistol | Sniper Shot 1, hip | middle | 6 m | 2.95 | 7.80 | 0.38 | -2% |
| one pistol | Sniper Shot 1, hip | middle | 30 m | 2.56 | 7.80 | 0.33 | +11% |
| one pistol | Sniper Shot 1, hip | high | 6 m | 2.58 | 7.80 | 0.33 | +0% |
| one pistol | Sniper Shot 1, hip | high | 30 m | 2.23 | 7.80 | 0.29 | +12% |
| one pistol | Sniper Shot 1, aimed | low | 6 m | 3.60 | 7.80 | 0.46 | -6% |
| one pistol | Sniper Shot 1, aimed | low | 30 m | 3.02 | 7.80 | 0.39 | +1% |
| one pistol | Sniper Shot 1, aimed | middle | 6 m | 3.09 | 7.80 | 0.40 | +3% |
| one pistol | Sniper Shot 1, aimed | middle | 30 m | 2.65 | 7.80 | 0.34 | +15% |
| one pistol | Sniper Shot 1, aimed | high | 6 m | 2.68 | 7.80 | 0.34 | +4% |
| one pistol | Sniper Shot 1, aimed | high | 30 m | 2.33 | 7.80 | 0.30 | +17% |
| one pistol | Sniper Shot 2, hip | low | 6 m | 3.44 | 7.80 | 0.44 | -10% |
| one pistol | Sniper Shot 2, hip | low | 30 m | 3.12 | 7.80 | 0.40 | +5% |
| one pistol | Sniper Shot 2, hip | middle | 6 m | 3.00 | 7.80 | 0.38 | +0% |
| one pistol | Sniper Shot 2, hip | middle | 30 m | 2.77 | 7.80 | 0.35 | +20% |
| one pistol | Sniper Shot 2, hip | high | 6 m | 2.79 | 7.80 | 0.36 | +8% |
| one pistol | Sniper Shot 2, hip | high | 30 m | 2.58 | 7.80 | 0.33 | +29% |
| one pistol | Sniper Shot 2, aimed | low | 6 m | 3.68 | 7.80 | 0.47 | -4% |
| one pistol | Sniper Shot 2, aimed | low | 30 m | 3.13 | 7.80 | 0.40 | +5% |
| one pistol | Sniper Shot 2, aimed | middle | 6 m | 3.16 | 7.80 | 0.40 | +5% |
| one pistol | Sniper Shot 2, aimed | middle | 30 m | 2.74 | 7.80 | 0.35 | +19% |
| one pistol | Sniper Shot 2, aimed | high | 6 m | 2.90 | 7.80 | 0.37 | +12% |
| one pistol | Sniper Shot 2, aimed | high | 30 m | 2.55 | 7.80 | 0.33 | +28% |
| one pistol | Sniper Shot 3, hip | low | 6 m | 3.42 | 7.80 | 0.44 | -11% |
| one pistol | Sniper Shot 3, hip | low | 30 m | 3.19 | 7.80 | 0.41 | +7% |
| one pistol | Sniper Shot 3, hip | middle | 6 m | 3.00 | 7.80 | 0.39 | +0% |
| one pistol | Sniper Shot 3, hip | middle | 30 m | 2.83 | 7.80 | 0.36 | +23% |
| one pistol | Sniper Shot 3, hip | high | 6 m | 2.80 | 7.80 | 0.36 | +9% |
| one pistol | Sniper Shot 3, hip | high | 30 m | 2.65 | 7.80 | 0.34 | +33% |
| one pistol | Sniper Shot 3, aimed | low | 6 m | 3.63 | 7.80 | 0.46 | -5% |
| one pistol | Sniper Shot 3, aimed | low | 30 m | 3.15 | 7.80 | 0.40 | +5% |
| one pistol | Sniper Shot 3, aimed | middle | 6 m | 3.16 | 7.80 | 0.41 | +6% |
| one pistol | Sniper Shot 3, aimed | middle | 30 m | 2.76 | 7.80 | 0.35 | +20% |
| one pistol | Sniper Shot 3, aimed | high | 6 m | 2.93 | 7.80 | 0.38 | +14% |
| one pistol | Sniper Shot 3, aimed | high | 30 m | 2.58 | 7.80 | 0.33 | +29% |
| a pair | plain, hip | low | 6 m | 6.51 | 0.00 |  | +7% |
| a pair | plain, hip | low | 30 m | 1.49 | 0.00 |  | -67% |
| a pair | plain, hip | middle | 6 m | 4.72 | 0.00 |  | +7% |
| a pair | plain, hip | middle | 30 m | 1.09 | 0.00 |  | -67% |
| a pair | plain, hip | high | 6 m | 3.83 | 0.00 |  | +7% |
| a pair | plain, hip | high | 30 m | 0.88 | 0.00 |  | -67% |
| a pair | plain, aimed | low | 6 m | 6.10 | 0.00 |  | +0% |
| a pair | plain, aimed | low | 30 m | 4.57 | 0.00 |  | +0% |
| a pair | plain, aimed | middle | 6 m | 4.42 | 0.00 |  | +0% |
| a pair | plain, aimed | middle | 30 m | 3.32 | 0.00 |  | +0% |
| a pair | plain, aimed | high | 6 m | 3.59 | 0.00 |  | +0% |
| a pair | plain, aimed | high | 30 m | 2.69 | 0.00 |  | +0% |
| a pair | Power Blast 1, hip | low | 6 m | 10.11 | 7.80 | 1.30 | +66% |
| a pair | Power Blast 1, hip | low | 30 m | 9.78 | 7.80 | 1.25 | +114% |
| a pair | Power Blast 1, hip | middle | 6 m | 6.76 | 7.80 | 0.87 | +53% |
| a pair | Power Blast 1, hip | middle | 30 m | 6.46 | 7.80 | 0.83 | +95% |
| a pair | Power Blast 1, hip | high | 6 m | 5.09 | 7.80 | 0.65 | +42% |
| a pair | Power Blast 1, hip | high | 30 m | 4.87 | 7.80 | 0.62 | +81% |
| a pair | Power Blast 1, aimed | low | 6 m | 10.08 | 7.80 | 1.29 | +65% |
| a pair | Power Blast 1, aimed | low | 30 m | 10.09 | 7.80 | 1.29 | +121% |
| a pair | Power Blast 1, aimed | middle | 6 m | 6.73 | 7.80 | 0.86 | +52% |
| a pair | Power Blast 1, aimed | middle | 30 m | 6.71 | 7.80 | 0.86 | +102% |
| a pair | Power Blast 1, aimed | high | 6 m | 5.07 | 7.80 | 0.65 | +41% |
| a pair | Power Blast 1, aimed | high | 30 m | 5.07 | 7.80 | 0.65 | +88% |
| a pair | Power Blast 2, hip | low | 6 m | 13.36 | 7.80 | 1.71 | +119% |
| a pair | Power Blast 2, hip | low | 30 m | 13.02 | 7.80 | 1.67 | +185% |
| a pair | Power Blast 2, hip | middle | 6 m | 8.91 | 7.80 | 1.14 | +102% |
| a pair | Power Blast 2, hip | middle | 30 m | 8.60 | 7.80 | 1.10 | +159% |
| a pair | Power Blast 2, hip | high | 6 m | 6.70 | 7.80 | 0.86 | +87% |
| a pair | Power Blast 2, hip | high | 30 m | 6.47 | 7.80 | 0.83 | +140% |
| a pair | Power Blast 2, aimed | low | 6 m | 13.34 | 7.80 | 1.71 | +119% |
| a pair | Power Blast 2, aimed | low | 30 m | 13.35 | 7.80 | 1.71 | +192% |
| a pair | Power Blast 2, aimed | middle | 6 m | 8.89 | 7.80 | 1.14 | +101% |
| a pair | Power Blast 2, aimed | middle | 30 m | 8.85 | 7.80 | 1.14 | +167% |
| a pair | Power Blast 2, aimed | high | 6 m | 6.68 | 7.80 | 0.86 | +86% |
| a pair | Power Blast 2, aimed | high | 30 m | 6.68 | 7.80 | 0.86 | +148% |
| a pair | Power Blast 3, hip | low | 6 m | 15.53 | 7.80 | 1.99 | +155% |
| a pair | Power Blast 3, hip | low | 30 m | 15.18 | 7.80 | 1.95 | +232% |
| a pair | Power Blast 3, hip | middle | 6 m | 10.34 | 7.80 | 1.33 | +134% |
| a pair | Power Blast 3, hip | middle | 30 m | 10.02 | 7.80 | 1.28 | +202% |
| a pair | Power Blast 3, hip | high | 6 m | 7.77 | 7.80 | 1.00 | +117% |
| a pair | Power Blast 3, hip | high | 30 m | 7.54 | 7.80 | 0.97 | +180% |
| a pair | Power Blast 3, aimed | low | 6 m | 15.50 | 7.80 | 1.99 | +154% |
| a pair | Power Blast 3, aimed | low | 30 m | 15.52 | 7.80 | 1.99 | +239% |
| a pair | Power Blast 3, aimed | middle | 6 m | 10.32 | 7.80 | 1.32 | +134% |
| a pair | Power Blast 3, aimed | middle | 30 m | 10.28 | 7.80 | 1.32 | +210% |
| a pair | Power Blast 3, aimed | high | 6 m | 7.75 | 7.80 | 0.99 | +116% |
| a pair | Power Blast 3, aimed | high | 30 m | 7.76 | 7.80 | 0.99 | +188% |
| a pair | Sniper Shot 1, hip | low | 6 m | 5.53 | 7.80 | 0.71 | -9% |
| a pair | Sniper Shot 1, hip | low | 30 m | 3.13 | 7.80 | 0.40 | -32% |
| a pair | Sniper Shot 1, hip | middle | 6 m | 4.33 | 7.80 | 0.56 | -2% |
| a pair | Sniper Shot 1, hip | middle | 30 m | 2.56 | 7.80 | 0.33 | -23% |
| a pair | Sniper Shot 1, hip | high | 6 m | 3.63 | 7.80 | 0.47 | +1% |
| a pair | Sniper Shot 1, hip | high | 30 m | 2.19 | 7.80 | 0.28 | -19% |
| a pair | Sniper Shot 1, aimed | low | 6 m | 5.31 | 7.80 | 0.68 | -13% |
| a pair | Sniper Shot 1, aimed | low | 30 m | 4.28 | 7.80 | 0.55 | -7% |
| a pair | Sniper Shot 1, aimed | middle | 6 m | 4.15 | 7.80 | 0.53 | -6% |
| a pair | Sniper Shot 1, aimed | middle | 30 m | 3.34 | 7.80 | 0.43 | +1% |
| a pair | Sniper Shot 1, aimed | high | 6 m | 3.47 | 7.80 | 0.44 | -3% |
| a pair | Sniper Shot 1, aimed | high | 30 m | 2.81 | 7.80 | 0.36 | +4% |
| a pair | Sniper Shot 2, hip | low | 6 m | 5.82 | 7.80 | 0.75 | -5% |
| a pair | Sniper Shot 2, hip | low | 30 m | 3.49 | 7.80 | 0.45 | -24% |
| a pair | Sniper Shot 2, hip | middle | 6 m | 4.69 | 7.80 | 0.60 | +6% |
| a pair | Sniper Shot 2, hip | middle | 30 m | 2.94 | 7.80 | 0.38 | -12% |
| a pair | Sniper Shot 2, hip | high | 6 m | 4.02 | 7.80 | 0.52 | +12% |
| a pair | Sniper Shot 2, hip | high | 30 m | 2.61 | 7.80 | 0.33 | -3% |
| a pair | Sniper Shot 2, aimed | low | 6 m | 5.52 | 7.80 | 0.71 | -10% |
| a pair | Sniper Shot 2, aimed | low | 30 m | 4.55 | 7.80 | 0.58 | -0% |
| a pair | Sniper Shot 2, aimed | middle | 6 m | 4.43 | 7.80 | 0.57 | +0% |
| a pair | Sniper Shot 2, aimed | middle | 30 m | 3.72 | 7.80 | 0.48 | +12% |
| a pair | Sniper Shot 2, aimed | high | 6 m | 3.80 | 7.80 | 0.49 | +6% |
| a pair | Sniper Shot 2, aimed | high | 30 m | 3.19 | 7.80 | 0.41 | +19% |
| a pair | Sniper Shot 3, hip | low | 6 m | 6.04 | 7.80 | 0.77 | -1% |
| a pair | Sniper Shot 3, hip | low | 30 m | 3.64 | 7.80 | 0.47 | -20% |
| a pair | Sniper Shot 3, hip | middle | 6 m | 4.97 | 7.80 | 0.64 | +13% |
| a pair | Sniper Shot 3, hip | middle | 30 m | 3.20 | 7.80 | 0.41 | -4% |
| a pair | Sniper Shot 3, hip | high | 6 m | 4.40 | 7.80 | 0.56 | +23% |
| a pair | Sniper Shot 3, hip | high | 30 m | 2.94 | 7.80 | 0.38 | +9% |
| a pair | Sniper Shot 3, aimed | low | 6 m | 5.79 | 7.80 | 0.74 | -5% |
| a pair | Sniper Shot 3, aimed | low | 30 m | 4.96 | 7.80 | 0.64 | +8% |
| a pair | Sniper Shot 3, aimed | middle | 6 m | 4.78 | 7.80 | 0.61 | +8% |
| a pair | Sniper Shot 3, aimed | middle | 30 m | 4.18 | 7.80 | 0.54 | +26% |
| a pair | Sniper Shot 3, aimed | high | 6 m | 4.23 | 7.80 | 0.54 | +18% |
| a pair | Sniper Shot 3, aimed | high | 30 m | 3.76 | 7.80 | 0.48 | +40% |
| a rifle | plain, hip | low | 6 m | 4.33 | 0.00 |  | +12% |
| a rifle | plain, hip | low | 30 m | 0.77 | 0.00 |  | -67% |
| a rifle | plain, hip | middle | 6 m | 3.37 | 0.00 |  | +12% |
| a rifle | plain, hip | middle | 30 m | 0.60 | 0.00 |  | -67% |
| a rifle | plain, hip | high | 6 m | 2.91 | 0.00 |  | +12% |
| a rifle | plain, hip | high | 30 m | 0.52 | 0.00 |  | -67% |
| a rifle | plain, aimed | low | 6 m | 3.86 | 0.00 |  | +0% |
| a rifle | plain, aimed | low | 30 m | 2.36 | 0.00 |  | +0% |
| a rifle | plain, aimed | middle | 6 m | 3.00 | 0.00 |  | +0% |
| a rifle | plain, aimed | middle | 30 m | 1.84 | 0.00 |  | +0% |
| a rifle | plain, aimed | high | 6 m | 2.59 | 0.00 |  | +0% |
| a rifle | plain, aimed | high | 30 m | 1.58 | 0.00 |  | +0% |
| a rifle | Power Blast 1, hip | low | 6 m | 6.16 | 7.80 | 0.79 | +59% |
| a rifle | Power Blast 1, hip | low | 30 m | 6.13 | 7.80 | 0.79 | +160% |
| a rifle | Power Blast 1, hip | middle | 6 m | 4.59 | 7.80 | 0.59 | +53% |
| a rifle | Power Blast 1, hip | middle | 30 m | 4.57 | 7.80 | 0.59 | +149% |
| a rifle | Power Blast 1, hip | high | 6 m | 3.84 | 7.80 | 0.49 | +48% |
| a rifle | Power Blast 1, hip | high | 30 m | 3.83 | 7.80 | 0.49 | +143% |
| a rifle | Power Blast 1, aimed | low | 6 m | 6.16 | 7.80 | 0.79 | +59% |
| a rifle | Power Blast 1, aimed | low | 30 m | 6.39 | 7.80 | 0.82 | +171% |
| a rifle | Power Blast 1, aimed | middle | 6 m | 4.59 | 7.80 | 0.59 | +53% |
| a rifle | Power Blast 1, aimed | middle | 30 m | 4.76 | 7.80 | 0.61 | +159% |
| a rifle | Power Blast 1, aimed | high | 6 m | 3.84 | 7.80 | 0.49 | +48% |
| a rifle | Power Blast 1, aimed | high | 30 m | 3.98 | 7.80 | 0.51 | +152% |
| a rifle | Power Blast 2, hip | low | 6 m | 7.83 | 7.80 | 1.00 | +103% |
| a rifle | Power Blast 2, hip | low | 30 m | 7.80 | 7.80 | 1.00 | +231% |
| a rifle | Power Blast 2, hip | middle | 6 m | 5.83 | 7.80 | 0.75 | +94% |
| a rifle | Power Blast 2, hip | middle | 30 m | 5.81 | 7.80 | 0.74 | +216% |
| a rifle | Power Blast 2, hip | high | 6 m | 4.85 | 7.80 | 0.62 | +88% |
| a rifle | Power Blast 2, hip | high | 30 m | 4.86 | 7.80 | 0.62 | +207% |
| a rifle | Power Blast 2, aimed | low | 6 m | 7.83 | 7.80 | 1.00 | +103% |
| a rifle | Power Blast 2, aimed | low | 30 m | 8.05 | 7.80 | 1.03 | +242% |
| a rifle | Power Blast 2, aimed | middle | 6 m | 5.83 | 7.80 | 0.75 | +94% |
| a rifle | Power Blast 2, aimed | middle | 30 m | 5.97 | 7.80 | 0.77 | +225% |
| a rifle | Power Blast 2, aimed | high | 6 m | 4.85 | 7.80 | 0.62 | +88% |
| a rifle | Power Blast 2, aimed | high | 30 m | 4.99 | 7.80 | 0.64 | +216% |
| a rifle | Power Blast 3, hip | low | 6 m | 8.95 | 7.80 | 1.15 | +132% |
| a rifle | Power Blast 3, hip | low | 30 m | 8.91 | 7.80 | 1.14 | +278% |
| a rifle | Power Blast 3, hip | middle | 6 m | 6.65 | 7.80 | 0.85 | +122% |
| a rifle | Power Blast 3, hip | middle | 30 m | 6.63 | 7.80 | 0.85 | +260% |
| a rifle | Power Blast 3, hip | high | 6 m | 5.53 | 7.80 | 0.71 | +114% |
| a rifle | Power Blast 3, hip | high | 30 m | 5.54 | 7.80 | 0.71 | +251% |
| a rifle | Power Blast 3, aimed | low | 6 m | 8.95 | 7.80 | 1.15 | +132% |
| a rifle | Power Blast 3, aimed | low | 30 m | 9.16 | 7.80 | 1.17 | +289% |
| a rifle | Power Blast 3, aimed | middle | 6 m | 6.65 | 7.80 | 0.85 | +122% |
| a rifle | Power Blast 3, aimed | middle | 30 m | 6.78 | 7.80 | 0.87 | +269% |
| a rifle | Power Blast 3, aimed | high | 6 m | 5.53 | 7.80 | 0.71 | +114% |
| a rifle | Power Blast 3, aimed | high | 30 m | 5.66 | 7.80 | 0.73 | +258% |
| a rifle | Sniper Shot 1, hip | low | 6 m | 3.48 | 7.80 | 0.45 | -10% |
| a rifle | Sniper Shot 1, hip | low | 30 m | 1.46 | 7.80 | 0.19 | -38% |
| a rifle | Sniper Shot 1, hip | middle | 6 m | 2.84 | 7.80 | 0.36 | -5% |
| a rifle | Sniper Shot 1, hip | middle | 30 m | 1.26 | 7.80 | 0.16 | -31% |
| a rifle | Sniper Shot 1, hip | high | 6 m | 2.47 | 7.80 | 0.32 | -5% |
| a rifle | Sniper Shot 1, hip | high | 30 m | 1.10 | 7.80 | 0.14 | -30% |
| a rifle | Sniper Shot 1, aimed | low | 6 m | 3.47 | 7.80 | 0.45 | -10% |
| a rifle | Sniper Shot 1, aimed | low | 30 m | 2.50 | 7.80 | 0.32 | +6% |
| a rifle | Sniper Shot 1, aimed | middle | 6 m | 2.82 | 7.80 | 0.36 | -6% |
| a rifle | Sniper Shot 1, aimed | middle | 30 m | 2.08 | 7.80 | 0.27 | +13% |
| a rifle | Sniper Shot 1, aimed | high | 6 m | 2.46 | 7.80 | 0.32 | -5% |
| a rifle | Sniper Shot 1, aimed | high | 30 m | 1.82 | 7.80 | 0.23 | +15% |
| a rifle | Sniper Shot 2, hip | low | 6 m | 3.49 | 7.80 | 0.45 | -10% |
| a rifle | Sniper Shot 2, hip | low | 30 m | 1.51 | 7.80 | 0.19 | -36% |
| a rifle | Sniper Shot 2, hip | middle | 6 m | 2.85 | 7.80 | 0.37 | -5% |
| a rifle | Sniper Shot 2, hip | middle | 30 m | 1.30 | 7.80 | 0.17 | -29% |
| a rifle | Sniper Shot 2, hip | high | 6 m | 2.54 | 7.80 | 0.33 | -2% |
| a rifle | Sniper Shot 2, hip | high | 30 m | 1.21 | 7.80 | 0.16 | -23% |
| a rifle | Sniper Shot 2, aimed | low | 6 m | 3.48 | 7.80 | 0.45 | -10% |
| a rifle | Sniper Shot 2, aimed | low | 30 m | 2.57 | 7.80 | 0.33 | +9% |
| a rifle | Sniper Shot 2, aimed | middle | 6 m | 2.84 | 7.80 | 0.36 | -5% |
| a rifle | Sniper Shot 2, aimed | middle | 30 m | 2.13 | 7.80 | 0.27 | +16% |
| a rifle | Sniper Shot 2, aimed | high | 6 m | 2.54 | 7.80 | 0.33 | -2% |
| a rifle | Sniper Shot 2, aimed | high | 30 m | 1.93 | 7.80 | 0.25 | +22% |
| a rifle | Sniper Shot 3, hip | low | 6 m | 3.53 | 7.80 | 0.45 | -9% |
| a rifle | Sniper Shot 3, hip | low | 30 m | 1.50 | 7.80 | 0.19 | -36% |
| a rifle | Sniper Shot 3, hip | middle | 6 m | 2.90 | 7.80 | 0.37 | -3% |
| a rifle | Sniper Shot 3, hip | middle | 30 m | 1.30 | 7.80 | 0.17 | -30% |
| a rifle | Sniper Shot 3, hip | high | 6 m | 2.59 | 7.80 | 0.33 | +0% |
| a rifle | Sniper Shot 3, hip | high | 30 m | 1.21 | 7.80 | 0.16 | -23% |
| a rifle | Sniper Shot 3, aimed | low | 6 m | 3.53 | 7.80 | 0.45 | -9% |
| a rifle | Sniper Shot 3, aimed | low | 30 m | 2.60 | 7.80 | 0.33 | +10% |
| a rifle | Sniper Shot 3, aimed | middle | 6 m | 2.88 | 7.80 | 0.37 | -4% |
| a rifle | Sniper Shot 3, aimed | middle | 30 m | 2.16 | 7.80 | 0.28 | +17% |
| a rifle | Sniper Shot 3, aimed | high | 6 m | 2.58 | 7.80 | 0.33 | -0% |
| a rifle | Sniper Shot 3, aimed | high | 30 m | 1.95 | 7.80 | 0.25 | +24% |
