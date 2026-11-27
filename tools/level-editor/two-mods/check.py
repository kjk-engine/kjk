#!/usr/bin/env python3
"""Installs two mods made with kjk's level editor side by side and plays both.

This is a benchmark of the level editor and of how kjk reads its mods
(docs/LEVEL-EDIT.md, "Sharing a level"). The two mods are the Tomb of Vessk
Tarn (tomb-replay) and the Three Halls (three-maps). Each brings a quest
and an item of its own. The benchmark builds both, installs them into a
stand-in for the game's folder, and passes when:

- the two mods bring no file of the same name,
- kjk's journal has both mods' quests beside the game's own,
- kjk names no clash at its start,
- both mods play: the tomb's quiz moves the tomb's quest on, and kjk walks
  the Three Halls,
- kjk says so when a file of a mod is replaced,
- after the tomb is removed by the list of files it brought, the Three
  Halls still plays, and only its quest is left.

It needs KOTOR 1 installed where kjk finds it, and a built editor and kjk:

    cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
    python tools/level-editor/two-mods/check.py

It never writes into the game's own folder. The stand-in holds a hard link
to each of the game's files, and only its override folder and the new
modules are the mods'.

    --keep    keep the run's folder, and say where it is
"""

import argparse
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

TOMB, TRIO = "vessk_bench", "kje_trio"


def install(mod, game):
    """Copies a mod into a game folder. The files that were there already."""
    over = []
    for part in ("override", "modules"):
        source = os.path.join(mod, part)
        for folder, _, files in os.walk(source):
            inside = os.path.relpath(folder, source)
            target = os.path.join(game, part) if inside == "." else os.path.join(game, part, inside)
            os.makedirs(target, exist_ok=True)
            for name in files:
                if os.path.exists(os.path.join(target, name)):
                    over.append(os.path.join(part, inside, name))
                shutil.copy(os.path.join(folder, name), os.path.join(target, name))
    return over


def manifest_files(game, name):
    """The files a mod's own list names, and the list's path."""
    path = os.path.join(game, "override", "kjk-mod-%s.toml" % name)
    with open(path, encoding="utf-8") as file:
        return re.findall(r'^path = "([^"]+)"', file.read(), re.M), path


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    suffix = ".exe" if os.name == "nt" else ""
    kjk = os.path.join(REPO, "target", "release", "kjk" + suffix)
    run = tempfile.mkdtemp(prefix="kjk-two-mods-")
    began = time.time()
    failed = []

    def check(name, passed, shown):
        print("%s %s: %s" % ("PASS" if passed else "FAIL", name, str(shown)[:400]))
        if not passed:
            failed.append(name)

    try:
        # The two mods, each built by its own benchmark.
        mods = {TOMB: os.path.join(run, "tomb"), TRIO: os.path.join(run, "trio")}
        scripts = (
            (os.path.join(HERE, "..", "tomb-replay", "replay.py"), TOMB),
            (os.path.join(HERE, "..", "three-maps", "build.py"), TRIO),
        )
        for script, name in scripts:
            done = subprocess.run([sys.executable, script, "--export-to", mods[name]],
                                  capture_output=True, text=True, errors="replace")
            built = done.returncode == 0 and os.path.isdir(os.path.join(mods[name], "override"))
            check("the mod %s builds" % name, built, done.stdout.strip().splitlines()[-1:])
            if not built:
                raise SystemExit(done.stdout[-2000:])

        real = build.game_folder(kjk)
        before = sum(len(files) for _, _, files in os.walk(real))
        game = os.path.join(run, "k1")
        empty = os.path.join(run, "no-mod")
        os.makedirs(os.path.join(empty, "override"))
        os.makedirs(os.path.join(empty, "modules"))
        build.stand_in(real, game, empty)
        home = os.path.join(run, "play-home")
        os.makedirs(home)
        settings = os.path.join(replay.kjk_folder(), "settings.json")
        if os.path.isfile(settings):
            shutil.copy(settings, home)
        env = dict(os.environ, KJK_KOTOR1=game, KJK_HOME=home)

        def play(*words):
            done = subprocess.run([kjk] + list(words), env=env, capture_output=True, text=True,
                                  errors="replace", cwd=run, timeout=1500)
            lines = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in (done.stdout + done.stderr).splitlines()]
            return done.returncode, lines

        def quests():
            """How many quests kjk's journal has, and what kjk says of the mods."""
            _, lines = play("script", "journal", "k1", "end_m01aa")
            found = [re.search(r"global\.jrl: (\d+) quests", line) for line in lines]
            count = [int(match.group(1)) for match in found if match]
            said = [line.strip() for line in lines if "kjk mods in" in line]
            return (count or [None])[0], said

        def clashes(said):
            marks = ("both bring", "missing", "not the ones it brought", "two mods bring a quest")
            # A mod built on a machine that has another mod installed (a
            # community patch) lists that mod's files as what it was built
            # on. The stand-in has no other mod, so kjk says those files are
            # missing. That is true of the stand-in, and no clash of the two
            # mods.
            said = [line for line in said if "was built on another mod" not in line]
            return [line for line in said if any(mark in line for mark in marks)]

        def walk():
            code, lines = play("playthrough", "k1", build.GAME_LEVEL, build.ROUTE, "--god", "--goal-seconds", "120")
            return code == 0, [line.strip()[:150] for line in lines if line.startswith("playthrough:")]

        own, _ = quests()
        check("the game's own journal reads", own is not None and own > 50, own)

        # Both installed.
        over = install(mods[TOMB], game) + install(mods[TRIO], game)
        check("the two mods bring no file of the same name", not over, over[:8])
        both, said = quests()
        check("the journal has both mods' quests beside the game's own", both == own + 2, (own, both))
        named = [line for line in said if TOMB in line or TRIO in line]
        check("kjk names both mods and no clash", len(named) >= 2 and not clashes(said), clashes(said) or said[-2:])
        code, lines = play("try-conversation", "k1", TOMB, "kje_vt_c_term", "--on", "kje_vt_term",
                           "--seconds", "40", "--picks", "2,3,1")
        moved = [line.strip()[:120] for line in lines if "kje_vessk at 30" in line]
        check("the tomb plays: its quiz moves its quest on", code == 0 and bool(moved), moved or lines[-3:])
        walked, shown = walk()
        check("the Three Halls plays: kjk walks it", walked, shown)

        # A replaced file is named.
        seal = os.path.join(game, "override", "kje_trio_seal.uti")
        kept = seal + ".kept"
        shutil.copy(seal, kept)
        with open(seal, "ab") as file:
            file.write(b"\0\0\0\0")
        _, said = quests()
        named = [line for line in said if "kje_trio_seal.uti" in line and "not the ones it brought" in line]
        check("kjk says when a mod's file was replaced", bool(named), named[:1] or said[-3:])
        os.replace(kept, seal)

        # The tomb removed by its own list.
        files, listing = manifest_files(game, TOMB)
        for path in files:
            os.remove(os.path.join(game, *path.split("/")))
        os.remove(listing)
        left, said = quests()
        check("with the tomb removed, only the other quest is left", left == own + 1, (own, left))
        check("kjk names no clash after the removal", not clashes(said), clashes(said))
        walked, shown = walk()
        check("the Three Halls still plays", walked, shown)
        after = sum(len(files) for _, _, files in os.walk(real))
        check("the game's own folder is untouched", before == after, (before, after))
    finally:
        if args.keep:
            print("the run's folder is kept: %s" % run)
        else:
            shutil.rmtree(run, ignore_errors=True)

    print("%.0f s in all" % (time.time() - began))
    print("TWO MODS PASSES" if not failed else "TWO MODS FAILS: " + "; ".join(failed))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
