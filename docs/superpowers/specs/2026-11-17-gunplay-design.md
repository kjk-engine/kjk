# Gunplay

Date: 2026-11-17. Status: waiting for the owner's approval.

## Why

The owner wants shooting a blaster in kjk to feel like a modern shooter. The
model is Star Wars Battlefront. Three things are wrong today:

- A character with a gun stands in one of Jedi Academy's one-frame ready
  poses. The arms go out only during the shot. The character looks stiff.
- A bolt that hits an enemy plays sparks and a sound, and nothing else. The
  player does not see or feel the hit.
- There is no way to aim. Only the Sniper Shot feat has a held aim.

The owner's words for the goal: the player must "FEEL and see an impact".

## Goals

1. A player can hold a button to aim with any pistol, pair of pistols or
   rifle. Aiming raises the gun to eye level, makes shots more accurate, and
   limits the character to walking.
2. A character who fires a gun moves like someone firing a gun.
3. A bolt that hits an enemy leaves a burn, lights the spot, and makes the
   enemy react.
4. A short list of extras (part 4) that the owner picks from.

## Not goals

- No new weapons, no new feats, no new enemies.
- No weapon overheating. KOTOR's items and feats do not account for it.
- No first-person iron sights. First person keeps its own gun view. Aiming
  there narrows the view and tightens the spread only.
- No hand-drawn effects. Every texture, model and sound comes from KOTOR 1,
  KOTOR 2 or Jedi Academy (the owner's rule of 2026-11-01).

## What would make this a failure

- The raise to aim pops, slides the feet, or twists an elbow.
- A burn appears on a face.
- A burn looks like a flat black smudge (the owner turned those off on
  2026-10-24).
- Two machines in a co-op session disagree about whether a character is
  aiming, or a replay or a save stops loading.

## What exists today

- `sim::combat::bloom` holds each shooter's spread and kick. Walking adds
  0.4° of spread, running adds 1.0°, and crouching multiplies spread by 0.6.
- The reticle and hitmarkers read the sim's spread (`game::reticle_view`,
  `hitmarker_view`).
- `cast::Figure::raise` (view only) already holds the last frame of the
  attack clip while a Power Blast charges or a Sniper Shot aims. This is a
  raised gun pose that already exists for pistols and rifles.
- `stance.rs` adds breathing over the held gun poses.
- `combat_fx::hits` plays KOTOR's blaster impact effect and sound at the hit
  location's node.
- `body_marks.rs` draws lightsaber burns that wrap around a body. Bolts
  leave none.
- [Alt attack] with a blaster uses the gun's feat special. Sniper Shot is
  "hold to aim".

## The parts, in the order they are built

Each part is its own pull request. Each one can be played and judged
before the next starts.

### Part 1: hold to aim

**Input.** A new action, [Aim], on the right mouse button and the left
trigger. With a blaster in hand, the feat special moves from [Alt attack]
to a new action, [Feat attack]. The plan picks its default key and
controller button from the ones that are free, and the owner confirms them.
With a lightsaber or no weapon, [Alt attack] keeps its present meaning.

**Rules (sim).** While [Aim] is held with a blaster in hand:

- The character moves at walking speed and cannot run, roll or jump.
- The gun's spread is multiplied by 0.5. The walking penalty still applies.
- The character reaches the aimed state 200 ms after the press, and leaves
  it 150 ms after the release. A shot fired before 200 ms uses the normal
  spread.
- Sniper Shot becomes a shot of its own, used like Power Blast (the
  owner's call, 2026-11-17). Hold [Feat attack] to steady the gun for
  500 ms, then release to fire one shot. A pair of pistols fires both guns
  at once. The shot has no spread at all, keeps Sniper Shot's bonus to hit
  and its wider critical threat range, and deals normal damage. It uses
  the same share of the second bar as today. While steadying, the
  character moves at walking speed, cannot jump, and shows KOTOR's sight
  line. Steadying takes 250 ms when the character already aims. Releasing
  early fires nothing and uses nothing. Sniper Shot's old held aim (spread
  multiplied by 0.25, speed by 0.5) is removed.
- Aiming limits speed the way holding [Walk] does. It stacks with Force
  Speed as walking does.
- Every blaster can aim: pistols, rifles, repeaters, the bowcaster,
  disruptors, ion and sonic weapons. Wrist launchers cannot.
- Only players aim in the sim. Enemies and companions keep their present
  accuracy and speed. Their raised pose is view only (see below).

These numbers go in `data/rules/weapons.toml` as rules the owner can tune.
Aiming is part of the sim's state, so it goes in snapshots, the network
codec, replays and saves, with a bump file.

**View.** While aiming:

- The body blends to the raised gun pose over the same 200 ms, with an
  eased blend. The pose is built per wield: one pistol, two pistols, rifle.
- The third-person camera moves closer to the shoulder and narrows its
  field of view to 75% of normal, over the same 200 ms.
- The reticle tightens because it already reads the sim's spread.
- Enemies and companions who are about to fire use the same raised pose.

### Part 2: firing motion

View only. Nothing in the sim changes.

- Each shot pushes the gun, the hands, the forearms and the shoulders back,
  then lets them settle. The size follows the gun's existing `kick` rule.
  A pistol kicks up at the wrist. A rifle pushes into the shoulder.
- A pair of pistols alternates hands, following which gun the sim fired.
- During sustained fire the chest leans a few degrees toward the target.
- The breathing in `stance.rs` continues underneath.
- The motion is added over whatever pose the body is in, aimed or not, so
  hip fire also looks alive.

### Part 3: impacts on enemies

View only.

- **Burn.** The look the owner approved from the test on 2026-11-17: a
  white-to-red flash for 0.6 s, Jedi Academy's ember texture glowing for
  3.5 s, and Jedi Academy's body burn texture for 8 s with a 2 s fade. The
  burn wraps around the body and moves with it. Size follows the weapon:
  small for a pistol, larger for a rifle, largest for a Power Blast.
- **Never on the head.** A hit on the head leaves no burn, and a burn
  elsewhere is not allowed to reach the head's mesh.
- **Armour.** Burns must draw on armoured bodies such as Sith troopers. In
  the test they did not. Finding and fixing the cause is part of this part.
- **Exact place.** The burn sits where the bolt met the body, not on the
  hit location's node.
- **Light.** A short flash of light at the hit, in the bolt's colour.
- **Smoke.** One of KOTOR's own smoke effects rises from the burn for about
  1 s.
- **Reaction.** The enemy flinches away from the direction the bolt came
  from, on the body part that was hit. A killing shot pushes the ragdoll
  along the bolt's path.
- **Sound.** The existing impact sound stays. No new sound is recorded.

### Part 4: extras

The owner picks which of these to build, after parts 1 to 3 are played:

- Bolts that pass close to the camera play a fly-by sound from the games'
  own files.
- A controller rumbles on each shot and on each hit taken.
- A shot to the head deals bonus damage while the shooter aims.

## Approaches considered

**Where the aim pose comes from.**

1. Hold the last frame of Jedi Academy's attack clip, as `Figure::raise`
   already does, and correct it per wield so the sights meet the eye line.
   This reuses a pose that is already retargeted and already blends into
   the shot. Picked.
2. Use KOTOR's own gun clips. KOTOR has no aiming clip, and its ready
   clips looked like shaking over Jedi Academy's stances.
3. Solve the arms with inverse kinematics from the gun to the eye. This
   gives the most exact result, but the reach solver is already the source
   of most elbow pops in saber fights.

**Where aiming is decided.** Aiming changes spread and speed, so it must
be decided in the sim and sent to every machine. A view-only aim was
rejected for that reason.

**Where firing motion is decided.** It changes no rule, so it is view
only. The sim's skeleton keeps Jedi Academy's pose, as with breathing.

**How the burn is drawn.** The lightsaber burn code already wraps a mark
over a skinned body. Bolt burns reuse it, with their own three layers and
their own timing per burn. A flat quad at the hit point was rejected: it
would float off a moving body.

## Settled after the builder's review

- **Buttons.** [Aim] is a new action of its own. Its defaults are the
  right mouse button and the left trigger, which [Alt attack] also uses.
  The weapon in hand decides which of the two the button means: a blaster
  aims, anything else uses [Alt attack]. Blocking with a blade under the
  Advanced rules is unchanged. A player's saved [Alt attack] binding is
  kept, and [Aim] and [Feat attack] start at their defaults.
- **Other characters' pose.** An enemy or companion shows the raised pose
  from each shot until 600 ms after its last shot. The view reads this
  from the shots it already sees. The sim gets no new state for it.
- **Where a burn goes.** The view traces the bolt it draws against the
  body as posed and puts the burn where they meet. When the trace finds
  nothing, the burn goes on the skin nearest the hit location's node. The
  sim's hit event is not changed, so part 3 stays view only.
- **Flinch.** The new flinch is a small view-only movement on every bolt
  hit, added over the sim's own pain clip when that plays. The sim's pain
  rule is not changed. The ragdoll push that exists today is kept, and
  part 3 checks that it follows the bolt's direction.
- **Which bodies burn.** Every body that takes lightsaber burns: people,
  droids and creatures. Droids keep their sparks as well.
- **How many burns.** A body keeps at most 6 bolt burns. A seventh
  replaces the oldest. Bolt burns never push out lightsaber burns.
- **Poses.** Guns that are not pistols use the rifle pose and the rifle's
  recoil motion, each scaled by its own `kick` rule.
- **Recordings.** In scripted input, `alt` keeps meaning the right button,
  so with a blaster it now aims. A new word, `feat`, presses
  [Feat attack]. Routes and tests that used `alt` for a gun's feat are
  changed to `feat`. Replays and saves follow the repository's bump file
  process: old ones load, and old replays are not required to play back
  the same.

## Calls made by Claude, for the owner to check

- Spread multiplier 0.5 while aiming, 200 ms in, 150 ms out.
- Camera field of view 75% of normal while aiming.
- Sniper Shot steadies for 500 ms, or 250 ms while aiming, and deals
  normal damage.
- Enemies and companions show the raised pose but gain no accuracy.
- At most 6 bolt burns on one body.
- Headshot bonus is an extra, not part of the first three parts.
- Four pull requests, in the order above.

## Sniper Shot's balance

The owner approved the design on 2026-11-17 on the condition that Sniper
Shot is balanced. Part 1 is not done until this is measured:

- Measure damage per second and damage per point of the second bar for a
  plain shot, Power Blast and Sniper Shot, at each feat tier, for a
  pistol, a pair of pistols and a rifle. Use the repository's combat bench
  (`kjk combat`), against targets with low, middle and high defense.
- Sniper Shot must beat a plain shot against high defense and at long
  range, where its accuracy counts. It must not beat Power Blast's damage
  per second against low defense at any tier.
- With aiming held, plain shots must not make Sniper Shot pointless: its
  damage per point of the second bar stays above a plain aimed burst of
  the same length.
- Tune the steady time, the share of the second bar and the threat range
  to reach this. Report the measured table to the owner before the pull
  request is opened.

## Left to the builder

- The default key and button for [Feat attack].
- The exact per-wield corrections to the aim pose.
- Which KOTOR smoke effect to use.
- The cause of burns not drawing on armoured bodies.
- Probes and approved tests for each part, following the repository's
  existing ones for bloom and flinches.

## The test that exists

Branch `feat/bolt-burn-spike`, in its own worktree, not committed. It is a
throwaway. It shows the burn look on cloth and skin. It also found that
`body_marks::gore` dropped any triangle whose corners all lay outside the
depth window, even when the triangle crossed it. The real part 3 keeps that
fix and checks lightsaber burns against it.
