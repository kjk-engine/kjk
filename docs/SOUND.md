# Sound

The `audio` crate mixes the view's sounds with kira. The sim hears nothing:
views ask for sounds from the presented snapshots (`sound::Cues`, loops
`sound::Loops`, beds `sound::Beds`) at a moment of the view's clock (ms);
the mixer starts each then. What plays when: the fight's in `COMBAT.md`,
"Sounds"; the world's below ("What plays when"); movies at the end.

- **Files** through `vfs`, decoded once at load; nothing is copied out.
  Names are JA's code's; one missing as a WAV is read as the MP3 of that
  name, as JA does (most of JA's sounds are MP3s). KOTOR's are `k1/<resref>`
  (`sound::kotor_file`: a WAV of the install's layers, else of any stream
  folder, as the games look them up; `audio::Clip::kotor`: bytes before a
  `RIFF` passed over (the ambience's whole MP3 frame, `FF F3 60 C4`), an
  MP3 behind an empty `data` chunk taken out, IMA ADPCM decoded block by
  block by `fmt_odyssey::wav::Ima`).
- **Beds are streamed** (`audio::Stream`): music, ambience and a movie's
  sound are kept as their coded bytes (MP3, IMA ADPCM, PCM) and decoded as
  they play, through the same decoder a clip is decoded whole with (an
  area's tracks decoded whole held 70 MB on `end_m01aa`, coded 5 MB). A
  window's bed is decoded ahead on a thread of its own (a second and more;
  behind, it waits silent). The capture decodes in the render call, on the
  game's clock: its `audio.wav` has every frame, the same ones a bed
  decoded whole played.
- **Placing** (JA's mixer; Quake units). The listener is the camera, its
  right, the player's character. The player's own sounds are not placed:
  both ears 0.75. Others: full volume to 256 units, then 0.0008 less a unit;
  each ear `0.5 ± 0.5 × side` (`side`: the cosine of the camera's right and
  the way to the sound) times that, in 1/255ths. Voices: full three times
  as far; `Falloff::Far` eight. Placed again every frame; one following its
  character moves with it; one gone silent stops (not a voice).
- **Brought and hushed.** A view may bring clips it read itself
  (`sound::Added`, a conversation's voiced lines) into the bank before the
  frame's cues, and silence a character's slot at once (`sound::Hush`,
  `Mixer::stop`: what plays there and what waits to).
- **Loops** are held afresh every frame; one name's holders play as one
  sound, each ear summed (at most full), in step with the clock; never
  unplaced. Each holder has its falloff and gain (a sound object's range).
- **Beds** (music, ambience, a movie's sound) are held every frame too,
  heard alike in both ears at a gain: one begun fades in, one let go fades
  out, a new gain is glided to, each over its fade; one played to its end
  stays silent while held (`Sounding` says which beds sound).
- **KOTOR's range** (`Falloff::Range`, a sound object's): full within its
  near distance, falling straight to nothing at its far one (xoreos's
  linear model); placed, or in both ears alike.
- **Slots.** A character's sound in a slot (`Channel::Weapon`, …) cuts off
  its last there; `Auto` adds. 32 at once; past that the oldest gives way,
  never the player's own for another's. The mix at 0.5 (`s_volume`), then
  a fast limiter (−3 dB, 20:1): a fight over its music no longer clips
  (Q-0090).
- **Volumes** (`audio::Bus`): four tracks into the mix, the options'
  sound sliders (`sound.music`, `sound.voice`, `sound.effects`,
  `sound.movie`; `game::sound::Volumes`), each at the slider squared
  (half way is −12 dB; full, the default, as mixed). The music, battle
  music, stingers and the menu's and credits' music are Music; a voice
  slot's sound (lines, barks, cries) Voice; a movie's sound Movie; the
  rest (the ambience, loops, every other cue) Effects.
- **Keyframed** (`animevents.cfg`, `asset_ja::events`). Each 60 Hz view
  step, a figure's legs (`model_root`) and torso (`lower_lumbar`) whole
  frames are compared with the last step's: a keyframe plays if landed on,
  or, the part having moved more than a frame within one animation, gone
  over (backward animations backward, round a loop) within three frames.

## Window and headless

A window plays to the sound device (none: no sound, a warning); its menu
has a mixer of its own on real time, playing the beds the menu's views
hold (its music, movies), with nothing in its bank until a session brings
its sounds.
A headless menu is silent. A headless run captures the mix on the game's
clock to `audio.wav` in its run
folder (44.1 kHz stereo, from game time 0), so `anim` holds the sound too.
`record name from to [fps]` steps held time (solo); `clip name seconds
[fps]` screenshots the running game (a host or a joined player), each frame
stamped with the game's time shown; both end in `name.mp4` with the sound.
(`film` keeps raw frames on wall time, without sound: `RUN.md`.)

```bash
kjk play --stage end_m01aa --arrive wp_end_weld01_02 --class jedi \
  --hostile end_sith2 --hostile end_sith3 --ally end_trask --headless \
  --moves '200ms yaw 186; 300ms attack yaw 186; 1100ms w yaw 186; 300ms yaw 215; 500ms attack w yaw 215; 6s attack yaw 215' \
  --cmds 'anim 0.05; screenshot warm; record fight 0.1 6 30; dump; quit'
kjk host --stage duel --class jedi --headless --cmds 'wait 9s; clip duel 9; quit'
```

Dumps' `sound`: output, clock, voices and loops, sounds asked for by name,
the last 256 started with their moments (`cued`, ms),
started, started out of earshot (`unheard`), files not read, the listener;
`ambience`'s `streams`: the area's beds kept, by file, with their bytes.

## What scripts set (the ambience hook)

The scripts' sound routines change sim state every view reads from the
presented snapshot (`sim::snapshot::sound`):
- a sound object's `ObjectState::state` (`sound::AS_PLACED`: as its UTS
  says; `PLAYING`, `STOPPED`: a script's `SoundObjectPlay`, `…Stop`,
  `…FadeAndStop`) and `volume` (1 + 0..=127 from `SoundObjectSetVolume`;
  0 the UTS's), and `pitch` (`SoundObjectSetFixedVariance`: 1 + hundredths
  of its speed; 0 the UTS's variation); a loop plays at its first holder's
  speed;
- `Snapshot::area_sound` (`AreaSound`): the music, battle music and
  ambience, each started or stopped by `MusicBackgroundPlay`/`Stop`,
  `MusicBattlePlay`/`Stop`, `AmbientSoundPlay`/`Stop`, its day's and night's
  track (`…ChangeDay`, `…ChangeNight`, `MusicBattleChange`; `None`: the
  ARE's) and the ambience's volume (`AmbientSoundSetDayVolume`, `…Night…`).
  Reset on each install.


## What plays when (I20)

The world's own sound (`game::ambience`), from the presented snapshot
and the area's files (`asset_odyssey::sound`: the GIT's `AreaProperties`,
the ARE's `IsNight`, each sound object's UTS, each door's and placeable's
sounds, each creature's footsteps). The area's sound and every file it
plays (objects, footsteps, music day and night, battle music and its
stingers, ambience) are read (the clips decoded, the beds kept coded) with
its scene as the module
loads (`ambience::Preread`, beside the scene on the installer's thread:
about 130 ms for `end_m01aa`), so the music, the ambience and the objects
sound from the world's first frame, as KOTOR's do; a swap whose scene does
not carry them (an install with nothing to draw) reads them on a thread of
the view's:

- **Music**: `ambientmusic.2da` row `MusicDay` (`MusicNight` in an area
  that is night), column `resource`, from a stream folder; played through,
  then again `MusicDelay` ms after it ends (5 s on the Endar Spire). Row 0
  is none.
- **Battle music** (`game::battle_music`; neither reference plays it, the
  rule is K1's executable's, 2026-10-24): while the area's battle flag is
  up, the area's `MusicBattle` row loops, faded in over 1.5 s; the music
  fades out. The flag goes up with a script's `MusicBattlePlay`, or when a
  seat's body is stirred (`excitedduration.2da`, 10 s: an attack or power
  landing on it or by it, or on or by one of its party within
  `PercepRngDefault`, 20 m, in a fight with one hostile to it) while a
  living creature hostile to it stands within 30 m; it comes down with
  `MusicBattleStop`, or when the last stirred seat's 10 s run out (which
  ends a script's battle music too). Shots and swings at the air, a duel
  with a friend, others' fights in a cutscene play none. As the flag drops
  one of the row's stingers (`stinger1..3`, e.g. `mus_sbat_sithbs`) plays
  through, then the music begins again (unless a script stopped it, as
  the games' `k_act_cmusic_on` does).
- **Ambience**: `ambientsound.2da` row `AmbientSndDay` (or night), looped,
  at its volume (`AmbientSndDayVol`, `AmbientSndNitVol`, 0 to 127) by
  xoreos's curve `1.25·(1 − 5^(−v/127))`.
- **Scripts** (`snapshot::AreaSound`) win over the area: a track stopped
  or started, its row changed (`MusicBackgroundChangeDay`, …), the
  ambience's volume.
- **Sound objects** (the GIT's sounds, their UTS): on as placed (`Active`)
  or as scripts switch them (`SoundObjectPlay`, `…Stop`, `…SetVolume`).
  One with a single `Looping` sound holds it where it stands, seamlessly;
  the rest play one sound at a time, the next `Interval` ± `IntervalVrtn`
  ms after one ends, picked at random (`Random`) or in order, and stop
  after the last unless `Continuous` or `Looping`; the first within its
  first interval, so objects placed together do not all sound at once (a
  sound not read yet waits its turn). Gain `Volume`/127 ± `VolumeVrtn`,
  speed 1 ± `PitchVariation`, `Elevation` metres up, a random spot within
  `RandomRangeX/Y` (`RandomPosition`); full within `MinDistance`, nothing
  past `MaxDistance`, placed if `Positional`, else in both ears alike.
  `Hours` and `Times` are not read (no time of day yet). A script's
  `SoundObjectPlay` starts one at once from its first sound, each time it
  is called (`ObjectState::plays`), not within its first interval. In a minigame one
  not `Positional` is heard alike everywhere, and the ARE's minigame
  `Music` is the music (`MINIGAMES.md`, "Heard").
- **Doors and containers**: `placeableobjsnds.2da` by the appearance's
  `SoundAppType` (a door's `genericdoors.2da` row by its generic type,
  `doortypes.2da` by a non-zero appearance; Q-0091): `opened` when a door's
  state leaves 0 or a container's first user opens it, `closed` back;
  `locked` when one fails to open (K1's feedback for a locked object: the
  engine's `sim::notice::LOCKED_SOUND`, a `Sound` told to every player,
  played as the object's own). `used` has no signal in the snapshot yet.
- **Scripts' sounds** (`PlaySound`): told to every player as a notice
  (`sim::notice::Sound`, the network's message 12), each played once as
  soon as its file is read (dropped if not read within 2 s): at the object
  that played it and moving with it (a ronto's bellow, a tach's cry),
  falling off as most sounds do; the module's or area's heard alike
  everywhere. `dump`'s `ambience.scripted` counts them.
- **Sound-set lines** the engine has a character say (Security: begun
  "No problem.", failed "Didn't work.", done "Got it." for K1's male
  player; slots 22, 23, 24, `fmt_odyssey::ssf::slots`): a `Sound` named
  `*ssf<slot>` (`sim::notice::Sound::line`), told to every player, played
  as the scripts' sounds are, in the speaker's voice slot. The view reads
  the line when said: a creature's sound set is its template's
  `SoundSetFile`, a player's its persona's (`soundset.2da`
  `Player_<Female|Male>_W` by the portrait's sex: K1's `_B` sets have no
  recordings in the install; TSL's player has none); the SSF's strref
  there, the talk table's sound for it. Voice only, no subtitle or bubble.
  The fight's grunts (`game::grunts`, `COMBAT.md` "Sounds") are such
  lines the view says itself, its own choice as the executable's client's.
- **A conversation's voice** (`dialogue_view`): its line's VO in the
  speaker's voice slot (a line skipped stops it), unplaced
  (`Falloff::Unplaced`: alike in both ears, wherever the speaker stands,
  as reone and xoreos play it), so a speaker far from the shot is heard
  (Taris's duel announcer, 91 m from the ring's cameras). Barks keep
  their place.
- **Barks' voices** (`game::bark_view`; the bubble: `UI.md`,
  "Barks"): each bark's once, from when it was barked: a one-liner's as
  its line would play in a conversation, a `BarkString`'s its talk-table
  entry's sound (K1's name theirs: the turret's "Incoming fighters!" is
  `nm12aacuts01001_`). From a drawn speaker, following it, in its voice's
  slot; from nobody, or in a minigame, heard alike everywhere. Barks alike
  at once are voiced once (the turret's skybox script runs for the module
  and the area both). A voice plays out past its bubble.
- **Footsteps** (Q-0093): a character's, each time its footstep cycle
  (`move_body`'s `footstep_cycle`) passes 64 or 192 while it moves (Quake's
  footsteps rule): `footstepsounds.2da`'s row for its appearance's
  `footsteptype` (players: row 0, `Soft`), column the floor's
  (`surfacemat.2da` `sound` code of the walkmesh face traced under it:
  dirt, grass, stone, wood, water, carpet, metal, leaves), one of three at
  random, at its soles, speed 1 + `pitchoffset`, gain 0.6. A rolling or
  floating droid holds its `rolling` sound while it moves. A character
  standing still steps where its KOTOR clip passes an `snd_Footstep` event
  (a script's clip, a cutscene's stunt: `ambience::ClipSteps`), the K1
  executable's only source of footsteps; one moving is stepped by its walk
  alone, so no step is heard twice.
- **Gains**, a first mix to judge by ear: music 0.6, ambience 0.8 × its
  curve, objects their volume, footsteps 0.6.
- **The menu** (before a game): K1's main menu plays `mus_theme_cult`,
  character creation `mus_theme_rep` (reone's and xoreos's), looped.
- Nothing plays while a movie is up; the music begins again after it.
- **The screens' sounds** (`game::ui_sounds`, `UI.md` "Controls'
  sounds"): each `GuiSound` the screens make, by its `guisounds.2da` row's
  label, its `soundresref` read from the screens' install as first played
  (`sound::Added`, so the menu's mixer has them too), started at once and
  alike everywhere (`sound::Now`, `Mixer::play_now`: over a break or a
  movie too). The store's purchase `gui_invadd`, sale `gui_invdrop`; the
  equip page's item worn `gui_invselect`, taken off `gui_invdrop`; a party
  member come to a level (the HUD's mark lit) `gui_level`. A dump's
  `sound.cued` lists them as started.
- **The game's own interface sounds** (`game::ui_sounds`): a quest
  moved on `gui_quest`, one ended `gui_complete` (`guisounds.2da`
  `Quest_New_Notify`, `Quest_Complete_Notify`); Security's animations
  (`unlockdr`, `unlockcntr`) `gui_lockpick`, disarming or recovering a
  mine `gui_minedisarm`, setting one `gui_minearm`, where its user is, as
  the animation begins (K1's exe picks them by the animation); pazaak's
  `mgs_drawmain` (a main-deck card), `mgs_playside` (a side card),
  `mgs_startturn` (the player's turn), `mgs_winset` / `mgs_loseset`,
  `mgs_winmatch` / `mgs_losematch`, from the table's changes
  (`pazaak_view`).

`dump` shows `ambience`: the area, what plays (`music`, `battle`,
`stinger`, `ambient`), clips read and failed, objects sounding, sounds
cued, footsteps, doors' and containers' sounds. Each track that starts or
stops is logged (`sound: plays` / `stops`), so a run's log and trace
(`KJK_PERF=1`) show them.

## Movies

KOTOR's movies are Bink. `ffmpeg` (with `ffprobe`, from the `PATH`) makes
each one ours once (`movie`, `kjk movie import`): its frames as JPEGs
(`-q:v 6`, about 370 KB a second at 640 wide) and its sound as one MP3
(FLAC where `ffmpeg` has no MP3 encoder), in the store, keyed by the Bink's
bytes (`import/<game>/movies`). A movie not there yet is made when first
played (the 2-minute crawl in about 3 s); `kjk movie import k1` makes
every one up front (about 600 MB for K1's 27 minutes). Without `ffmpeg`
a movie is skipped, with a warning.

- **What plays them**: `PlayMovie` and `StartNewModule`'s `sMovie1..6`
  (which play over the load), and TSL's `PlayMovieQueue` (the movies
  `QueueMovie` kept, in order, one run: the galaxy map's takeoff,
  hyperspace and landing, the endings), sent to every seat as a script
  screen (`notice::Screen::Movie`); the console's `ui script movie 01a`; a
  menu opens with K1's logos, `leclogo`, `biologo`, `legal` (xoreos's
  order; a headless menu, a check, opens on the menu unless
  `KJK_MOVIES=1`). The Endar Spire's: `01a` (the opening crawl, its
  area's `OnEnter`), `01c` (the escape pod, into `STUNT_00`).
- **How** (`game::movie_view`): one after another, over everything,
  black to the window's edges from the first movie to the end of the last
  one queued (the menu under the logos never shows between two of them,
  and its music waits for the last), the picture fitted within 640 × 480
  keeping its shape, its sound a bed at full gain. Escape, Space, Enter or a click
  skips it, unless it was queued unskippable (`QueueMovie`'s flag; a skip
  of a skippable queued one drops the rest of its run, as TSL's player
  drops the rest of its list: QUIRKS Q-0237) (a press, taken: the conversation and screens under it get no
  keys or pointer while it is up). Play reads none of a player's input
  while one is up, the host's and a guest's alike (`sim_host::PlayerInput`:
  no keys, buttons or mouse turning; the host plays it for everyone). A headless run skips every movie (its
  checks and captures would film the crawl) unless `KJK_MOVIES=1`;
  `KJK_MOVIES=0` skips them in a window too (as xoreos can skip videos).
- **The world waits** (the owner's rule, 2026-10-02, as KOTOR freezes it):
  the host steps no ticks while its movie is up or a break holds (so in
  co-op the host's skip decides when play goes on; a guest's skip ends only
  its own picture)
  (`PARTY.md`), and every sound but the
  movie's is paused where it was (a voiced line, a saber's hum), the
  sounds waiting for their time waiting the pause out (`Mixer::hush`).
  What is heard, and the movie, run on the game's time plus the time the
  world has waited (`sim_host::Waited`: real time in a window, the held
  animation time's under `anim` and `record`), so a recording's frames
  and its `audio.wav` keep in step and its world waits too.
- `dump` shows `movie` (the one up: its frame and time) and `movies` (those
  played and queued). Subtitles (`*.bik.*sub.txt`, other languages) are
  not read; the English ones are in the pictures.
