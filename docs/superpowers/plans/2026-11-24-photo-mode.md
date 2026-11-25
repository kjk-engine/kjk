# Photo mode

Plan, 2026-11-24. Design: `docs/superpowers/specs/2026-11-24-photo-mode-design.md`.

Branch `feat/photo-mode`, in a fresh worktree from `origin/main` (at the time of writing `ed14e7c08`). All of the work lives in the view. In co-op, photo mode uses the existing break, so the simulation, the snapshot and the replays do not change, and the branch needs no bump file. The whole feature is one pull request.

## How the code works today

These are the pieces photo mode builds on. Every path is on `origin/main`.

- **The camera's shot.** `play_view::camera_shot` picks the shot of each frame. A conversation's shot (`TalkShot`) comes first, then first person's eyes, then the follow camera or one of the test cameras (`CameraMode::Side`, `Arc` and `Flank`). A shot is a position, three angles (pitch, yaw and roll) and a field of view. Roll is already used by the dialogue cameras, so a tilted photo camera needs no new drawing code.
- **The view keys.** `play_view::view_keys` reads `Action::View` (F1) and the zoom, and switches `View::camera_mode` between `Third` and `First`. It skips its work while a screen is open (`UiOpen`).
- **The fly camera.** The scene viewer has a free camera (`scene.rs`, `FlyCamera` and `fn fly`). It moves by W, A, S, D, Q and E, turns by the mouse with the right button held, and keeps the pitch inside `sim_host::pitch_limit()`. It reads raw keys, not the player's bindings.
- **Actions and bindings.** `settings::ACTIONS` lists every keyboard action with its saved name, its label on the Controls page and its default key. `pad::PAD` lists the default gamepad chords. An action left out of `PAD` has no gamepad default and can still be bound. F1 is First person, F5 and F9 are quick save and quick load, F12 is Report a bug, and Print Screen is Take a screenshot. F2 is free.
- **The player's command.** `sim_host::player_command` builds the player's command from the held actions each tick.
- **Stopping the world.** In the stepping code of `sim_host.rs`, a game played alone waits while a menu is open (`menu_up`, only when `!authority.solo.hosting()`). The world also waits for a movie, a break, the status summary and a loading picture. All of these set `Frozen`, which pauses the sound beds.
- **Slow motion.** `sim_host::Slowed` scales the clock to `rules::duel::slow_motion::SCALE` after a perfect block, and the stepping code reads `slowed.scale(...)` where it counts the ticks due.
- **The HUD.** `ui_view::Ui::hud_hidden` hides the HUD. `Action::HideInterface` toggles it.
- **Near fade and the head.** `camera_tune::fade_near` makes the character see-through when the camera is close. `first_person` hides the character's head in first person.
- **Breaks.** `sim::world::party::breaks` handles `Action::Break` and `Action::Resume`. It refuses a break in combat (`Notice::InCombat`) and lets the caller or seat 0 resume (`may_resume`). `PartyState::paused_by` rides in the snapshot. `party_calls::banner` writes "Paused by player N".
- **Format bumps.** A branch never edits a version number. It adds `bumps/<branch>.toml` that names each format it changes (see `bumps/README.md`).
- **Scripted camera for headless runs.** `camera_tune::Orbit` takes `ui camera orbit YAW PITCH` from a run's script (`scripted`). Photo mode gets a command of the same kind.

## Tasks

### Task 1. The action and its controls

- Add `Action::PhotoMode` to `settings::ACTIONS`, with the saved name `photo_mode`, the label "Photo mode" and the default key F2. Do not add it to `pad::PAD`, so it has no default gamepad chord.
- The controls inside photo mode are fixed and are not shown on the Controls page, except that movement, Jump, Crouch and Screenshot use the player's own bindings. Put the fixed keys and pad buttons in one table in `photo_mode.rs`, so the corner line, the code and the tests read the same list.
- Extend the settings probe that checks every action has a unique saved name and a label.

### Task 2. The photo camera

New module `crates/game/src/photo_mode.rs` with a `PhotoMode` resource and a plugin.

- `PhotoMode` keeps whether photo mode is on, the camera mode it came from, the camera's position, look angles, roll and field of view, the camera's velocity for the ease-out, whether the player's own character is hidden, and the world's speed (Stopped, Quarter, Normal, and Tenth in a replay).
- A pure function `step(camera, input, limits, dt) -> camera` moves the camera. The input is the movement axes, up and down, look deltas, the fast and slow modifiers, tilt and zoom. The function applies the speeds (3, 12 and 0.75 metres a second), the 0.2-second ease-out, the field of view range of 15 to 100 degrees, the look speed scaled by the zoom, the roll limit of 45 degrees and the pitch limit from `sim_host::pitch_limit()`.
- A pure function `clamp(position, area_box, soles) -> position` keeps the camera inside the area's room box grown by 2 metres, and within 40 metres of the character's soles. It slides along the limit: the part of the move that goes past the sphere is removed, and the rest is kept. The area's box comes from `scene::bounds` over the rooms.
- Take the forward and right vectors out of `scene::fly` into one shared function, and use it from both the scene viewer and `step`. Do not otherwise change the scene viewer.
- Entering copies the current shot (the last `camera_shot` result) into `PhotoMode`, so the camera never jumps. Leaving restores the camera mode it came from. The follow camera's orbit and zoom are not touched while photo mode is on, so they are as they were.
- Entering is refused while a screen is open (`UiOpen`), a movie is up or a loading picture is held.

### Task 3. The shot and the input

- In `camera_shot`, the photo camera's shot comes before every other shot when photo mode is on. It gives no aim line (`aim = None`).
- `player_command` returns the empty command while photo mode is on. The tick's time and seat stay as they are, only the buttons and movement are zero.
- Every system that reads play keys skips its work while photo mode is on. These are at least `view_keys`, `party_calls::keys`, the HUD's hide key, the action bar and power wheel, the target cycling, the leader and solo keys and the dialogue reply keys. Put one run condition, `photo_mode::off`, on them instead of a check inside each one.
- Escape leaves photo mode before the pause menu sees it. On a gamepad, B leaves.

### Task 4. Stopping and slowing the world

- In the stepping code of `sim_host.rs`, add `photo_up` next to `menu_up`. It is true when photo mode is on, its speed is Stopped, the world is live, and `!authority.solo.hosting()`. It joins `waiting`, so `Frozen` pauses the sound beds as it does for a menu.
- Where the stepping code reads `slowed.scale(...)`, multiply by the photo speed's scale: 0.25 for Quarter and 1.0 for Normal. Take the smaller of the two scales, so a perfect block's slow motion during a photo does not speed the world up.
- In a replay (`Source::Replay`), Stopped holds the replay's ticks, Tenth scales them by 0.1, and the step control lets exactly one recorded tick through.
- Photo time does nothing when the session is hosting. Task 5 covers that case.

### Task 5. Co-op through the normal break

- When the session is hosting or joined, entering photo mode pushes `Action::Break` onto `party_calls::Asked` if no break holds (`snapshot.paused_by` is `None`). `PhotoMode` notes that it called the break.
- Leaving photo mode pushes `Action::Resume` only if photo mode called the break and the break still holds by this seat. A break called by someone else, or by this player with the key before entering, is left alone.
- During combat the simulation refuses the break and sends `Notice::InCombat`, which the game already shows as a line. Photo mode stays on, and the world keeps running.
- The Photo time control does nothing in co-op. The corner line says "Paused for everyone" while a break holds, and "The game keeps running" when none does.
- `party_calls::keys` is skipped while photo mode is on (task 3), so the Z key cannot resume the break by accident.

### Task 6. What the screen hides and shows

- Hide the HUD through a second flag next to `Ui::hud_hidden`, so the player's own Hide interface choice is kept. The HUD is hidden when either flag is set.
- Hide the crosshair (`reticle_view`), the target ring (`target_view`), the block marker (`block_reticle_view`), the hit markers (`hitmarker_view`), floating text and damage numbers (`floaty_view`), conversation lines and replies (`dialogue_view`), and cutscene subtitles.
- Draw the head in first person while photo mode is on, and turn off `fade_near`.
- The H control hides and shows the player's own character, and its held weapons and effects.
- The corner line shows "Photo mode", the speed, and the keys for leaving and for a screenshot. It fades out after 3 seconds without input. The screenshot code (`report`) hides it for the frame it captures.

### Task 7. Headless commands and checks

- A run's script gets `ui photo on X Y Z PITCH YAW [FOV]`, `ui photo off` and `ui photo speed stopped|quarter|normal|tenth`, read the same way `ui camera orbit` is.
- Approved test `photo_camera`: a parameterised table of cases for `step` and `clamp` (each speed, the ease-out, zoom limits, roll limit, pitch limit, the 40-metre limit with a slide, the area box).
- Probe `photo_mode_alone`: a game played alone, photo mode on for 5 seconds of wall time, then off. The world's tick and every body's position are unchanged, and the shot after leaving equals the shot before entering. A second case runs at Quarter and checks that the ticks due are a quarter of normal.
- Probe `photo_mode_input`: while photo mode is on, Z, Q and the attack button change nothing in the world.
- Probe `photo_mode_coop`: two seats. Seat 1 enters photo mode, and a break by seat 1 holds. Seat 1 leaves, and the break ends. When seat 0 called a break first, seat 1 entering and leaving photo mode leaves that break alone. In combat, entering calls no break, and the world keeps running.

### Task 8. Footage and the pull request

- Film the photo camera in the Endar Spire (a fight frozen mid-swing) and on Peragus (a slow pan at Quarter), and two seats in co-op paused in the same moment, with `--headless --mute` and a scratch `KJK_SETTINGS`. Send the clips to the owner.
- Run `cargo fmt --all` and a release build. CI runs clippy, the tests and the chains.
- Open the pull request "Photo mode" with a description written for players.
