# Photo mode

Date: 2026-11-24. Status: design approved in its main points by the owner on 2026-11-24. The plan is `docs/superpowers/plans/2026-11-24-photo-mode.md`.

The owner's answers on 2026-11-24:

- Camera mode is a photo mode.
- The camera stays within 40 metres of the player's character.
- The action has no default button on a gamepad. A gamepad player binds one if they want it.
- In co-op, photo mode pauses the world for every player.

## What photo mode is

The player presses one key, and the camera leaves the character. The player can then fly the camera around the area, turn it, tilt it and zoom it, and take screenshots. The interface is hidden while photo mode is on. When the player presses the key again, the camera goes back to exactly where it was, and the game goes on as before.

Photo mode is for three things:

1. Players who want to take pictures of the game, for Discord, Nexus pages and their own mods.
2. The trailer. The trailer notes ask for engine-real footage, filmed with a free camera, slow motion and the interface off.
3. Bug reports. A player can fly the camera to show a floating prop, a hole in a wall or a creature stuck in the floor.

Photo mode does not change the world. It does not move the character, open doors, or let the player act from the camera's position. It adds no new visual effects, filters, frames or depth of field. The picture is the game's normal picture, seen from somewhere else.

## Entering and leaving

The player turns photo mode on with a new action, "Photo mode". Its default key is F2, next to F1 for first person. The action has no default gamepad button. A gamepad player can bind it to a button or a chord on the Controls page, like any other action.

The player can turn photo mode on in free play, in combat, in a conversation and in a cutscene. The player cannot turn it on while a menu, the map, a loading screen or a movie is up, because those screens have no world to film.

The player leaves photo mode by pressing the Photo mode key again, or by pressing Escape, or by pressing B on a gamepad. Escape leaves photo mode and does not open the pause menu. A second Escape opens the pause menu as usual.

When photo mode ends, the camera returns to the shot it had when photo mode began. In third person that is the follow camera, with its orbit and zoom as they were. In first person it is the character's eyes. In a conversation it is the conversation's current shot.

While photo mode is on, the keys of normal play do nothing. For example, Z does not call a break, Q does not turn on solo mode, and the mouse buttons do not attack. Only the photo mode controls below work.

## Time

### Playing alone

When the game is played alone, the world stops the moment photo mode turns on. It stops the same way it stops while a menu is open. The simulation takes no steps, and the sound beds pause where they were. Creatures hold their poses, bolts hang in the air, and a lightsaber swing stays where it was. This works in combat too, as KOTOR's own pause does.

The player can change the world's speed with the "Photo time" control (T on the keyboard, A on a gamepad). Each press moves to the next speed in this list, and then back to the start:

1. Stopped (the starting speed).
2. 25% of normal speed.
3. Normal speed.

At 25% and at normal speed the world runs on, and the characters act as they would without photo mode. The player's own character gets no orders from the player. It stands where it was, the way it does when the player lets go of every key. Its companions and its enemies are run by their own AI as usual, so a fight goes on and the player's character can be hit.

The slow speed uses the same clock scale that a perfect block's slow motion already uses. Slow motion changes the game's clock only, so the world plays the same at 25% as at normal speed, only slower.

### Co-op

In co-op, photo mode pauses the world for every player by calling the game's normal break. When a player turns photo mode on, the game presses "Take a break" for them. When the player leaves photo mode, the game resumes that break. The banner at the top of the screen says "Paused by player 2", as it does for any break. The simulation does not know about photo mode at all.

The break follows its normal rules:

- If a break already holds when a player turns photo mode on, no new break is called. The world is already paused, and the player flies the camera in that paused moment. This is how a group can take a picture of the whole party: one player calls a break, and then everyone turns photo mode on.
- Leaving photo mode resumes the break only if photo mode called it. A break that the player called with the key before entering photo mode, or that another player called, stays.
- The host can end any break with the "Take a break" key. If the host ends a photo break, the player in photo mode keeps the camera, and the world runs on at normal speed around them.
- When the player who called the photo break leaves photo mode, the break ends, even if other players are still in photo mode. They keep their cameras, and the world runs on around them.
- Breaks cannot be called during combat. A player who turns photo mode on during combat gets the camera, but the world does not stop. The game shows its usual line, "You cannot do that with enemies nearby." The player's character stands still and gets no orders.

The Photo time control does nothing in co-op. Slow motion for one player would change the game for everyone, so the world only runs at normal speed or stands still in a break.

### Replays

Photo mode works when the game plays back a recorded replay (`kjk play --replay`). In a replay the Photo time control also offers 10% of normal speed, and the player can step the replay forward one tick at a time with the "Photo step" control (the period key). This is the main tool for filming the trailer.

## Moving the camera

The camera starts at the exact position, direction and field of view of the shot the player was seeing. It never jumps when photo mode begins.

The camera moves with the player's own movement controls, so a player who moved the movement to other keys finds them working here too. The camera goes up with the Jump control and down with the Crouch control.

| What it does | Keyboard and mouse | Gamepad |
|---|---|---|
| Move forward, back, left and right | The movement keys (W, S, A and D unless rebound) | Left stick |
| Move straight up and down | The Jump key and the Crouch key | Right trigger and left trigger |
| Look around | Move the mouse (no button needs to be held) | Right stick |
| Move 4 times as fast | Hold Shift | Click the left stick |
| Move at a quarter of the speed | Hold Ctrl | Hold the left bumper |
| Tilt the camera to the left and right | Z and C | D-pad left and right |
| Zoom in and out | Mouse wheel | D-pad up and down |
| Put the tilt and zoom back to normal | R | Y |
| Hide or show the player's own character | H | X |
| Change the world's speed | T | A |
| Step a replay forward one tick | Period | Right bumper |
| Take a screenshot | The Screenshot key (Print Screen unless rebound) | Start |
| Leave photo mode | The Photo mode key or Escape | B |

The camera moves at 3 metres a second. With Shift it moves at 12 metres a second. With Ctrl it moves at 0.75 metres a second. The camera eases to a stop over 0.2 seconds when the player lets go, so a slow pan looks smooth in a video.

Zoom changes the field of view. The narrowest view is 15 degrees from top to bottom, and the widest is 100 degrees. The mouse look slows down in step with the zoom, so a zoomed-in camera does not swing wildly.

The tilt (roll) goes up to 45 degrees to either side.

The look direction stops just short of straight up and straight down, with the same limit the follow camera uses.

### Where the camera may go

The camera flies through walls and floors. It does not collide with anything.

The camera cannot leave the area. Its position is kept inside the box around all of the area's rooms, made 2 metres larger on every side. Outside that box there is nothing to draw but the fog colour.

The camera also stays within 40 metres of the player's character. Without this limit a player could fly ahead and look into rooms the story has not opened yet. When the camera reaches the limit, it slides along it instead of stopping dead. Rooms are drawn by the area's usual visibility list, from the room the camera is in.

## What changes on the screen

While photo mode is on:

- The HUD is hidden. This is the same hiding the Hide interface key already does. The player's own Hide interface choice is kept and comes back when photo mode ends.
- The crosshair, the target ring, the block marker and the hit markers are hidden.
- Floating text over creatures and the damage numbers are hidden.
- Conversation lines and their replies are hidden. When photo mode ends, the conversation shows the line it is on.
- A cutscene's subtitles are hidden.
- The player's character is drawn whole. In first person the character's head is hidden today, and photo mode draws it again. The fade that makes the character see-through when the camera is close is off.
- A small line of text in a corner says "Photo mode", the world's speed, and the keys for leaving and for taking a screenshot. The line fades away after 3 seconds without input. It is hidden while a screenshot is taken.

A screenshot taken in photo mode is saved to the same screenshots folder the Screenshot key already uses. It is taken at the window's size, with nothing drawn over it.

## Conversations and cutscenes

A conversation goes on choosing its shots while photo mode is on, but the screen shows the free camera instead. The conversation's own timing does not change. When the world is stopped, the line on screen does not move on until the world runs again.

The player cannot pick a reply while photo mode is on. The reply waits until photo mode ends.

## How this fits the code

This section is for the people who build it. The plan has the details.

- Photo mode lives in the view, in a new module `crates/game/src/photo_mode.rs`. It is never saved by a player alone, and it changes nothing in the snapshot of a game played alone.
- `camera_shot` in `play_view.rs` returns the photo camera's shot before every other shot, including a conversation's shot.
- A game played alone stops through a new condition next to `menu_up` in the stepping code of `sim_host.rs`. It sets the same `Frozen` flag, so the sound beds pause as they do for a menu. The 25% speed is applied where the clock already takes `Slowed::scale`.
- The player's own command is the empty command while photo mode is on.
- In co-op, photo mode sends the existing `Action::Break` and `Action::Resume`. The simulation, the snapshot and the replays do not change.

## Checks

- An approved test checks the photo camera's movement, its limits and its speeds, as a table of cases.
- A probe turns photo mode on and off in a game played alone, and checks that the world did not move and that the camera returned to the same shot.
- A probe checks the co-op break. Entering photo mode calls a break, leaving it resumes the break, a break called by someone else is left alone, and in combat no break is called.
- Footage of the camera flying through the Endar Spire and Peragus is sent to the owner before the pull request is opened.
