# Safety net and flight recorder — design

Date: 2026-10-28. Status: draft for the owner's approval.

## Why

kjk is public and people will play it without Claude watching. Until now
every bug was caught live while the owner played. From here on, a bug reaches
us as a few words in a GitHub issue, if it reaches us at all. Two things have
to hold:

1. **A player never loses a playthrough** to a bad save, a crash or an update.
2. **A report carries enough to reproduce the bug exactly.** The sim is
   deterministic, so the starting state plus every tick's input replays the
   bug tick for tick. That is our big advantage, and this design makes it
   reach us.

There are no playtesters yet, so the game has to collect this unprompted.

## Goals

- Saves are backed up before a new build touches them, autosaved on module
  entry, and checked for corruption.
- A crash leaves a folder holding the backtrace, the log, the build, the
  system and a recording up to the crash, even when the process dies hard.
- The player can make a bug report in one key press. It is a zip they attach
  to a GitHub issue.
- We can open a report and replay it headless on its own build. Replaying it
  on main is a best-effort extra: it may diverge or be refused, and that is
  an answer too ("fixed on main?" / "data moved"). Making it always work is
  the migrations design, not this one.

## Non-goals (for this design)

- Making old-format saves load on new builds (migrations, the old-saves test in
  CI, automatic re-stamping, the story-state fallback). That is the next
  design.
- Native crash minidumps and shipping debug symbols.
- Uploading anything. Nothing leaves the player's machine unless they attach
  it themselves.
- Recording on a co-op joiner. The host's recording already holds every
  seat's input.
- A hang watchdog.

## Anti-goals (a failure even if it works)

- **A report that contains the games' files**, or anything copyrighted from
  them. State and inputs only.
- **A report that leaks who the player is.** The home folder and user name
  must not show in any file in the zip.
- A frame hitch from the recorder writing to disk. Writing happens off the
  main thread.
- Filling the disk. Every folder has a cap.
- Nagging. The game offers a report after a crash, once, and never pops one
  up during play.
- Any change to the sim hash or the save format (`KJKV`, `KJKS`). This work
  sits around the sim and leaves the sim alone.

## What exists today (survey, 2026-10-28)

- **Every run already gets a folder** at `settings::folder()/logs/<YYYYMMDD-HHMMSS-pid>`
  (`crates/game/src/runtime.rs:367-413`), and the newest `KEPT_RUNS = 10` are
  kept. The folder holds `kjk.log` (a tracing file layer, `runtime.rs:431-452`)
  and `manifest.json` (git sha, profile, render adapter).
- **Every live session is recorded**: a `sim::replay::Recorder` with
  checkpoints every 100 ticks (`crates/game/src/sim_host.rs:553-557`, records
  at `:1592`). The catch is that it lives in memory and is written as
  `inputs.kjkr` + `end.kjkv` only on `AppExit` (`sim_host.rs:1815-1846`). **A
  crash loses it.** A solo tick with no actions is about 28 bytes, roughly
  6 MB an hour at 60 Hz.
- `kjk play --replay <file>` plays a recording back (`cli_play.rs:86`).
- **Saves are atomic**: write `.<id>.new`, move the old one to `.<id>.old`,
  rename, and put back an orphaned `.old` (`crates/saves/src/lib.rs:188-231`).
  `meta.json` holds name, module, area, saved, seats and character. There is
  no checksum, no build info, no fsync and **no autosave**: `CB_AUTOSAVE` is
  greyed in the options screen (`crates/ui/src/screens/options.rs:103`).
- There is **no panic hook** in the game and no `--version`.
  `env!("KJK_GIT_SHA")` exists (`crates/game/build.rs`, with `-dirty`).
- A screenshot exists only as a console command (`crates/console/src/lib.rs:268`).
- The determinism test runs on Linux in CI, and on Windows and macOS only
  monthly or by hand (`manual.yml`).
- The user's folder is `settings::folder()`: `~/Library/Application Support/kjk`,
  `%APPDATA%\kjk`, or `~/.config/kjk`. Saves are in `saves/` (or `KJK_SAVES`).

## Design

### 1. Save safety net

**1a. Backup on a new build.** A file `saves/.build` holds the build that last
wrote to the saves folder (`KJK_GIT_SHA` + package version). At startup,
before anything lists or writes saves, if it differs from this build:

- copy the whole saves folder to `save-backups/update-<old version>-<old sha8>-<date>/`;
- then write the new build into `.build`.

`save-backups/` is the folder the save-compat skill already uses. Keep the
newest 5 `update-*` backups and never prune the skill's own folders. If the
copy fails (disk full), log it, show a one-line notice in the main menu and go
on. A failed backup must not stop the game.

**1b. Autosave on module entry** (K1's rule). This turns on `CB_AUTOSAVE` in
the options, default on, stored in `settings.json`. Once a module is installed
and the first tick after the install has run, the host writes an autosave
through the same `sim_host::save_world` path. If `saves::refusal` refuses
(a movie or conversation running, combat, someone down), it retries each
second until the refusal clears.

*My call:* three rotating slots `auto`, `auto-2` and `auto-3` instead of K1's
one. The newest is always `auto`, and the others shift down on write. The load
list shows them as "Autosave", "Autosave (earlier)" and so on, newest first.
Quick save and named saves are unchanged. Solo and host only, as with saves
today.

**1c. Checksum and build stamp in `meta.json`.** These are new optional fields,
so older builds and existing saves still parse:

- `sha256` of `save.kjkv`;
- `build` (sha), `version` (package version), `game` (`k1`/`tsl`);
- `formats` (the `KJKV` and `KJKS` version numbers).

Write `save.kjkv` and `meta.json`, then `fsync` both, then do the renames.

When saves are listed, a save whose bytes don't match its `sha256` shows as
damaged and can't be loaded. Saves without the field are trusted.

**1d. Better refusal text.** When a save is refused for its version or data, the
message says which build made it ("made with 0.3.1 (ab12cd34); this is
0.4.0"). It also says where the backups are and that an autosave or an older
build may still load it. The text points at the backups folder by name; it
does not show a full path with the user's name.

### 2. Crash folder

**2a. Panic hook.** Installed first thing in `main` for the windowed game, the
headless game, host and join (not for the probe verbs, which have their own).
It chains to the default hook, so stderr still gets the message. It writes
`crash.txt` into the current run folder with:

- the message, the location and the thread name;
- `std::backtrace::Backtrace::force_capture()`;
- the build, OS and render adapter (as in `manifest.json`);
- the **breadcrumbs**: a small global (`Mutex` read with `try_lock` in the
  hook, so a panic while it is held still writes the rest). The sim host
  updates it each tick with the tick number, module, area, the current
  conversation (dlg resref and node), the running script (resref) where
  cheaply known, the party's tags and the active screen.

Then it flushes the flight recorder (below) and the log.

**2b. A clean-exit marker.** On a normal exit the run folder gets `ended`. At
startup the previous run folder is checked. If it has `crash.txt`, or it lacks
`ended` (a hard crash, kill or power loss), the main menu shows once: "kjk
stopped unexpectedly last time." It offers **Make a bug report** and **Not
now**. The answer is remembered in that run folder (`seen`), so it never asks
twice.

**2c. Keep crashed runs.** `prune_runs` keeps the newest 10 runs as now, plus
any run with `crash.txt` from the last 30 days, at most 10 of those.

### 3. Flight recorder

The recording has to survive a crash, so it is streamed to disk instead of
held until exit.

- **New container, `flight.kjkf`**, in the run folder. A header (magic,
  version, build sha), then length-prefixed chunks. Each chunk carries a CRC32.
  - A *start* chunk: what `Recorder::new` takes today (hz, start snapshot,
    level, script state).
  - *Tick* chunks: a batch of ticks (msec + `TickInput`) and their checkpoints.
  - *Install* chunks: the same data `Recorder::install` gets.
- **Batching and the writer thread:** the sim host sends each tick to a writer
  thread over a channel. The writer appends a tick chunk every second (or 60
  ticks) and flushes. A panic or kill loses at most the last second.
- **Bounded size:** at each module entry (right after the autosave in 1b),
  the recorder starts a new segment, `flight-<n>.kjkf`, with a fresh start
  snapshot. The run folder keeps the current segment and the one before it.
  A report always holds the current module's play from its start, plus the
  module before.
- **Reading:** a reader turns segments into a `sim::replay::Recording`. A
  truncated last chunk, or one that fails its CRC, ends the recording there
  without failing. `kjk play --replay` accepts `.kjkf` as well as `.kjkr`.
- **The existing exit path stays:** `inputs.kjkr` / `end.kjkv` on `AppExit`
  are still written, so nothing that uses them breaks. The builder may make
  them derive from the flight segments.
- **Co-op host:** the host already records every seat, so it streams the
  same way. Joiners don't record (a non-goal).

### 4. Bug report

**4a. How the player makes one:**

- **F12** in play, or **Report a bug** in the pause menu and the main menu;
- or **Make a bug report** on the crash notice (2b).

**4b. What happens:**

1. Take a screenshot of the frame (before any report UI shows).
2. Show a small screen in the game UI's style with a text box: "What went
   wrong?" It uses the typed-name text entry from the save screen, with
   **Make report** and **Cancel**. The game stays paused while it is open
   (solo; in co-op the host's world keeps running).
3. Write `reports/kjk-report-<YYYYMMDD-HHMMSS>.zip` in `settings::folder()`.
4. Show "Report saved" with **Open folder** (reveal in Finder/Explorer/file
   manager) and **Open GitHub issue** (opens the browser at the repo's
   new-issue page with `template=bug.yml`; no data in the URL).

**4c. What the zip holds:**

| File | Where it comes from |
|---|---|
| `note.txt` | the player's text |
| `screenshot.png` | step 1 |
| `system.txt` | build sha + version, OS + version, CPU, RAM, render adapter + driver + backend, resolution + window mode, game (k1/tsl), digest of the game's key file |
| `manifest.json`, `kjk.log`, `crash.txt` | the run folder (for a crash report, the crashed run's folder) |
| `flight-*.kjkf` | the run's flight segments |
| `now.kjkv` | the world as a save, encoded now (solo and host, when not crashed) |
| `settings.json` | the settings, with paths scrubbed and `host.id` (a stable id of the install) removed |
| `breadcrumbs.txt` | the breadcrumbs (2a) at the moment of the report |

There are no game files, and nothing from the save folders except `now.kjkv`.

**4d. Scrubbing.** Every text file in the zip goes through one function. It
replaces the home folder with `~`, the user name with `<user>`, and the
machine's host name with `<host>`. A test feeds it Windows, macOS and Linux
paths. The zip is built in memory, then written in one go.

The snapshots (`now.kjkv`, the flight segments' start snapshots) hold the
character names, every co-op seat's included. The report screen says so in
one line ("The report holds your save, character names included"). The
"no home folder" test searches the bytes of every file in the zip, binary
ones too.

**4e. Cap.** `reports/` keeps the newest 20 zips. A zip larger than 25 MB (the
GitHub attachment limit) drops `flight-*` segments, oldest first, until it
fits. `system.txt` then says what was dropped.

**4f. The GitHub side.** `.github/ISSUE_TEMPLATE/bug.yml` asks:

- what happened and what was expected;
- the game (K1/TSL) and where it came from (Steam/GOG/disc);
- the kjk version;
- the zip (a required checkbox: "I attached the report zip, or could not
  make one").

`config.yml` turns off blank issues.

### 5. Our side: opening a report

- `kjk report show <zip>`: build, game, system, the note, the breadcrumbs, the
  crash message and the top of the backtrace, the last 40 log lines, and what
  the flight segments span (ticks, modules).
- `kjk report replay <zip> [--to-tick N] [--headless]`: unpacks into a scratch
  folder and plays the flight segments with the existing replay path. It says
  whether the report's build matches this one. If not, it names the commit to
  check out, since a recording replays exactly only on its own build.
  Divergence uses the existing `Divergence` reporting.
- A short `docs/BUG-REPORTS.md`: for players, how to make a report and where
  it is saved; for developers, how to show and replay one.

### 6. Small extras that make reports useful

- `kjk --version` prints the package version + sha. The main menu shows the
  same in a corner in small text (the K2 retint's font).
- The determinism test runs on Windows and macOS in `release.yml` before
  packing. A release whose sim hash differs across OSes fails. This is what
  makes a Windows player's recording replay on the owner's Mac.

## Which runs get what (my call)

The player's game is a windowed `kjk` launch, `kjk play` without harness flags,
`kjk host` and `kjk join`. Only those get:

- the backup on a new build;
- autosaves;
- the crash notice.

Headless runs, `kjk chain`, the probes and every harness verb get none of
these, so the chain's timing and CI disk are untouched. The flight recorder
and the panic hook run everywhere, since they cost almost nothing and a chain
crash is a crash too.

The backup triggers when the **package version** changes, not the sha, so a
developer's commits don't rotate the owner's backups away. It is skipped when
`KJK_SAVES` is set.

## Replay builds (my call)

"Its own build" means the same sha **and the release profile**. Release
recordings are replayed with a release build of that sha. `kjk report replay`
checks the report's sha against its own and says which tag to check out. It
doesn't fetch binaries. Debug builds may be used but are flagged as possibly
diverging.

## Mods and other editions

Replay checks each level's digest, and those digests come from the player's
own game files. A different edition, patch or `Override` folder refuses the
replay on the owner's machine. The report therefore carries:

- in `system.txt`, the names, sizes and sha256 of every file in `Override`;
- in `data.txt`, the digest of every module the flight segments touch.

`kjk report show` says "modded" or "other edition" when these differ from the
owner's install. The issue form asks about mods. *My call (the owner left it
to me):* `kjk report replay --ignore-data` skips the level and install digest
checks and replays anyway. It reports the first divergence like any other
replay. Most players will have a different edition or patch than the owner,
and a replay that runs part of the way still shows where things went wrong.

## Order of work

0. Find out whether the sim hash matches on Windows, macOS and Linux today:
   run `manual.yml`'s platforms check by hand (the recent runs were the
   screenshot job only). If it doesn't match, fixing that comes before
   step 3, since recordings from other OSes are worthless without it.
1. Version string + panic hook + clean-exit marker + crash notice (2, 6a).
2. Save safety net (1a-1d).
3. Flight recorder (3).
4. Bug report + issue form (4).
5. `kjk report show/replay` + docs (5).
6. Cross-OS determinism in `release.yml` (6b).

Each step is useful alone and can merge alone. Steps 3-5 depend on each
other in that order.

## How we know it works

- A probe that panics on purpose in a headless run. The run folder gets
  `crash.txt` with a backtrace and breadcrumbs, the flight segment replays to
  within 60 ticks of the panic, and the next launch shows the crash notice
  once.
- A probe that kills the process (`SIGKILL`) mid-module. The flight segment
  still reads, and the notice shows.
- The save round trip test grows a damaged-save case (one byte flipped), a
  backup-on-new-build case, and an autosave-rotation case.
- The scrubber test (4d), and a test that a report zip has no path holding the
  home folder.
- A recorder test: replaying from `.kjkf` gives the same hashes as the
  in-memory `Recording`.
- The sim hash is unchanged. This work adds no bump file for `sim`, `save` or
  `snapshot`.

## Left to the builder

- Where exactly the breadcrumbs are updated, and how cheaply the running
  script can be known.
- The exact chunk encoding of `.kjkf` (reuse `sim::wire`).
- The zip crate (MIT/Apache, no C dependency) and the sha256/crc32 crates.
- Whether F12 conflicts with an existing binding. If it does, pick a free key
  and tell the owner.
- Whether `inputs.kjkr` on exit is kept as-is or derived from the flight
  segments.
