# Safety Net and Flight Recorder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Players never lose a playthrough. Every crash or report reaches us
with a recording that replays the bug.

**Architecture:** This work sits around the sim and leaves the sim's state
alone:
- the `saves` crate grows backups, autosave slots and checksums;
- a new `game::crash` module adds the panic hook, breadcrumbs and the
  clean-exit marker;
- a new `sim::flight` container streams the recording to disk from a writer
  thread;
- a new `game::report` module builds a scrubbed zip;
- `kjk report show|replay` opens one.

**Tech Stack:** Rust, Bevy, clap, serde_json, blake3 (workspace dep),
crc32fast (already in Cargo.lock), and two new crates: `zip` (default features
off, `deflate` on) and `sysinfo`.

**Spec:** `docs/superpowers/specs/2026-10-28-safety-net-recorder-design.md`

## Global Constraints

- The sim hash (`KJK_SIM_HASH`) stays unchanged, and so do `KJKV` (`save.rs:108`,
  59) and `KJKS` (`snapshot.rs:176`, 72). There is no `bumps/*.toml` for
  `sim`, `save` or `snapshot`.
- `.kjkf` is a new format with its own magic `KJKF`, version 1. Bump it with a
  bump file only if it changes after merge.
- **A "player run"** is a non-headless run whose script (`cmds`) is empty:
  `kjk`, `kjk play`, `kjk host` or `kjk join`. Only player runs get the backup,
  autosave and crash notice. Panic hook, breadcrumbs and flight recorder run
  in every `runtime::run`.
- The backup triggers on a change of `CARGO_PKG_VERSION`, not the sha. It is
  skipped when `KJK_SAVES` is set. Keep 5 `update-*` backups.
- Autosave slots are `auto`, `auto-2` and `auto-3`, newest in `auto`. The
  setting is `game.autosave`, default `"true"`.
- Report zips go in `settings::folder()/reports/kjk-report-<YYYYMMDD-HHMMSS>.zip`.
  Keep 20. Each is at most 25 MB; drop the oldest `flight-*` first.
- No path in a report holds the home folder, user name or host name. Every
  file in the zip, binary ones included, is searched in the test.
- `host.id` is removed from the report's `settings.json`. No game files go in a
  report.
- Issue URL: `https://github.com/kjk-engine/kjk/issues/new?template=bug.yml`.
  No data goes in the URL.
- **Deviation from the spec:** the checksum is blake3, the workspace's hash,
  not sha256. The `meta.json` field is `blake3`.
- Every `kjk` run in a test or by hand passes `--mute`.
- Done means `cargo test` for the touched crates,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --release -p approved_tests` all pass.
- Commit messages are one plain descriptive sentence in the repo's style, with
  no attribution lines.

## Review Focus

1. **A first launch with no saves folder, or an empty one.** The backup is a
   no-op, `.build` is written, and no empty backup folder is left. Test in
   Task 2.
2. **The disk fills or a write fails during a backup or autosave.** The game
   goes on with a warning and leaves no half-written slot (`.new` is cleaned
   up). Test in Task 2.
3. **A panic off the main thread, or while the breadcrumbs lock is held.**
   `crash.txt` is still written and the hook never deadlocks (`try_lock`).
   Test in Task 1.
4. **A truncated or empty `.kjkf`** (killed mid-write, a zero-byte file). The
   reader returns the ticks up to the last whole chunk; an empty file gives a
   clear error, not a panic. Test in Task 3.
5. **A non-ASCII home folder or user name, or one with a space**
   (`C:\Users\Åsa`, and the same kinds of names under Linux's and macOS's
   home folders; the test builds these paths at run time). It is scrubbed in text and found by the
   byte search. Test in Task 4.

---

### Task 0: Is the sim hash the same on all three OSes?

The spec says to find this out before the recorder work. This is not a code
task.

- [ ] **Step 1:** Start the `manual.yml` workflow on `main`:
  `GH_CONFIG_DIR=~/.config/gh-kjk gh workflow run manual.yml --ref main`.
  Its `platforms` job runs `cargo test --release -p approved_tests` on Windows
  and macOS. Linux CI already pins the hash.
- [ ] **Step 2:** When it finishes, read `gh run view <id> --log-failed`.
  - Green on both: note it in the PR description and go on.
  - `sim_determinism` fails on either: stop and report the hash difference to
    the owner before Task 3. Tasks 1 and 2 can still go ahead.

### Task 1: Version, panic hook, breadcrumbs, clean exit, crash notice

**Files:**
- Create: `crates/game/src/crash.rs`, `crates/game/src/crash/probes.rs`
- Modify:
  - `crates/game/src/lib.rs` (add `pub mod crash;`)
  - `crates/game/src/runtime.rs:64-76` (install the hook, insert `PlayerRun`),
    `:376` (`prune_runs`), the `AppExit` path (`ended`)
  - `crates/game/src/sim_host.rs` `advance` (~1152, breadcrumbs per tick)
  - `crates/game/src/session_flow.rs` (crash notice)
  - `crates/launcher/src/lib.rs:136` (`version`)
  - the main menu screen `crates/ui/src/screens/main_menu.rs` (version label)

**Interfaces (produces):**
- `#[derive(Resource, Clone, Copy)] pub struct PlayerRun(pub bool);` in
  `crash.rs`. `runtime::run` inserts it as `!options.headless &&
  options.script.trim().is_empty()`.
- `#[derive(Default, Clone, Debug)] pub struct Breadcrumbs { pub tick: u64, pub module: String, pub area: String, pub conversation: Option<(String, u32)>, pub script: Option<String>, pub party: Vec<String>, pub screen: Option<String> }`
- `pub fn note(f: impl FnOnce(&mut Breadcrumbs))`. Updates a
  `static CRUMBS: Mutex<Breadcrumbs>`.
- `pub fn crumbs() -> Option<Breadcrumbs>`. Uses `try_lock`, so it returns
  `None` while the lock is held.
- `pub fn install_hook(folder: PathBuf, facts: String)`. Chains to the
  previous hook. `facts` is the build/OS/adapter text.
- `pub fn write_crash(folder: &Path, message: &str, location: &str, thread: &str, backtrace: &str, facts: &str, crumbs: Option<&Breadcrumbs>) -> io::Result<()>`.
  Writes `crash.txt`. The hook calls it.
- `pub const ENDED: &str = "ended"; pub const SEEN: &str = "seen"; pub const CRASH: &str = "crash.txt";`
- `pub fn unexpected_end(runs: &Path, current: &Path) -> Option<PathBuf>`.
  Returns the newest run folder other than `current` that has `crash.txt`,
  or lacks `ended`, and has no `seen`.

- [ ] **Step 1: Write failing tests** in `crash/probes.rs`:
  - `write_crash_holds_everything`: `crash.txt` contains the message,
    location, thread, the backtrace text, the facts and `tick 42` from
    crumbs.
  - `write_crash_without_crumbs`: `None` writes `breadcrumbs: unavailable`.
  - `crumbs_while_locked_is_none`: hold the lock on another thread; `crumbs()`
    returns `None` within 10 ms.
  - `unexpected_end_cases`, a table driven over folders:
    - `{crash.txt}` → Some
    - `{}` → Some
    - `{ended}` → None
    - `{crash.txt, seen}` → None
    - the current folder → None
  - `prune_keeps_crashes`: 12 plain runs + 3 crashed (one 31 days old by
    folder name) → 10 plain + 2 crashed remain.
- [ ] **Step 2:** Run `cargo test -p game crash`. Expected: FAIL (module
  missing).
- [ ] **Step 3: Implement `crash.rs`.**
  - Install the hook in `runtime::run` right after `create_run_folder()`.
    `facts` = `KJK_GIT_SHA`, `CARGO_PKG_VERSION` and `std::env::consts::OS`.
    The adapter is added later by `log_render_adapter` via a
    `crash::set_adapter(String)` that writes a second static.
  - Use `std::backtrace::Backtrace::force_capture()`.
  - In `prune_runs`, keep `keep` plain runs plus at most 10 crashed runs whose
    folder-name date is within 30 days.
  - Write `ended` in the same `AppExit` reader that `write_session` uses
    (`sim_host.rs:1817`), or a sibling system in `runtime.rs`.
- [ ] **Step 4: Breadcrumbs.**
  - In `advance`, after each stepped tick, call `crash::note` with the tick,
    level name, area, the party's tags, and the conversation (dlg resref +
    node) when one is running.
  - The running script is set where the script VM starts a script, if that
    is a single call site. Otherwise leave it `None` and say so in the PR.
  - The UI's top screen goes in from `ui_view.rs`.
  - Keep it cheap: no allocation when nothing changed.
- [ ] **Step 5: Crash notice.**
  - At startup of a player run, `unexpected_end(&runtime::runs(), &folder)`.
  - When it returns Some, set `ui.vm.session.error = Some("kjk stopped unexpectedly last time.".into())`
    and open `message` over `main_menu`, the same pattern as
    `session_flow.rs:469-477`.
  - Write `seen` into that folder at once.
  - Task 4 adds the **Make a bug report** button to that message.
- [ ] **Step 6: `--version` and the menu label.**
  - `#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("KJK_GIT_SHA"), ")"))]`.
    If `KJK_GIT_SHA` isn't set for the launcher, use the one game's build.rs
    sets.
  - Main menu: bind a spare `LB_` label of `mainmenu16x12` to the same text,
    small.
- [ ] **Step 7: Crash probe.**
  - Add a console command `crash`, under the `probes` feature, that panics
    with `"crash requested"`.
  - Run `kjk --headless --mute --cmds "crash"`.
  - Expected: the newest run folder has `crash.txt` with `crash requested`
    and a backtrace, and no `ended`.
  - Then run `kjk --headless --mute --cmds "quit"` (or the repo's exit
    command). Expected: that run has `ended`.
- [ ] **Step 8:** Run the tests, then
  `cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 9:** Commit.

### Task 2: Save safety net

**Files:**
- Modify:
  - `crates/saves/src/lib.rs` (Meta 41, `valid_id` 72, `Listing` 100, `list`
    111, `read` 141, `write_files` 188)
  - `crates/saves/src/probes.rs`
  - `crates/game/src/sim_host.rs` (`save_world` 1020, the install arm ~1262-1330)
  - `crates/game/src/settings.rs` (`DEFAULTS` 430, `page_keys` 475)
  - `crates/ui/src/screens/options.rs:103`
  - `crates/ui/src/screens/save_load.rs` (labels, damaged)
  - `crates/sim/src/save.rs` / `snapshot.rs` (expose the version numbers only)

**Interfaces (produces):**
- New `Meta` fields, all `#[serde(default, skip_serializing_if = "Option::is_none")]`:
  - `blake3: Option<String>`, `build: Option<String>`, `version: Option<String>`,
    `game: Option<String>`;
  - `formats: Option<Formats>`, where
    `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] pub struct Formats { pub save: u32, pub snapshot: u32 }`.
  - `write_files` fills `blake3` from the bytes; the callers fill the rest.
- `pub const AUTOS: [&str; 3] = ["auto", "auto-2", "auto-3"];` and `valid_id`
  accepts them.
- `pub fn write_auto(root: &Path, world: &[u8], meta: &Meta) -> Result<String, String>`.
  Rotates `auto-2`→`auto-3`, `auto`→`auto-2`, then writes `auto`.
- `Listing` gains `pub damaged: Vec<String>`: ids whose `save.kjkv` blake3
  doesn't match.
- `pub fn backup_on_new_version(root: &Path, backups: &Path, version: &str, build: &str) -> Result<Option<PathBuf>, String>`.
  - It reads and writes `root/.build` (`"<version> <build>"`).
  - It copies `root` to `backups/update-<old version>-<old sha8>-<YYYYMMDD>/`.
  - It keeps the newest 5 `update-*` and returns the backup's path.
- `sim::save::FORMAT: u32` and `sim::snapshot::FORMAT: u32`, both `pub const`
  equal to `VERSION`.
- `pub(crate) fn store(authority: &mut Authority, slot: Slot, meta_name: &str, settings: Option<&Settings>) -> Result<String, String>`
  in `sim_host.rs`, with `enum Slot { Named(Option<String>), Auto }`. It is the
  shared body of `save_world` and the autosave.
- `#[derive(Resource, Default)] struct AutosaveDue { since: Option<Instant> }`
  and the system `fn autosave(...)`, which tries once a second while `since`
  is Some.

- [ ] **Step 1: Write failing tests** in `saves/src/probes.rs`:
  - `meta_round_trips_new_fields`
  - `old_meta_without_fields_reads`: a JSON with only the old fields
  - `damaged_save_listed`: flip one byte of `save.kjkv` → `damaged` has the
    id, and `read` errors with `damaged`
  - `save_without_blake3_trusted`
  - `autos_rotate`: 4 writes → `auto`, `auto-2` and `auto-3` hold writes 4, 3
    and 2
  - `backup_cases`, a table:
    - no root → `Ok(None)`, `.build` written, no backup folder
    - same version → `Ok(None)`
    - new version → a folder with every slot
    - 7 updates → 5 kept
  - `failed_write_leaves_no_slot`: the root is a read-only folder → `Err`, no
    `.<id>.new` left behind
  - `refusal_names_build`: `read` of a save whose decode fails with
    `DecodeError::Version` and whose meta has `version: "0.3.1", build:
    "ab12cd34…"` → the error contains `made with 0.3.1 (ab12cd34)`, this
    build's version, and `save-backups`
- [ ] **Step 2:** Run `cargo test -p saves`. Expected: FAIL.
- [ ] **Step 3: Implement the saves crate changes.**
  - `fsync`: `File::sync_all` on both files, then on the `.new` folder where
    the OS allows (ignore the error on Windows), before the renames.
- [ ] **Step 4:** Run `cargo test -p saves`. Expected: PASS.
- [ ] **Step 5: Wire the game.**
  - **Backup:** at startup of a player run, before the first `saves::list`,
    call `backup_on_new_version(saves_root, folder()/save-backups, CARGO_PKG_VERSION, KJK_GIT_SHA)`.
    Skip it when `KJK_SAVES` is set. An `Err` goes to `warn!` plus a one-line
    message on the main menu.
  - **Store:** factor `store` out of `save_world`, filling `build`, `version`,
    `game` and `formats`.
  - **Autosave:**
    - In the install arm, after `authority.installs.push`, set
      `AutosaveDue.since = Some(now)` when the run is a player run, solo or
      host, and `settings.value("game.autosave") == Some("true")`.
    - `autosave` calls `store(.., Slot::Auto, ..)` once a second until it gets
      `Ok`.
    - A refusal (movie, conversation, combat, down, loading) waits for the next
      second. Any other `Err` is a `warn!`, and `since` clears.
  - **Settings:** add `("game.autosave", "true")` to `DEFAULTS` (bump its
    length) and to `page_keys("options_gameplay")`. In the options screen use
    `("CB_AUTOSAVE", Toggle("game.autosave"))`.
  - **Load list:** show `auto` as "Autosave", and `auto-2`/`auto-3` as
    "Autosave (earlier)". Show damaged saves greyed with "damaged" and refuse
    to load them.
- [ ] **Step 6: Probe.** Run a headless player-like run that installs a
  module.
  - If headless runs can't be player runs, add a `--player-run` probe flag
    under `probes` that forces `PlayerRun(true)`.
  - Expected: `saves/auto/meta.json` has `blake3`, `build`, `version`,
    `formats`.
  - A second install rotates it to `auto-2`.
- [ ] **Step 7:** Run `cargo test -p saves -p game`, clippy, and
  `cargo test --release -p approved_tests`. Expected: PASS, hash unchanged.
- [ ] **Step 8:** Commit.

### Task 3: Flight recorder

**Files:**
- Create: `crates/sim/src/flight.rs`, `crates/sim/src/flight/probes.rs`,
  `crates/game/src/flight_writer.rs`
- Modify:
  - `crates/sim/src/lib.rs` (`pub mod flight;`)
  - `crates/sim/src/replay.rs` (an option to skip the data check)
  - `crates/game/src/sim_host.rs` (`Source::Live` 553, record 1592, install arm,
    `write_session` 1817)
  - `crates/launcher/src/cli_play.rs:86` (accept `.kjkf`)

**Interfaces (produces):**
- **`sim::flight` (pure, no I/O)** has `pub const MAGIC: &[u8;4] = b"KJKF"; pub const VERSION: u32 = 1;`
- `pub struct FlightWriter`:
  - `new(hz: u32, start: &Snapshot, level: LevelId, script: &ScriptVmState, build: &str) -> (Self, Vec<u8>)`.
    It returns the header and start chunk.
  - `fn tick(&mut self, msec: u32, input: &TickInput, after: &Snapshot) -> Option<Vec<u8>>`.
    Every 60 ticks it returns a tick chunk ending with the hash of `after`.
  - `fn install(&mut self, announcement: &Announcement) -> Vec<u8>`. It flushes
    the pending ticks first, in the same returned bytes.
  - `fn flush(&mut self, after: &Snapshot) -> Vec<u8>`
- **Chunks:** `kind u8` (0 start, 1 ticks, 2 install), `len u32`, the
  payload, `crc32 u32` of the payload. Use `crc32fast`.
- **The payloads** use the same per-tick encoding as `Recorder` (factor that
  out of `replay.rs` instead of copying it).
- `pub fn read(bytes: &[u8]) -> Result<Recording, DecodeError>`.
  - It stops at the first truncated chunk, or one that fails its CRC.
  - `Recording.ticks` are cut to the last whole tick chunk.
  - `checkpoints` are each chunk's end hash; `end` is the last one.
  - A file with no start chunk is `Err`.
- `pub fn sniff(bytes: &[u8]) -> bool`: true when the bytes start with `KJKF`.
- **`replay.rs`:** `pub fn play_checked(&self, level: &Level, levels: &mut dyn FnMut(&Announcement) -> Result<Level, String>, watch: &mut dyn FnMut(&Snapshot), check_data: bool) -> Result<Snapshot, Divergence>`.
  `play_watched` calls it with `true`. With `false` it skips the `level.check`
  calls.
- **`game::flight_writer`:**
  - `pub struct Flight { tx: Sender<Message> }`, a `Resource`.
  - `Message::{Segment(PathBuf, Vec<u8>), Append(Vec<u8>)}`.
  - The writer thread `kjk-flight` appends and calls `flush()` after each
    message.
  - `pub fn segment_path(folder: &Path, n: u32) -> PathBuf` gives
    `flight-<n>.kjkf`.
  - On a new segment, it deletes `flight-<n-2>.kjkf`.

- [ ] **Step 1: Write failing tests** in `sim/src/flight/probes.rs`:
  - `flight_matches_recorder`: drive a small world for 250 ticks through both
    `Recorder` and `FlightWriter` → `flight::read(bytes).play_watched(..)` hits
    the same final hash as the `Recorder`'s `Recording`.
  - `truncated_keeps_whole_chunks`: cut the bytes at every offset in the last
    chunk → `read` is `Ok` with 240 ticks.
  - `bad_crc_stops`
  - `empty_is_error`: `&[]`, and the header only
  - `install_chunk_round_trips`
  - `skip_data_check`: a `Recording` with a wrong level digest → `play_watched`
    is `Err`, and `play_checked(.., false)` is `Ok`.
- [ ] **Step 2:** Run `cargo test -p sim flight`. Expected: FAIL.
- [ ] **Step 3:** Implement `sim::flight` and `play_checked`.
- [ ] **Step 4:** Run `cargo test -p sim`. Expected: PASS.
- [ ] **Step 5: Wire the game.**
  - Next to the existing `Recorder`, `Source::Live` holds a `FlightWriter`.
  - Each tick, send `Append` when `tick` returns bytes.
  - On each module install (the install arm), finish the segment and start
    `flight-<n+1>` with a fresh start chunk. That covers a reload too.
  - `write_session` sends a final `flush`.
  - `inputs.kjkr` / `end.kjkv` stay as they are.
- [ ] **Step 6:** `cli_play` `--replay`: when `flight::sniff(bytes)`, read with
  `flight::read`. Segments after the first carry their own start, so a path
  plays one segment.
- [ ] **Step 7: Probes.**
  - Run `kjk --headless --mute --cmds "<walk a few seconds>; crash"`. Expected:
    the run folder's last `flight-*.kjkf` replays with `kjk play --replay <it>
    --headless --mute` to within 60 ticks of the crash's breadcrumb tick.
  - Repeat with the process killed (`kill -9`) instead of `crash`: the file
    still reads.
- [ ] **Step 8:** Run `cargo test --release -p approved_tests` and clippy.
  Expected: PASS, hash unchanged.
- [ ] **Step 9:** Commit.

### Task 4: Bug report

**Files:**
- Create:
  - `crates/game/src/report.rs`, `crates/game/src/report/probes.rs`
  - `crates/ui/src/screens/report.rs`
  - `.github/ISSUE_TEMPLATE/bug.yml`, `.github/ISSUE_TEMPLATE/config.yml`
- Modify:
  - `crates/game/Cargo.toml` (`zip`, `sysinfo`)
  - `crates/game/src/settings.rs` (`Action::Report` on F12 in `ACTIONS` 78)
  - `crates/ui/src/screens/mod.rs` (register `report`)
  - `options_in_game.rs`, `main_menu.rs` (entry points), the crash notice from
    Task 1

**Interfaces (produces):**
- `pub fn scrub(text: &str, home: &str, user: &str, host: &str) -> String`.
  - It replaces `home` with `~`, then `user` with `<user>`, then `host` with
    `<host>`.
  - It replaces both slash forms of `home`, and is case-insensitive on
    Windows paths.
- `pub fn identity() -> (String, String, String)` gives the home, user and
  host of this machine.
- `pub struct Part { pub name: String, pub bytes: Vec<u8>, pub text: bool }`
- `pub fn system_text(adapter: &str, resolution: (u32,u32), window_mode: &str, game: &str, key_digest: &str, overrides: &[(String, u64, String)]) -> String`
- `pub fn gather(run: &Path, note: &str, screenshot: Option<Vec<u8>>, now: Option<Vec<u8>>, settings_json: &str, crumbs: Option<&crash::Breadcrumbs>, system: String) -> Vec<Part>`.
  - It reads `manifest.json`, `kjk.log`, `crash.txt` and every
    `flight-*.kjkf` from `run`.
  - It drops `host.id` from the settings.
  - It adds `data.txt` with the digest of each module the flight segments
    name.
- `pub fn zip(parts: Vec<Part>, limit: usize) -> (Vec<u8>, Vec<String>)`.
  - It scrubs text parts and builds the zip in memory.
  - Over `limit`, it drops `flight-*` oldest first and returns the dropped
    names, which are noted in `system.txt`.
- `pub fn save(reports: &Path, bytes: &[u8]) -> io::Result<PathBuf>` keeps 20.
- `pub fn reveal(path: &Path)` and `pub fn open_issue()` use
  `std::process::Command` with `open`/`explorer`/`xdg-open`; no new crate.

- [ ] **Step 1: Write failing tests** in `report/probes.rs`:
  - `scrub_cases`, a table:
    - `C:\Users\Åsa\AppData\Roaming\kjk\saves` → `~\AppData\Roaming\kjk\saves`
    - `c:/users/åsa/x` → `~/x`
    - Linux home `zoë` + `/.config/kjk` → `~/.config/kjk`
    - macOS home with a space in the name + `/Library/...` → `~/Library/...`
    - (Build the Linux and macOS homes with `format!` from parts, so the repo's
      publish check doesn't flag them as personal paths.)
    - `user zoë on box-7` (with host `box-7`) → `user <user> on <host>`
  - `zip_has_no_identity`: text parts that hold the home, user and host →
    a byte search of every decompressed file in the zip finds none of them.
  - `snapshot_has_no_home`: binary parts are never scrubbed, so the encoded
    snapshot of a probe world (run with `KJK_SAVES` and `KJK_SETTINGS` under
    the home folder) must not contain the home folder's bytes.
  - `settings_drop_host_id`
  - `zip_cap_drops_oldest_flight`: 3 flight parts of 10 MB each, limit 25 MB
    → `flight-1` dropped and named.
  - `save_keeps_20`
- [ ] **Step 2:** Run `cargo test -p game report`. Expected: FAIL.
- [ ] **Step 3:** Implement `report.rs`. Use `sysinfo` for CPU, RAM and OS
  version; the adapter comes from `DumpFacts["render_adapter"]`.
- [ ] **Step 4:** Run the tests. Expected: PASS.
- [ ] **Step 5: The report screen.**
  - **F12** (a new `Action::Report`, file key `report`, label "Report a bug",
    rebindable like the others) takes a screenshot first. Use the console's
    `screenshot()` capture with an observer that hands the PNG bytes over.
  - Then it opens `report`. Reuse the save screen's name layout and
    `EditBox`, with up to 500 bytes of text, the title "What went wrong?", the
    line "The report holds your save, character names included.", and
    **Make report** / **Cancel**.
  - A solo game pauses while the screen is open.
  - **Make report** gathers, zips and saves, then shows the message "Report
    saved." with **Open folder** and **Open GitHub issue**.
  - Add a pause-menu button on a spare `optionsingame` control, and a
    main-menu button on a spare `mainmenu16x12` control. If either layout has
    no spare control, F12 alone serves that screen; say so in the PR.
  - The crash notice gets **Make a bug report**. It builds the report from
    the crashed run's folder, with no screenshot and no `now.kjkv`.
- [ ] **Step 6: The issue form.**
  - `bug.yml` has the fields:
    - what happened, and what you expected (both required);
    - game: a K1/TSL dropdown;
    - where it came from: Steam/GOG/disc/other;
    - mods: yes/no plus details;
    - the kjk version (required; "shown in the main menu's corner, or
      `kjk --version`");
    - a required checkbox: "I attached the report zip, or could not make one".
  - `config.yml` has `blank_issues_enabled: false`.
- [ ] **Step 7: Probe.** A headless player-like run with a console
  `report "probe note"` command (under `probes`) writes a zip.
  - Expected: it holds `note.txt`, `system.txt`, `manifest.json`, `kjk.log`,
    `flight-*.kjkf`, `now.kjkv`, `settings.json` and `breadcrumbs.txt`.
  - `strings` over the whole zip finds no `$HOME` and no `$USER`.
- [ ] **Step 8:** Run clippy and tests. Expected: PASS.
- [ ] **Step 9:** Commit.

### Task 5: `kjk report show|replay` and docs

**Files:**
- Create: `crates/launcher/src/report_verbs.rs`, `docs/BUG-REPORTS.md`
- Modify:
  - `crates/launcher/src/lib.rs` (`Command::Report { verb: ReportVerb }`,
    dispatched like `Command::Store` at 632; not behind `probes`)
  - `crates/launcher/Cargo.toml` (`zip`)

**Interfaces:**
- `enum ReportVerb { Show { zip: PathBuf }, Replay { zip: PathBuf, #[arg(long)] to_tick: Option<u64>, #[arg(long)] headless: bool, #[arg(long)] ignore_data: bool } }`
- `pub(crate) fn show(zip: &Path) -> ExitCode` prints, in order:
  - the build + version, and whether it matches this build;
  - the game and the system;
  - the note;
  - the breadcrumbs;
  - the crash message + the first 15 backtrace lines;
  - the last 40 log lines;
  - the flight segments' tick spans and modules;
  - "modded" or "other edition" when `system.txt`'s overrides or `data.txt`'s
    digests differ from this install's.
- `pub(crate) fn replay(..) -> ExitCode`:
  - It unpacks to `std::env::temp_dir()/kjk-report-<stem>/`.
  - A sha mismatch prints `this report was made with <sha>; check out that
    commit and build --release for an exact replay` and goes on.
  - It plays the last segment through the `cli_play` replay path, with
    `play_checked(.., !ignore_data)`, stopping at `to_tick`.
  - It prints `matched` or the `Divergence`.

- [ ] **Step 1: Write failing tests:** `show` and `replay --headless` on the
  zip Task 4's probe made, kept as a fixture made by the test, not committed.
  - `show` exits 0, and its output contains the note.
  - `replay` exits 0 with `matched`.
  - Mutating `data.txt`, or the level digest, makes `replay` fail unless
    `--ignore-data` is given.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run them. Expected: PASS.
- [ ] **Step 5:** Write `docs/BUG-REPORTS.md`:
  - **for players:** F12, what's in the zip, where it is per OS, and that
    nothing is uploaded;
  - **for developers:** `kjk report show`, `replay`, `--ignore-data`, and
    checking out the report's commit.
- [ ] **Step 6:** Commit.

### Task 6: Cross-OS determinism in releases

**Files:** Modify `.github/workflows/release.yml` (job `package`).

- [ ] **Step 1:** Add a step before `cargo xtask dist` in every matrix entry:
  `cargo test --release -p approved_tests --test approved sim_determinism`.
- [ ] **Step 2:** Check the YAML with
  `GH_CONFIG_DIR=~/.config/gh-kjk gh workflow view release.yml`. It must
  parse. Don't start a release.
- [ ] **Step 3:** Commit.

### Finish

- [ ] Update the spec's "checksum" lines to blake3, and note the 60-tick chunk
  hashes.
- [ ] Open a PR with the bump-file rules from `bumps/README.md` (none expected).
- [ ] In the PR description, list the probes run and their results, and Task
  0's outcome.
