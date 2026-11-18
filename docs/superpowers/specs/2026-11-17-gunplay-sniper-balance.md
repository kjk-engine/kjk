# Sniper Shot balance

The owner approved the gunplay design on 17 November 2026 on the condition
that Sniper Shot is balanced. This document reports the measurement that the
design asks for, the three checks, one change to how Sniper Shot works, and
a verdict on when Sniper Shot is worth using.

The measurement is a section of the combat bench. Anyone can run it again
with `kjk combat duel --only sniper_balance`. It takes about a minute.

## What changed since the design

The design had a Sniper Shot steady only once the gun was ready to fire.
The steadying time then came on top of the gun's time between shots. A
first measurement showed that this made Sniper Shot weak almost everywhere.
A pistol that fired Sniper Shots spent 1.3 seconds on each of them instead
of the pistol's normal 0.8 seconds. A rifle spent 0.85 seconds instead of
0.35 seconds.

Sniper Shot now works like this, which the design did not have:

- The player can press [Feat attack] and start to steady the gun while the
  gun is still cooling down after the last shot.
- If the player lets go once the gun is steady, but before the gun is ready
  to fire again, the shot leaves on the first moment the gun is ready.
- Until the shot leaves, the character still counts as steadying. The
  character walks, and a stun, a knockdown, a weapon switch, putting the
  gun away or using a medpac drops the shot. A dropped shot fires nothing
  and uses no stamina.
- Letting go before the gun is steady still fires nothing and uses nothing.

So when the steadying time is shorter than the gun's time between shots,
Sniper Shots in a row leave at the gun's own rate. A blaster rifle steadies
in 250 ms while aiming, and its shots are 350 ms apart, so an aiming rifle
fires Sniper Shots exactly as fast as plain shots. A pistol's shots are
800 ms apart, so a pistol fires Sniper Shots as fast as plain shots from
the hip and while aiming.

No rule value differs from the design. The steadying takes 500 ms from the
hip and 250 ms while aiming. The bonus to hit is +4, +6 and +8 for the three
tiers. The critical threat range is multiplied by 2, 3 and 4. The cost of a
feat attack is 15% of the character's maximum stamina, the same for every
feat attack. With the new steadying, all three checks pass with these
values, so nothing was tuned.

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
    shot is steady, lets go, and presses it again on the step after the
    shot leaves. This is measured from the hip and while aiming.
- The dummy has low, middle or high Defense. Low Defense means a plain shot
  hits 95% of the time. Middle means 75% and high means 40%.
- The dummy stands 6 m or 30 m away.
- The soldier aims at the middle of the dummy's body. The reason is given
  under "Where the soldier aims" below.

A feat attack is paid with stamina. The soldier has 120 points of stamina,
each feat attack uses 18 of them, and stamina comes back at 6 points a
second. When the stamina runs out, holding [Feat attack] fires plain shots
until there is enough stamina for the next feat attack. The damage a second
of a feat attack is what a player gets by keeping the button held for the
whole minute, and it includes those plain shots.

Every feat attack case is also played a second time. In the second run the
soldier presses [Feat attack] only when the stamina can pay for it and never
holds [Attack], so every shot is a feat attack. That run measures what the
feat attack alone deals for its stamina.

The full table at the end has these columns:

- **Damage a second** is the dummy's lost hit points divided by 60.
- **Stamina a second** is the stamina the soldier lost in the minute,
  divided by 60. It is measured from every drop of the soldier's stamina.
- **Plain shots** is the share of the shots in the minute that were plain
  shots fired while the stamina was empty. A shot here is a moment the
  soldier's gun fired, so a pair firing a Sniper Shot or a Power Blast from
  both guns at once counts as one shot.
- **Damage per point, feat alone** is the damage a feat attack deals for
  each point of stamina it uses, from the second run. It is blank for plain
  fire, which uses no stamina.
- **Gain over plain aimed** compares the damage a second with plain fire
  while aiming, for the same gun, Defense and range.

## The three checks

All three checks pass.

1. **Sniper Shot beats plain fire from the hip against high Defense at
   30 m.** This passes for every gun and tier, from the hip and while
   aiming. Sniper Shot deals at least 2.9 times the damage a second of plain
   fire from the hip there. The smallest margin is a pair of pistols firing
   Sniper Shot 1 from the hip. This check says little about the aimed
   rows. Plain fire from the hip at 30 m deals 61% to 73% less than plain
   aimed fire, so anything aimed beats it. For the aimed rows, the gain over
   plain aimed fire is the comparison that matters.
2. **Sniper Shot does not beat Power Blast against low Defense.** This was
   checked for every gun and tier, at both ranges, comparing Sniper Shot
   and Power Blast fired the same way (both from the hip, or both while
   aiming). It passes everywhere. Against low Defense, Power Blast deals
   1.7 to 4.1 times the damage a second of Sniper Shot of the same tier.
3. **One Sniper Shot deals more than plain aimed fire deals in the same
   time.** The check is made against high Defense at 30 m, for Sniper Shots
   from the hip and while aiming. This is how the plan reads the design's
   wording "damage per point of the second bar stays above a plain aimed
   burst of the same length", because plain fire uses no stamina. The time
   a Sniper Shot takes in a chain is now the longer of its steadying time
   and the gun's time between shots. It passes for every gun and tier. The
   smallest margin is a blaster rifle firing Sniper Shot 1 from the hip. One
   shot deals 1.44 hit points on average, and plain aimed fire deals 0.90
   hit points in the 500 ms the shot takes.

One Sniper Shot against high Defense at 30 m:

| Gun | Way of firing | Damage of one Sniper Shot | Time it takes in a chain | Plain aimed fire in that time |
|---|---|---|---|---|
| one pistol | Sniper Shot 1, hip | 3.30 | 800 ms | 1.61 |
| one pistol | Sniper Shot 1, aimed | 3.30 | 800 ms | 1.61 |
| one pistol | Sniper Shot 2, hip | 4.00 | 800 ms | 1.61 |
| one pistol | Sniper Shot 2, aimed | 4.00 | 800 ms | 1.61 |
| one pistol | Sniper Shot 3, hip | 4.05 | 800 ms | 1.61 |
| one pistol | Sniper Shot 3, aimed | 4.05 | 800 ms | 1.61 |
| a pair | Sniper Shot 1, hip | 5.21 | 800 ms | 2.18 |
| a pair | Sniper Shot 1, aimed | 5.21 | 800 ms | 2.18 |
| a pair | Sniper Shot 2, hip | 6.11 | 800 ms | 2.18 |
| a pair | Sniper Shot 2, aimed | 6.11 | 800 ms | 2.18 |
| a pair | Sniper Shot 3, hip | 6.72 | 800 ms | 2.18 |
| a pair | Sniper Shot 3, aimed | 6.72 | 800 ms | 2.18 |
| a rifle | Sniper Shot 1, hip | 1.44 | 500 ms | 0.90 |
| a rifle | Sniper Shot 1, aimed | 1.42 | 350 ms | 0.63 |
| a rifle | Sniper Shot 2, hip | 1.66 | 500 ms | 0.90 |
| a rifle | Sniper Shot 2, aimed | 1.66 | 350 ms | 0.63 |
| a rifle | Sniper Shot 3, hip | 1.75 | 500 ms | 0.90 |
| a rifle | Sniper Shot 3, aimed | 1.70 | 350 ms | 0.63 |

Each figure for one shot is the average of about 26 shots in each of 8
runs. These averages move by a few percent from one set of seeds to
another.

## Where the soldier aims

The first version of this table showed plain aimed fire at 6 m dealing 6%
to 12% less than plain fire from the hip. The cause is where the shots land
on the body, not the aiming itself.

The bench used to aim at the point the creatures' minds aim at, half the
body's height above its feet. The hit-location rules come from Jedi
Academy. They judge a hit by its direction from the middle of the body's
box, and that box reaches lower than the feet. Seen from the middle of the
box, a hit at that aim point on the front of the body is high and in front,
and the rules count it as an arm. An arm hit deals 85% of the normal damage.
A shot with no spread lands on that point every time. Shots from the hip
spread, so some of them land on the chest (100%) or the head (130%). One
seed of plain fire with a pistol at 6 m against middle Defense showed this:
all 75 aimed hits landed on an arm, while 14 of the 75 hits from the hip
landed on the chest or the head.

The bench now aims at the middle of the body's box. The rules count a hit
there as the waist, which deals 100% of the damage. With that aim, plain
aimed fire at 6 m deals 3% to 4% more than plain fire from the hip, for
every gun and Defense. This is the same effect that makes aiming worth
doing at 30 m, only much smaller.

The aim point matters in play too. A player who aims precisely at the
middle of an enemy's chest, or a mind that aims at its chest point, hits an
arm and deals 85% of the damage. This applies to Sniper Shot most, because
its shots never spread. The hit-location rules match Jedi Academy's, so
this was not changed. The owner may want to look at it.

## What the table shows

- **Sniper Shot is worth using at long range.** At 30 m, keeping [Feat
  attack] held while aiming deals more damage a second than plain aimed
  fire at every Defense. The gain is 1% to 31% against low Defense, 9% to
  50% against middle Defense, and 10% to 64% against high Defense. Higher
  tiers gain more. The largest gain, 64%, is a pair of pistols with Sniper
  Shot 3 against high Defense. The smallest gains are a blaster rifle's.
- **Sniper Shot is about even with plain aimed fire at 6 m.** While aiming,
  it deals 2% to 8% less than plain aimed fire against low Defense, and
  between 2% less and 20% more against middle and high Defense.
- **Sniper Shot from the hip is worse than Sniper Shot while aiming, except
  with a blaster rifle at 30 m.** The plain shots that the soldier fires
  while the stamina is empty spread much more from the hip. A rifle at 30 m
  from the hip deals 5% to 12% more than while aiming. A likely reason is
  that its 500 ms steadying from the hip is longer than the 450 ms after
  which a rifle's spread and kick settle, so the plain shots after each
  Sniper Shot start accurate again. This was not studied further.
- **Against low Defense, Sniper Shot's bonus to hit does nothing.** A plain
  shot already hits 95% of the time, which is the most any attack can. Only
  the wider critical threat range counts there. At 30 m the shot's lack of
  spread still counts.
- **Most of the shots in a held minute are plain shots.** Stamina comes back
  at 6 points a second, so every feat line fires about 26 feat attacks a
  minute, and every feat line uses 7.8 stamina a second. Sniper Shots now
  leave as fast as the gun fires, so the stamina runs out quickly. Plain
  shots are 59% to 62% of a pistol's shots, 73% of a pair's and 81% to 83%
  of a rifle's. For Power Blast they are 38% of a pistol's shots, 47% of a
  pair's and 73% of a rifle's.
- **Power Blast deals far more than Sniper Shot, in damage a second and for
  its stamina.** Power Blast deals 1.4 to 4.1 times the damage a second of
  Sniper Shot of the same tier, fired the same way. For each point of
  stamina, a Power Blast alone deals 2.3 to 12 times what a Sniper Shot
  alone deals. A Sniper Shot alone deals 0.18 to 0.23 hit points for each
  point of stamina with one pistol, 0.28 to 0.46 with a pair and 0.08 to
  0.10 with a rifle. A Power Blast alone deals 0.56 to 1.47 with one
  pistol, 0.68 to 2.23 with a pair and 0.42 to 1.11 with a rifle.

The reason for the gap is how the game sizes a shot's damage. A shot's
damage grows with the time its attack takes. A Power Blast's charge counts
as part of that time, so a fully charged Power Blast from a pistol deals the
damage of 1.8 seconds of attack, against 0.8 seconds for a plain shot. It
also adds 5, 8 or 10 points of damage. A Sniper Shot's steadying does not
count, so a Sniper Shot deals the damage of one plain shot. It only hits
more often and scores more critical hits. A blaster rifle's shot takes the
least time of the three guns, so each of its Sniper Shots deals the least
damage for the same 18 points of stamina.

## Verdict

Sniper Shot is now worth using against an enemy at long range, while
aiming. There it deals 1% to 64% more damage a second than plain aimed
fire, more against harder targets, and up close it is about even with plain
aimed fire. It never comes near Power Blast, which deals 1.4 to 4.1 times
its damage a second and 2.3 to 12 times its damage for each point of
stamina.

If the owner wants Sniper Shot to compete with Power Blast, one option is
to count the steadying time as part of the shot's attack time, the way
Power Blast counts its charge. A steadied shot would then deal more damage
for its stamina. This was not built.

## The full table

Damage is in hit points. Stamina is in points. The range is the distance
between the soldier and the dummy.

| Gun | Way of firing | Defense | Range | Damage a second | Stamina a second | Plain shots | Damage per point, feat alone | Gain over plain aimed |
|---|---|---|---|---|---|---|---|---|
| one pistol | plain, hip | low | 6 m | 4.34 | 0.00 |  |  | -3% |
| one pistol | plain, hip | low | 30 m | 1.18 | 0.00 |  |  | -61% |
| one pistol | plain, hip | middle | 6 m | 3.39 | 0.00 |  |  | -3% |
| one pistol | plain, hip | middle | 30 m | 0.91 | 0.00 |  |  | -61% |
| one pistol | plain, hip | high | 6 m | 2.91 | 0.00 |  |  | -3% |
| one pistol | plain, hip | high | 30 m | 0.79 | 0.00 |  |  | -61% |
| one pistol | plain, aimed | low | 6 m | 4.48 | 0.00 |  |  | +0% |
| one pistol | plain, aimed | low | 30 m | 3.01 | 0.00 |  |  | +0% |
| one pistol | plain, aimed | middle | 6 m | 3.50 | 0.00 |  |  | +0% |
| one pistol | plain, aimed | middle | 30 m | 2.35 | 0.00 |  |  | +0% |
| one pistol | plain, aimed | high | 6 m | 3.02 | 0.00 |  |  | +0% |
| one pistol | plain, aimed | high | 30 m | 2.01 | 0.00 |  |  | +0% |
| one pistol | Power Blast 1, hip | low | 6 m | 8.21 | 7.78 | 38% | 0.91 | +83% |
| one pistol | Power Blast 1, hip | low | 30 m | 7.85 | 7.78 | 38% | 0.92 | +161% |
| one pistol | Power Blast 1, hip | middle | 6 m | 6.17 | 7.78 | 38% | 0.68 | +76% |
| one pistol | Power Blast 1, hip | middle | 30 m | 5.87 | 7.78 | 38% | 0.69 | +150% |
| one pistol | Power Blast 1, hip | high | 6 m | 5.12 | 7.78 | 38% | 0.56 | +70% |
| one pistol | Power Blast 1, hip | high | 30 m | 4.89 | 7.78 | 38% | 0.57 | +143% |
| one pistol | Power Blast 1, aimed | low | 6 m | 8.21 | 7.78 | 38% | 0.91 | +83% |
| one pistol | Power Blast 1, aimed | low | 30 m | 8.19 | 7.78 | 38% | 0.92 | +172% |
| one pistol | Power Blast 1, aimed | middle | 6 m | 6.17 | 7.78 | 38% | 0.68 | +76% |
| one pistol | Power Blast 1, aimed | middle | 30 m | 6.13 | 7.78 | 38% | 0.69 | +161% |
| one pistol | Power Blast 1, aimed | high | 6 m | 5.12 | 7.78 | 38% | 0.56 | +70% |
| one pistol | Power Blast 1, aimed | high | 30 m | 5.10 | 7.78 | 38% | 0.57 | +154% |
| one pistol | Power Blast 2, hip | low | 6 m | 10.80 | 7.78 | 38% | 1.24 | +141% |
| one pistol | Power Blast 2, hip | low | 30 m | 10.39 | 7.78 | 38% | 1.25 | +245% |
| one pistol | Power Blast 2, hip | middle | 6 m | 8.10 | 7.78 | 38% | 0.93 | +131% |
| one pistol | Power Blast 2, hip | middle | 30 m | 7.76 | 7.78 | 38% | 0.93 | +231% |
| one pistol | Power Blast 2, hip | high | 6 m | 6.72 | 7.78 | 38% | 0.77 | +123% |
| one pistol | Power Blast 2, hip | high | 30 m | 6.45 | 7.78 | 38% | 0.77 | +221% |
| one pistol | Power Blast 2, aimed | low | 6 m | 10.80 | 7.78 | 38% | 1.24 | +141% |
| one pistol | Power Blast 2, aimed | low | 30 m | 10.74 | 7.78 | 38% | 1.25 | +256% |
| one pistol | Power Blast 2, aimed | middle | 6 m | 8.10 | 7.78 | 38% | 0.93 | +131% |
| one pistol | Power Blast 2, aimed | middle | 30 m | 8.04 | 7.78 | 38% | 0.93 | +243% |
| one pistol | Power Blast 2, aimed | high | 6 m | 6.72 | 7.78 | 38% | 0.77 | +123% |
| one pistol | Power Blast 2, aimed | high | 30 m | 6.69 | 7.78 | 38% | 0.77 | +233% |
| one pistol | Power Blast 3, hip | low | 6 m | 12.53 | 7.78 | 38% | 1.46 | +179% |
| one pistol | Power Blast 3, hip | low | 30 m | 12.07 | 7.78 | 38% | 1.47 | +301% |
| one pistol | Power Blast 3, hip | middle | 6 m | 9.39 | 7.78 | 38% | 1.09 | +168% |
| one pistol | Power Blast 3, hip | middle | 30 m | 9.02 | 7.78 | 38% | 1.10 | +285% |
| one pistol | Power Blast 3, hip | high | 6 m | 7.79 | 7.78 | 38% | 0.90 | +158% |
| one pistol | Power Blast 3, hip | high | 30 m | 7.50 | 7.78 | 38% | 0.91 | +273% |
| one pistol | Power Blast 3, aimed | low | 6 m | 12.53 | 7.78 | 38% | 1.46 | +179% |
| one pistol | Power Blast 3, aimed | low | 30 m | 12.45 | 7.78 | 38% | 1.47 | +313% |
| one pistol | Power Blast 3, aimed | middle | 6 m | 9.39 | 7.78 | 38% | 1.09 | +168% |
| one pistol | Power Blast 3, aimed | middle | 30 m | 9.31 | 7.78 | 38% | 1.10 | +297% |
| one pistol | Power Blast 3, aimed | high | 6 m | 7.79 | 7.78 | 38% | 0.90 | +158% |
| one pistol | Power Blast 3, aimed | high | 30 m | 7.74 | 7.78 | 38% | 0.91 | +285% |
| one pistol | Sniper Shot 1, hip | low | 6 m | 3.95 | 7.78 | 59% | 0.21 | -12% |
| one pistol | Sniper Shot 1, hip | low | 30 m | 3.45 | 7.78 | 59% | 0.21 | +14% |
| one pistol | Sniper Shot 1, hip | middle | 6 m | 3.45 | 7.78 | 59% | 0.21 | -2% |
| one pistol | Sniper Shot 1, hip | middle | 30 m | 3.03 | 7.78 | 59% | 0.21 | +29% |
| one pistol | Sniper Shot 1, hip | high | 6 m | 3.04 | 7.78 | 59% | 0.18 | +1% |
| one pistol | Sniper Shot 1, hip | high | 30 m | 2.67 | 7.78 | 59% | 0.18 | +33% |
| one pistol | Sniper Shot 1, aimed | low | 6 m | 4.24 | 7.80 | 62% | 0.21 | -5% |
| one pistol | Sniper Shot 1, aimed | low | 30 m | 3.69 | 7.80 | 62% | 0.21 | +22% |
| one pistol | Sniper Shot 1, aimed | middle | 6 m | 3.66 | 7.80 | 62% | 0.21 | +4% |
| one pistol | Sniper Shot 1, aimed | middle | 30 m | 3.22 | 7.80 | 62% | 0.21 | +37% |
| one pistol | Sniper Shot 1, aimed | high | 6 m | 3.19 | 7.80 | 62% | 0.19 | +6% |
| one pistol | Sniper Shot 1, aimed | high | 30 m | 2.82 | 7.80 | 62% | 0.18 | +41% |
| one pistol | Sniper Shot 2, hip | low | 6 m | 4.08 | 7.78 | 59% | 0.22 | -9% |
| one pistol | Sniper Shot 2, hip | low | 30 m | 3.58 | 7.78 | 59% | 0.22 | +19% |
| one pistol | Sniper Shot 2, hip | middle | 6 m | 3.57 | 7.78 | 59% | 0.22 | +2% |
| one pistol | Sniper Shot 2, hip | middle | 30 m | 3.14 | 7.78 | 59% | 0.22 | +34% |
| one pistol | Sniper Shot 2, hip | high | 6 m | 3.32 | 7.78 | 59% | 0.22 | +10% |
| one pistol | Sniper Shot 2, hip | high | 30 m | 2.94 | 7.78 | 59% | 0.22 | +46% |
| one pistol | Sniper Shot 2, aimed | low | 6 m | 4.31 | 7.80 | 62% | 0.21 | -4% |
| one pistol | Sniper Shot 2, aimed | low | 30 m | 3.83 | 7.80 | 62% | 0.22 | +27% |
| one pistol | Sniper Shot 2, aimed | middle | 6 m | 3.71 | 7.80 | 62% | 0.21 | +6% |
| one pistol | Sniper Shot 2, aimed | middle | 30 m | 3.36 | 7.80 | 62% | 0.22 | +43% |
| one pistol | Sniper Shot 2, aimed | high | 6 m | 3.44 | 7.80 | 62% | 0.21 | +14% |
| one pistol | Sniper Shot 2, aimed | high | 30 m | 3.15 | 7.80 | 62% | 0.22 | +57% |
| one pistol | Sniper Shot 3, hip | low | 6 m | 4.22 | 7.78 | 59% | 0.22 | -6% |
| one pistol | Sniper Shot 3, hip | low | 30 m | 3.63 | 7.78 | 59% | 0.22 | +20% |
| one pistol | Sniper Shot 3, hip | middle | 6 m | 3.68 | 7.78 | 59% | 0.22 | +5% |
| one pistol | Sniper Shot 3, hip | middle | 30 m | 3.20 | 7.78 | 59% | 0.22 | +36% |
| one pistol | Sniper Shot 3, hip | high | 6 m | 3.42 | 7.78 | 59% | 0.22 | +13% |
| one pistol | Sniper Shot 3, hip | high | 30 m | 2.99 | 7.78 | 59% | 0.22 | +49% |
| one pistol | Sniper Shot 3, aimed | low | 6 m | 4.40 | 7.80 | 62% | 0.23 | -2% |
| one pistol | Sniper Shot 3, aimed | low | 30 m | 3.87 | 7.80 | 62% | 0.22 | +28% |
| one pistol | Sniper Shot 3, aimed | middle | 6 m | 3.79 | 7.80 | 62% | 0.23 | +8% |
| one pistol | Sniper Shot 3, aimed | middle | 30 m | 3.43 | 7.80 | 62% | 0.22 | +46% |
| one pistol | Sniper Shot 3, aimed | high | 6 m | 3.50 | 7.80 | 62% | 0.23 | +16% |
| one pistol | Sniper Shot 3, aimed | high | 30 m | 3.22 | 7.80 | 62% | 0.22 | +60% |
| a pair | plain, hip | low | 6 m | 6.87 | 0.00 |  |  | -4% |
| a pair | plain, hip | low | 30 m | 1.51 | 0.00 |  |  | -67% |
| a pair | plain, hip | middle | 6 m | 4.99 | 0.00 |  |  | -4% |
| a pair | plain, hip | middle | 30 m | 1.11 | 0.00 |  |  | -67% |
| a pair | plain, hip | high | 6 m | 4.05 | 0.00 |  |  | -4% |
| a pair | plain, hip | high | 30 m | 0.91 | 0.00 |  |  | -67% |
| a pair | plain, aimed | low | 6 m | 7.15 | 0.00 |  |  | +0% |
| a pair | plain, aimed | low | 30 m | 4.55 | 0.00 |  |  | +0% |
| a pair | plain, aimed | middle | 6 m | 5.18 | 0.00 |  |  | +0% |
| a pair | plain, aimed | middle | 30 m | 3.33 | 0.00 |  |  | +0% |
| a pair | plain, aimed | high | 6 m | 4.21 | 0.00 |  |  | +0% |
| a pair | plain, aimed | high | 30 m | 2.72 | 0.00 |  |  | +0% |
| a pair | Power Blast 1, hip | low | 6 m | 11.85 | 7.78 | 47% | 1.38 | +66% |
| a pair | Power Blast 1, hip | low | 30 m | 11.41 | 7.78 | 47% | 1.40 | +151% |
| a pair | Power Blast 1, hip | middle | 6 m | 7.91 | 7.78 | 47% | 0.91 | +53% |
| a pair | Power Blast 1, hip | middle | 30 m | 7.57 | 7.78 | 47% | 0.92 | +127% |
| a pair | Power Blast 1, hip | high | 6 m | 5.96 | 7.78 | 47% | 0.68 | +42% |
| a pair | Power Blast 1, hip | high | 30 m | 5.71 | 7.78 | 47% | 0.69 | +110% |
| a pair | Power Blast 1, aimed | low | 6 m | 11.87 | 7.78 | 47% | 1.38 | +66% |
| a pair | Power Blast 1, aimed | low | 30 m | 11.61 | 7.78 | 47% | 1.40 | +155% |
| a pair | Power Blast 1, aimed | middle | 6 m | 7.93 | 7.78 | 47% | 0.91 | +53% |
| a pair | Power Blast 1, aimed | middle | 30 m | 7.69 | 7.78 | 47% | 0.92 | +131% |
| a pair | Power Blast 1, aimed | high | 6 m | 5.98 | 7.78 | 47% | 0.68 | +42% |
| a pair | Power Blast 1, aimed | high | 30 m | 5.80 | 7.78 | 47% | 0.69 | +113% |
| a pair | Power Blast 2, hip | low | 6 m | 15.67 | 7.78 | 47% | 1.87 | +119% |
| a pair | Power Blast 2, hip | low | 30 m | 15.23 | 7.78 | 47% | 1.90 | +235% |
| a pair | Power Blast 2, hip | middle | 6 m | 10.44 | 7.78 | 47% | 1.23 | +101% |
| a pair | Power Blast 2, hip | middle | 30 m | 10.08 | 7.78 | 47% | 1.25 | +203% |
| a pair | Power Blast 2, hip | high | 6 m | 7.85 | 7.78 | 47% | 0.92 | +87% |
| a pair | Power Blast 2, hip | high | 30 m | 7.60 | 7.78 | 47% | 0.94 | +179% |
| a pair | Power Blast 2, aimed | low | 6 m | 15.69 | 7.78 | 47% | 1.87 | +119% |
| a pair | Power Blast 2, aimed | low | 30 m | 15.44 | 7.78 | 47% | 1.90 | +239% |
| a pair | Power Blast 2, aimed | middle | 6 m | 10.46 | 7.78 | 47% | 1.23 | +102% |
| a pair | Power Blast 2, aimed | middle | 30 m | 10.21 | 7.78 | 47% | 1.25 | +206% |
| a pair | Power Blast 2, aimed | high | 6 m | 7.87 | 7.78 | 47% | 0.92 | +87% |
| a pair | Power Blast 2, aimed | high | 30 m | 7.70 | 7.78 | 47% | 0.94 | +183% |
| a pair | Power Blast 3, hip | low | 6 m | 18.22 | 7.78 | 47% | 2.20 | +155% |
| a pair | Power Blast 3, hip | low | 30 m | 17.78 | 7.78 | 47% | 2.23 | +291% |
| a pair | Power Blast 3, hip | middle | 6 m | 12.13 | 7.78 | 47% | 1.45 | +134% |
| a pair | Power Blast 3, hip | middle | 30 m | 11.76 | 7.78 | 47% | 1.47 | +253% |
| a pair | Power Blast 3, hip | high | 6 m | 9.12 | 7.78 | 47% | 1.08 | +117% |
| a pair | Power Blast 3, hip | high | 30 m | 8.86 | 7.78 | 47% | 1.10 | +225% |
| a pair | Power Blast 3, aimed | low | 6 m | 18.24 | 7.78 | 47% | 2.20 | +155% |
| a pair | Power Blast 3, aimed | low | 30 m | 17.99 | 7.78 | 47% | 2.23 | +296% |
| a pair | Power Blast 3, aimed | middle | 6 m | 12.15 | 7.78 | 47% | 1.45 | +134% |
| a pair | Power Blast 3, aimed | middle | 30 m | 11.90 | 7.78 | 47% | 1.47 | +257% |
| a pair | Power Blast 3, aimed | high | 6 m | 9.13 | 7.78 | 47% | 1.08 | +117% |
| a pair | Power Blast 3, aimed | high | 30 m | 8.96 | 7.78 | 47% | 1.10 | +229% |
| a pair | Sniper Shot 1, hip | low | 6 m | 6.36 | 7.78 | 73% | 0.40 | -11% |
| a pair | Sniper Shot 1, hip | low | 30 m | 3.81 | 7.78 | 73% | 0.40 | -16% |
| a pair | Sniper Shot 1, hip | middle | 6 m | 4.98 | 7.78 | 73% | 0.34 | -4% |
| a pair | Sniper Shot 1, hip | middle | 30 m | 3.11 | 7.78 | 73% | 0.34 | -7% |
| a pair | Sniper Shot 1, hip | high | 6 m | 4.18 | 7.78 | 73% | 0.29 | -1% |
| a pair | Sniper Shot 1, hip | high | 30 m | 2.63 | 7.78 | 73% | 0.29 | -3% |
| a pair | Sniper Shot 1, aimed | low | 6 m | 6.55 | 7.78 | 73% | 0.39 | -8% |
| a pair | Sniper Shot 1, aimed | low | 30 m | 5.34 | 7.78 | 73% | 0.40 | +17% |
| a pair | Sniper Shot 1, aimed | middle | 6 m | 5.09 | 7.78 | 73% | 0.33 | -2% |
| a pair | Sniper Shot 1, aimed | middle | 30 m | 4.20 | 7.78 | 73% | 0.34 | +26% |
| a pair | Sniper Shot 1, aimed | high | 6 m | 4.26 | 7.78 | 73% | 0.28 | +1% |
| a pair | Sniper Shot 1, aimed | high | 30 m | 3.53 | 7.78 | 73% | 0.29 | +30% |
| a pair | Sniper Shot 2, hip | low | 6 m | 6.53 | 7.78 | 73% | 0.43 | -9% |
| a pair | Sniper Shot 2, hip | low | 30 m | 4.20 | 7.78 | 73% | 0.43 | -8% |
| a pair | Sniper Shot 2, hip | middle | 6 m | 5.27 | 7.78 | 73% | 0.39 | +2% |
| a pair | Sniper Shot 2, hip | middle | 30 m | 3.56 | 7.78 | 73% | 0.38 | +7% |
| a pair | Sniper Shot 2, hip | high | 6 m | 4.54 | 7.78 | 73% | 0.34 | +8% |
| a pair | Sniper Shot 2, hip | high | 30 m | 3.12 | 7.78 | 73% | 0.34 | +15% |
| a pair | Sniper Shot 2, aimed | low | 6 m | 6.81 | 7.78 | 73% | 0.42 | -5% |
| a pair | Sniper Shot 2, aimed | low | 30 m | 5.68 | 7.78 | 73% | 0.43 | +25% |
| a pair | Sniper Shot 2, aimed | middle | 6 m | 5.50 | 7.78 | 73% | 0.38 | +6% |
| a pair | Sniper Shot 2, aimed | middle | 30 m | 4.64 | 7.78 | 73% | 0.38 | +39% |
| a pair | Sniper Shot 2, aimed | high | 6 m | 4.69 | 7.78 | 73% | 0.34 | +11% |
| a pair | Sniper Shot 2, aimed | high | 30 m | 4.03 | 7.78 | 73% | 0.34 | +48% |
| a pair | Sniper Shot 3, hip | low | 6 m | 6.86 | 7.78 | 73% | 0.46 | -4% |
| a pair | Sniper Shot 3, hip | low | 30 m | 4.38 | 7.78 | 73% | 0.44 | -4% |
| a pair | Sniper Shot 3, hip | middle | 6 m | 5.65 | 7.78 | 73% | 0.41 | +9% |
| a pair | Sniper Shot 3, hip | middle | 30 m | 3.83 | 7.78 | 73% | 0.40 | +15% |
| a pair | Sniper Shot 3, hip | high | 6 m | 5.01 | 7.78 | 73% | 0.39 | +19% |
| a pair | Sniper Shot 3, hip | high | 30 m | 3.50 | 7.78 | 73% | 0.37 | +28% |
| a pair | Sniper Shot 3, aimed | low | 6 m | 6.98 | 7.78 | 73% | 0.46 | -2% |
| a pair | Sniper Shot 3, aimed | low | 30 m | 5.96 | 7.78 | 73% | 0.44 | +31% |
| a pair | Sniper Shot 3, aimed | middle | 6 m | 5.71 | 7.78 | 73% | 0.42 | +10% |
| a pair | Sniper Shot 3, aimed | middle | 30 m | 4.99 | 7.78 | 73% | 0.40 | +50% |
| a pair | Sniper Shot 3, aimed | high | 6 m | 5.04 | 7.78 | 73% | 0.40 | +20% |
| a pair | Sniper Shot 3, aimed | high | 30 m | 4.47 | 7.78 | 73% | 0.37 | +64% |
| a rifle | plain, hip | low | 6 m | 4.28 | 0.00 |  |  | -4% |
| a rifle | plain, hip | low | 30 m | 0.74 | 0.00 |  |  | -72% |
| a rifle | plain, hip | middle | 6 m | 3.33 | 0.00 |  |  | -4% |
| a rifle | plain, hip | middle | 30 m | 0.57 | 0.00 |  |  | -73% |
| a rifle | plain, hip | high | 6 m | 2.87 | 0.00 |  |  | -4% |
| a rifle | plain, hip | high | 30 m | 0.49 | 0.00 |  |  | -73% |
| a rifle | plain, aimed | low | 6 m | 4.47 | 0.00 |  |  | +0% |
| a rifle | plain, aimed | low | 30 m | 2.66 | 0.00 |  |  | +0% |
| a rifle | plain, aimed | middle | 6 m | 3.48 | 0.00 |  |  | +0% |
| a rifle | plain, aimed | middle | 30 m | 2.07 | 0.00 |  |  | +0% |
| a rifle | plain, aimed | high | 6 m | 3.00 | 0.00 |  |  | +0% |
| a rifle | plain, aimed | high | 30 m | 1.79 | 0.00 |  |  | +0% |
| a rifle | Power Blast 1, hip | low | 6 m | 7.24 | 7.78 | 73% | 0.68 | +62% |
| a rifle | Power Blast 1, hip | low | 30 m | 7.12 | 7.78 | 73% | 0.69 | +167% |
| a rifle | Power Blast 1, hip | middle | 6 m | 5.41 | 7.78 | 73% | 0.50 | +55% |
| a rifle | Power Blast 1, hip | middle | 30 m | 5.27 | 7.78 | 73% | 0.51 | +154% |
| a rifle | Power Blast 1, hip | high | 6 m | 4.52 | 7.78 | 73% | 0.42 | +51% |
| a rifle | Power Blast 1, hip | high | 30 m | 4.41 | 7.78 | 73% | 0.42 | +146% |
| a rifle | Power Blast 1, aimed | low | 6 m | 7.25 | 7.78 | 73% | 0.68 | +62% |
| a rifle | Power Blast 1, aimed | low | 30 m | 7.31 | 7.78 | 73% | 0.69 | +174% |
| a rifle | Power Blast 1, aimed | middle | 6 m | 5.41 | 7.78 | 73% | 0.50 | +56% |
| a rifle | Power Blast 1, aimed | middle | 30 m | 5.44 | 7.78 | 73% | 0.51 | +162% |
| a rifle | Power Blast 1, aimed | high | 6 m | 4.52 | 7.78 | 73% | 0.42 | +51% |
| a rifle | Power Blast 1, aimed | high | 30 m | 4.55 | 7.78 | 73% | 0.42 | +154% |
| a rifle | Power Blast 2, hip | low | 6 m | 9.21 | 7.78 | 73% | 0.93 | +106% |
| a rifle | Power Blast 2, hip | low | 30 m | 9.07 | 7.78 | 73% | 0.94 | +240% |
| a rifle | Power Blast 2, hip | middle | 6 m | 6.86 | 7.78 | 73% | 0.69 | +97% |
| a rifle | Power Blast 2, hip | middle | 30 m | 6.70 | 7.78 | 73% | 0.70 | +223% |
| a rifle | Power Blast 2, hip | high | 6 m | 5.71 | 7.78 | 73% | 0.57 | +90% |
| a rifle | Power Blast 2, hip | high | 30 m | 5.60 | 7.78 | 73% | 0.58 | +213% |
| a rifle | Power Blast 2, aimed | low | 6 m | 9.22 | 7.78 | 73% | 0.93 | +106% |
| a rifle | Power Blast 2, aimed | low | 30 m | 9.26 | 7.78 | 73% | 0.94 | +248% |
| a rifle | Power Blast 2, aimed | middle | 6 m | 6.87 | 7.78 | 73% | 0.69 | +97% |
| a rifle | Power Blast 2, aimed | middle | 30 m | 6.87 | 7.78 | 73% | 0.70 | +231% |
| a rifle | Power Blast 2, aimed | high | 6 m | 5.72 | 7.78 | 73% | 0.57 | +91% |
| a rifle | Power Blast 2, aimed | high | 30 m | 5.73 | 7.78 | 73% | 0.58 | +220% |
| a rifle | Power Blast 3, hip | low | 6 m | 10.53 | 7.78 | 73% | 1.10 | +135% |
| a rifle | Power Blast 3, hip | low | 30 m | 10.37 | 7.78 | 73% | 1.11 | +289% |
| a rifle | Power Blast 3, hip | middle | 6 m | 7.82 | 7.78 | 73% | 0.81 | +125% |
| a rifle | Power Blast 3, hip | middle | 30 m | 7.66 | 7.78 | 73% | 0.82 | +269% |
| a rifle | Power Blast 3, hip | high | 6 m | 6.50 | 7.78 | 73% | 0.67 | +117% |
| a rifle | Power Blast 3, hip | high | 30 m | 6.39 | 7.78 | 73% | 0.68 | +257% |
| a rifle | Power Blast 3, aimed | low | 6 m | 10.54 | 7.78 | 73% | 1.10 | +136% |
| a rifle | Power Blast 3, aimed | low | 30 m | 10.56 | 7.78 | 73% | 1.11 | +297% |
| a rifle | Power Blast 3, aimed | middle | 6 m | 7.83 | 7.78 | 73% | 0.81 | +125% |
| a rifle | Power Blast 3, aimed | middle | 30 m | 7.82 | 7.78 | 73% | 0.82 | +277% |
| a rifle | Power Blast 3, aimed | high | 6 m | 6.51 | 7.78 | 73% | 0.67 | +117% |
| a rifle | Power Blast 3, aimed | high | 30 m | 6.52 | 7.78 | 73% | 0.68 | +264% |
| a rifle | Sniper Shot 1, hip | low | 6 m | 3.67 | 7.80 | 81% | 0.09 | -18% |
| a rifle | Sniper Shot 1, hip | low | 30 m | 3.01 | 7.80 | 81% | 0.09 | +13% |
| a rifle | Sniper Shot 1, hip | middle | 6 m | 3.02 | 7.80 | 81% | 0.09 | -13% |
| a rifle | Sniper Shot 1, hip | middle | 30 m | 2.49 | 7.80 | 81% | 0.09 | +20% |
| a rifle | Sniper Shot 1, hip | high | 6 m | 2.63 | 7.80 | 81% | 0.08 | -12% |
| a rifle | Sniper Shot 1, hip | high | 30 m | 2.17 | 7.80 | 81% | 0.08 | +21% |
| a rifle | Sniper Shot 1, aimed | low | 6 m | 4.16 | 7.78 | 83% | 0.09 | -7% |
| a rifle | Sniper Shot 1, aimed | low | 30 m | 2.69 | 7.78 | 83% | 0.09 | +1% |
| a rifle | Sniper Shot 1, aimed | middle | 6 m | 3.40 | 7.78 | 83% | 0.09 | -2% |
| a rifle | Sniper Shot 1, aimed | middle | 30 m | 2.26 | 7.78 | 83% | 0.09 | +9% |
| a rifle | Sniper Shot 1, aimed | high | 6 m | 2.95 | 7.78 | 83% | 0.08 | -2% |
| a rifle | Sniper Shot 1, aimed | high | 30 m | 1.97 | 7.78 | 83% | 0.08 | +10% |
| a rifle | Sniper Shot 2, hip | low | 6 m | 3.69 | 7.80 | 81% | 0.09 | -17% |
| a rifle | Sniper Shot 2, hip | low | 30 m | 3.06 | 7.80 | 81% | 0.09 | +15% |
| a rifle | Sniper Shot 2, hip | middle | 6 m | 3.05 | 7.80 | 81% | 0.09 | -12% |
| a rifle | Sniper Shot 2, hip | middle | 30 m | 2.55 | 7.80 | 81% | 0.09 | +23% |
| a rifle | Sniper Shot 2, hip | high | 6 m | 2.73 | 7.80 | 81% | 0.09 | -9% |
| a rifle | Sniper Shot 2, hip | high | 30 m | 2.30 | 7.80 | 81% | 0.09 | +28% |
| a rifle | Sniper Shot 2, aimed | low | 6 m | 4.19 | 7.78 | 83% | 0.09 | -6% |
| a rifle | Sniper Shot 2, aimed | low | 30 m | 2.73 | 7.78 | 83% | 0.09 | +3% |
| a rifle | Sniper Shot 2, aimed | middle | 6 m | 3.43 | 7.78 | 83% | 0.09 | -1% |
| a rifle | Sniper Shot 2, aimed | middle | 30 m | 2.29 | 7.78 | 83% | 0.09 | +10% |
| a rifle | Sniper Shot 2, aimed | high | 6 m | 3.06 | 7.78 | 83% | 0.09 | +2% |
| a rifle | Sniper Shot 2, aimed | high | 30 m | 2.08 | 7.78 | 83% | 0.09 | +16% |
| a rifle | Sniper Shot 3, hip | low | 6 m | 3.70 | 7.80 | 81% | 0.09 | -17% |
| a rifle | Sniper Shot 3, hip | low | 30 m | 3.06 | 7.80 | 81% | 0.10 | +15% |
| a rifle | Sniper Shot 3, hip | middle | 6 m | 3.06 | 7.80 | 81% | 0.09 | -12% |
| a rifle | Sniper Shot 3, hip | middle | 30 m | 2.54 | 7.80 | 81% | 0.10 | +23% |
| a rifle | Sniper Shot 3, hip | high | 6 m | 2.75 | 7.80 | 81% | 0.09 | -8% |
| a rifle | Sniper Shot 3, hip | high | 30 m | 2.30 | 7.80 | 81% | 0.10 | +28% |
| a rifle | Sniper Shot 3, aimed | low | 6 m | 4.23 | 7.78 | 83% | 0.10 | -5% |
| a rifle | Sniper Shot 3, aimed | low | 30 m | 2.84 | 7.78 | 83% | 0.09 | +7% |
| a rifle | Sniper Shot 3, aimed | middle | 6 m | 3.48 | 7.78 | 83% | 0.10 | -0% |
| a rifle | Sniper Shot 3, aimed | middle | 30 m | 2.40 | 7.78 | 83% | 0.09 | +16% |
| a rifle | Sniper Shot 3, aimed | high | 6 m | 3.10 | 7.78 | 83% | 0.10 | +3% |
| a rifle | Sniper Shot 3, aimed | high | 30 m | 2.19 | 7.78 | 83% | 0.09 | +22% |
