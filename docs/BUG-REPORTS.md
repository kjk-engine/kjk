# Bug reports

A bug report is a zip the game makes when you press a key. Attach it to an
issue, and we can replay what happened on our machine, tick for tick.

## For players

1. When something goes wrong, press **F12** (Report a bug; rebind it in the
   options' keys). The game takes a screenshot and asks what went wrong.
2. Write a line or two and press **OK**.
3. The game saves the zip, opens its folder and opens GitHub's bug form
   (`kjk-engine/kjk`, *Bug report*). Attach the zip there.

After a crash, the next main menu says kjk stopped unexpectedly. Press F12
there to report the run that crashed.

**Nothing is uploaded.** The zip stays on your disk until you attach it
yourself.

Where the zips go (the newest 20 are kept):

| OS | folder |
|---|---|
| Windows | `%APPDATA%\kjk\reports` |
| macOS | `~/Library/Application Support/kjk/reports` |
| Linux | `$XDG_CONFIG_HOME/kjk/reports` (`~/.config/kjk/reports`) |

What a zip holds:

| file | what |
|---|---|
| `note.txt` | what you wrote |
| `screenshot.png` | the frame when you pressed F12 |
| `system.txt` | kjk's release and commit, the OS, CPU, memory, the window mode |
| `manifest.json`, `kjk.log` | how the game was started, and its log |
| `crash.txt` | after a crash: the error, its backtrace, where you were |
| `breadcrumbs.txt` | where you were: tick, module, conversation, screen |
| `flight-<n>.kjkf` | the flight recorder: your inputs from the current module's start (and the one before) |
| `data.txt` | a digest of each recorded module's data, to tell another edition or a mod |
| `now.kjkv` | the game as it was, like a save: your characters' names are in it |
| `settings.json` | your settings, without the install's id |
| `dropped.txt` | when the zip would pass GitHub's 25 MB, which recordings were left out |

Every text has your home folder, user name and computer name taken out. No
game files go in.

## For developers

```bash
kjk report show kjk-report-20261028-181837.zip
```

`show` prints the report's build and whether it is this one, the system,
the note, the breadcrumbs, the crash's message and the top 15 lines of its
backtrace, the log's last 40 lines, each flight segment's module and ticks,
and, for each module recorded, whether its data is this install's or
**other data (edition or mod)**.

```bash
kjk report replay kjk-report-20261028-181837.zip --headless
```

`replay` unpacks the zip into the temp folder (`kjk-report-<time>/`),
replays the last flight segment in the sim and prints `matched` or where it
diverged (exit code 0 or 1). Without `--headless` it also prints the
`kjk sim view --replay` command that shows the segment in a window.

- `--to-tick N` stops at the last checkpoint (every 60 ticks) at or before
  tick N.
- `--ignore-data` replays even when the module's data is not the player's
  (another edition, a patch, an `Override` mod). It goes as far as it can
  and names the first tick that diverges.

A recording replays exactly only on the build that made it: the same commit,
built `--release`. When the commits differ, `replay` names the report's:

```bash
git checkout <commit>
cargo build --release
```

A commit ending in `-dirty` had uncommitted changes, so it can't be checked
out exactly.
