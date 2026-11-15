# Targeted blocks design

Date: 2026-11-15. Status: the owner chose the two main points (aim the crosshair, always show the swing's side). After playing it, the owner replaced the crosshair's aim with the direction of the last camera turn. Every other number and rule below is Claude's call and is marked so.

## Intent

Under the Advanced combat rules a perfect block is a matter of timing alone. The player holds block and presses attack just before the blow arrives. The owner wants an optional mechanic in which the player must also aim. The player turns the camera toward the side that the swing comes from, and the direction of that last turn is the aim. Movie Battles II has a similar idea. The mechanic makes the player read the enemy's swing, which the slower swings of the pacing work now allow.

## Goals

1. A setting named **Targeted blocks**, off by default. It has an effect only under the Advanced rules.
2. With the setting on, a perfect block needs the correct timing and the player's last camera turn toward the correct side.
3. The reticule always shows the side the enemy's swing comes from, from the start of the swing.
4. With the setting off, and under Jedi Academy's rules, nothing changes. The Jedi Academy trace stays identical.

## Non-goals

- No new input data. The game already sends the player's view angles in every command. Recordings, saves and co-op commands keep their format.
- No change to how enemies block. An enemy's guard does not aim.
- No change to blaster bolt deflection.
- No balance fit. The numbers below are starting values. The owner plays the mechanic first.
- No aiming below the attacker. Pointing the camera at the floor is unpleasant, so there are three sides and not four.

## How it works today

- The player holds block. A press of attack while block is held opens the perfect window (`motion/saber.rs`, `duel_block`). The window is 150 ms at the normal difficulty, plus 5 ms for each point of the blocker's Reflex save.
- When a blow meets the blocker, the world asks for the blocker's guard (`world/sabers/duel.rs`, `duel_guard_when`). A player's guard inside the window is `Guard::Perfect`. Outside the window it is `Guard::Active`.
- Every saber move has a start quadrant and an end quadrant (`MoveData.start`, `MoveData.end` in `combat/saber/moves.rs`). The start quadrant is the side of the attacker where the blade is when the swing begins.
- The session's combat rules are fixed when a game starts and are stored in the snapshot (`Snapshot.saber_rules`). In co-op the host's rules apply.
- The blaster crosshair is drawn by `ui::screens::hud::reticle` from a `ReticleView` that `crates/game/src/reticle_view.rs` builds.

## Approaches

Three ways to give the block direction were put to the owner.

1. **Aim the crosshair.** The player turns the camera so the crosshair sits beside the attacker on the swing's side. It needs no new input data and works the same with a gamepad. **The owner picked this one.**
2. **Flick the mouse.** The direction of the last mouse movement is the block direction. It needs a new field in every command, so recordings, saves and co-op need a new version.
3. **Marker with a held camera.** The camera stops while block is held and the mouse moves a marker. The player cannot turn while blocking, which is awkward against several enemies.

After playing approach 1, the owner replaced it with the direction of the player's last camera turn. The turn is read from the view angles that every command already carries, so it needs no new field. The section "The aim" below and the block arcs spec describe it.

## The rule

### The setting

- Name in the options: **Targeted blocks**. Values: Off, On. Default: Off. It is on the Sabers page, below "Combat rules".
- A game keeps the value it started with, like the combat rules. In co-op the host's value applies. (Claude's call. It follows the pattern of the combat rules and of Collision.)
- The value is stored in the snapshot as one flag. An older save reads it as off.
- Under Jedi Academy's rules the flag is ignored.

### The side of a swing

A swing has one of three sides as the blocker sees it: **Left**, **Right** or **Above**. Some swings have no side.

- The start quadrant of the attacker's move decides it. Top is Above. Top-left, left and bottom-left are the attacker's left. Top-right, right and bottom-right are the attacker's right. Bottom has no side.
- The attacker's left or right is turned into the blocker's left or right by geometry, not by assuming the two face each other. The blade starts on the attacker's left or right. If that direction points to the blocker's right (the dot product with the blocker's right vector is above 0.3), the side is Right. If it points to the blocker's left (below -0.3), the side is Left. Between the two the attacker stands side-on, and the swing has no side.
- A move that only a perfect block can stop (katas, spins, jump attacks, back attacks, the list in `duel::unblockable`) has no side.
- A swing with no side needs timing alone, as today.

### The aim

- The aim is the direction of the player's last camera turn. The crosshair's place and the attacker's place play no part in it.
- The turn is read from the change of the view angles that the player's command carries on each step. A gamepad works the same way as a mouse.
- While the body holds block, the sim adds up the changes of yaw and pitch over the last 200 ms. When that sum is longer than 1.5 degrees, its direction becomes the aim. A shorter sum changes nothing, so a still hand keeps the last direction.
- The direction is an angle in degrees clockwise from straight up, from 0 up to 360. A turn up is 0, a turn to the right is 90, a turn down is 180 and a turn to the left is 270.
- From 315 to 45 degrees the aim points above. From 45 to 135 it points right. From 225 to 315 it points left. From 135 to 225 it has no side. No aim has no side. At exactly 45 degrees the aim points right, and at exactly 315 it points left. At 135 and 225 it has no side. (Claude's call on the edges, so that left and right mirror each other.)
- The aim is cleared when the body stops holding block, and when the player starts to lead another body. A body that has just raised its guard has no aim until the camera turns.
- A camera that the game turns by itself (a cutscene, a knockdown) leaves the command's angles as they were, so it is no turn.

### The block

With the setting on, under the Advanced rules, for a body a player's keys act through (the player's character, or the companion the player leads):

- The perfect window is twice as long (200%). The player needs the extra time to aim. (Claude's call.)
- The side is read at the moment the blow meets the blocker, not at the press of attack. The player keeps the aim through the window. (Claude's call. The attacker is known at that moment.) With the aim by the last camera turn, the sim stores the direction of that turn and the turns of the last 200 ms for each body that holds block.
- If the window is open and the swing has no side, the block is perfect.
- If the window is open and the aim's side equals the swing's side, the block is perfect.
- If the window is open and the sides differ, or the aim has no side, the block is an ordinary aimed block (`Guard::Active`). The player loses nothing more than that.
- Everything after that (Stamina, the riposte, the parry animation) follows the existing rules.

### The reticule

The block arcs spec, `2026-11-15-block-arcs-design.md`, replaces this section. In that spec, two arcs around the centre of the screen take the place of the three marks. The aim arc sits at the direction of the last camera turn, and the swing arc shows the threat's swing. The rest of this section describes the three marks as they were first built.

Shown while the body the player controls holds block with a melee weapon, the setting is on and the rules are Advanced.

- Three marks around the crosshair: left, right and above.
- The mark of the crosshair's current side is lit. It is measured against the threat.
- The threat is the hostile fighter whose swing will arrive first: among hostile fighters within 4 meters, in front of the player, whose attack move is under way, the one whose swing began earliest. If none is swinging, the threat is the nearest hostile fighter within 4 meters in front, and no swing side is shown.
- From the start of the threat's swing, the mark of the swing's side is drawn in the warning colour. It changes to the confirm colour while the crosshair's side equals it.
- A swing with no side lights all three marks in the confirm colour.
- The rule itself never uses the threat. Each blow is judged against its own attacker.

### Player text

- Row: "Targeted blocks".
- Help: "A perfect block also needs aim. Turn the camera toward the side that the attack comes from. The sides are left, right and above. An arc around the centre of the screen shows the side. The time for a perfect block is twice as long. Advanced rules only.\n\nA game keeps this setting from its start. In co-op, the host's setting applies."

## Constraints

- Every new rule sits inside a branch on the Advanced rules and the flag. The Jedi Academy trace must be identical to the base branch.
- With the flag off under the Advanced rules, a trace must also be identical to the base branch.
- Rule numbers are in `data/rules/duel.toml`, in a new section `[targeted]`. Another agent tunes other sections of that file at the same time, so this work touches only its own section.
- Movie Battles II is a reference for the idea only. No code and no names are taken from it.
- Player text follows `docs/player-text.md`.

## Left to the builder

- The exact shape, size and colours of the three marks, using the existing reticule's drawing code and the HUD's existing warning and confirm colours.
- How the flag travels from the settings to the world, following the path of the combat rules.
