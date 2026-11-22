#!/usr/bin/env python3
"""Plays the Tomb of Vessk Tarn from the valley to its reward, with no person at the controls.

This is a benchmark of the level editor (docs/LEVEL-EDIT.md): it shows that
a level built in the editor can be played through. It builds the tomb
(tomb-replay), installs the exported mod into a stand-in for the game's
folder, and has `kjk playthrough` walk tomb.route: into the tomb from the
Valley of the Dark Lords, the datapad, the carvings, the terminal's quiz,
the guardian and the reward. It passes when:

- every goal of the route is met,
- the tomb's quest is closed (its entry 40),
- the party carries the reward and the datapad.

It needs KOTOR 1 installed where kjk finds it, and a built editor and kjk:

    cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
    python tools/level-editor/tomb-speedrun/run.py

It never writes into the game's own folder. The stand-in holds a hard link
to each of the game's files, and only its override folder and the new
module are the mod's.

    --mod DIR   play this exported mod instead of building the tomb first
    --keep      keep the run's folder, and say where it is
"""

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, os.path.join(HERE, "..", "tomb-replay"))
sys.path.insert(0, os.path.join(HERE, "..", "three-maps"))
import build  # noqa: E402
import replay  # noqa: E402

START = "korr_m36aa"
QUEST, CLOSED = "kje_vessk", 40
CARRIED = ("kje_vt_shroud", "kje_vt_datapad")


def journals(node):
    """Every journal the run's facts hold: each a list of [quest, entry]."""
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "journal" and isinstance(value, list):
                yield value
            else:
                yield from journals(value)
    elif isinstance(node, list):
        for value in node:
            yield from journals(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--mod")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    suffix = ".exe" if os.name == "nt" else ""
    kjk = os.path.join(REPO, "target", "release", "kjk" + suffix)
    run = tempfile.mkdtemp(prefix="kjk-tomb-speedrun-")
    began = time.time()
    failed = []

    def check(name, passed, shown):
        print("%s %s: %s" % ("PASS" if passed else "FAIL", name, str(shown)[:400]))
        if not passed:
            failed.append(name)

    try:
        mod = args.mod
        if not mod:
            mod = os.path.join(run, "tomb")
            done = subprocess.run(
                [sys.executable, os.path.join(HERE, "..", "tomb-replay", "replay.py"), "--export-to", mod],
                capture_output=True, text=True, errors="replace",
            )
            built = done.returncode == 0 and os.path.isdir(os.path.join(mod, "override"))
            check("the tomb builds", built, done.stdout.strip().splitlines()[-1:])
            if not built:
                raise SystemExit(done.stdout[-2000:])

        real = build.game_folder(kjk)
        before = sum(len(files) for _, _, files in os.walk(real))
        game = os.path.join(run, "k1")
        build.stand_in(real, game, mod)
        home = os.path.join(run, "play-home")
        os.makedirs(home)
        settings = os.path.join(replay.kjk_folder(), "settings.json")
        if os.path.isfile(settings):
            shutil.copy(settings, home)
        env = dict(os.environ, KJK_KOTOR1=game, KJK_HOME=home)
        done = subprocess.run(
            [kjk, "playthrough", "k1", START, os.path.join(HERE, "tomb.route"),
             "--god", "--mighty", "--class", "jedi", "--goal-seconds", "150"],
            env=env, capture_output=True, text=True, errors="replace", cwd=run, timeout=1800,
        )
        lines = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in (done.stdout + done.stderr).splitlines()]
        with open(os.path.join(run, "walk.log"), "w", encoding="utf-8") as file:
            file.write("\n".join(lines))
        blocked = [line.strip()[:160] for line in lines if "BLOCKED" in line]
        summary = [line.strip()[:160] for line in lines if line.startswith("playthrough:")]
        check("every goal of the route is met", done.returncode == 0 and not blocked, blocked or summary)

        found = sorted(glob.glob(os.path.join(run, "kjk-artifacts", "runs", "*", "facts.json")))
        facts = {}
        if found:
            with open(found[-1], encoding="utf-8") as file:
                facts = json.load(file)
        entries = [entry for journal in journals(facts) for entry in journal if entry and entry[0] == QUEST]
        check("the tomb's quest is closed", [QUEST, CLOSED] in entries, entries)
        carried = (facts.get("party") or {}).get("carried", [])
        has = [name for name in CARRIED if any(item.startswith(name + " ") for item in carried)]
        check("the party carries the reward and the datapad", len(has) == len(CARRIED), carried)
        after = sum(len(files) for _, _, files in os.walk(real))
        check("the game's own folder is untouched", before == after, (before, after))
    finally:
        if args.keep:
            print("the run's folder is kept: %s" % run)
        else:
            shutil.rmtree(run, ignore_errors=True)

    print("%.0f s in all" % (time.time() - began))
    print("TOMB SPEEDRUN PASSES" if not failed else "TOMB SPEEDRUN FAILS: " + "; ".join(failed))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
