#!/usr/bin/env python3
"""Builds the Three Halls in kjk's level editor: three new maps in one project.

This is a benchmark of the level editor (docs/LEVEL-EDIT.md), not a test of
the game. The Three Halls is an adventure of three new maps, a hub and two
wings, reached from Dreshdae on Korriban. All four levels are in one
project. The benchmark passes when:

- every command is answered ok,
- the project holds the four levels, three of them new maps,
- the project exports as one mod, with a module for each map,
- with --play: kjk walks from Dreshdae into the hub, to each wing and
  back, and out to Dreshdae again, with no person at the controls.

It needs KOTOR 1 installed where kjk finds it, and a built editor and kjk:

    cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
    python tools/level-editor/three-maps/build.py --play

The editor runs with a kjk folder of its own, made for the run and removed
after it. The walk needs the mod in a game folder, and the benchmark never
writes into the real one: it makes a stand-in beside the run's kjk folder,
where each of the game's files is a hard link and only override and the
new modules are the mod's.

    --editor PATH    the editor to run (default: target/release)
    --kjk PATH       the kjk to play with (default: target/release)
    --play           also walk the adventure in kjk
    --keep           keep the run's folder, and say where it is
    --export-to DIR  also copy the exported mod to this folder
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
import replay  # noqa: E402  (the link's helper, shared with the tomb replay)

PROJECT = "kje_trio"
HUB = "kje_hub"
MAPS = [HUB, "kje_wing_a", "kje_wing_b"]
GAME_LEVEL = "korr_m33aa"
# The walk: into the hub, to each wing and back, and out again.
ROUTE = "exit kje_in; exit kje_to_a; exit kje_a_back; exit kje_to_b; exit kje_b_back; exit kje_hub_out"


def paragraphs():
    """commands.txt as batches and directives, in order."""
    with open(os.path.join(HERE, "commands.txt"), encoding="utf-8") as file:
        text = file.read()
    found = []
    for paragraph in text.split("\n\n"):
        lines = [line.strip() for line in paragraph.splitlines()]
        lines = [line for line in lines if line and not line.startswith("#")]
        if not lines:
            continue
        if lines[0].startswith("@"):
            if len(lines) != 1:
                raise SystemExit("a directive stands alone in its paragraph: %s" % lines[0])
            found.append(lines[0].split())
        else:
            found.append(lines)
    return found


def wait_level(link, module, seconds=180):
    """Waits until a restarted editor shows the level `module`."""
    began = time.time()
    while time.time() - began < seconds:
        try:
            fresh = time.time() - os.path.getmtime(link.live) < 3
            if fresh and link.read().get("module") == module:
                time.sleep(1.5)
                return
        except (OSError, SystemExit):
            pass
        time.sleep(0.4)
    raise SystemExit("the editor did not open the level %s" % module)


def stand_for(source, target):
    """Makes `target` stand for the game's file `source` without changing it.

    A hard link costs no space, but it only works inside one drive. A game
    on another drive than the run's folder gets a symbolic link, and a copy
    where the system allows neither.
    """
    for make in (os.link, os.symlink):
        try:
            make(source, target)
            return
        except OSError:
            pass
    shutil.copy2(source, target)


def put(source, target):
    """Copies the mod's file `source` to `target` in the stand-in.

    A file of that name in the stand-in is a link to the game's own file
    (`stand_for`). A copy over it would write through the link into the
    game's folder, so the link is taken away first.
    """
    if os.path.lexists(target):
        os.remove(target)
    shutil.copy(source, target)


def stop(link, editor):
    """Stops the editor this run started, and no other program.

    The editor starts itself again for each further map, so the one that
    runs at the end is not the process this script started. It is asked to
    quit through its link, which only this run's editor reads. The process
    this script started is ended if it still runs.
    """
    try:
        fresh = os.path.join(link.folder, "inbox.tmp")
        with open(fresh, "w", encoding="utf-8", newline="\n") as file:
            file.write("ui quit\n")
        os.replace(fresh, os.path.join(link.folder, "inbox.txt"))
    except OSError:
        pass
    try:
        editor.wait(10)
    except subprocess.TimeoutExpired:
        editor.kill()
    # The editor that took over reads its link a few times a second.
    time.sleep(3)


def stand_in(real, to, mod):
    """A stand-in for the game's folder with the mod installed in it."""
    for folder, dirs, files in os.walk(real):
        inside = os.path.relpath(folder, real)
        top = inside.split(os.sep)[0].lower()
        target = to if inside == "." else os.path.join(to, inside)
        os.makedirs(target, exist_ok=True)
        if top in ("override", "saves", "movies"):
            dirs[:] = []
            continue
        for name in files:
            stand_for(os.path.join(folder, name), os.path.join(target, name))
    for name in os.listdir(to):
        if name.lower() == "override":
            shutil.rmtree(os.path.join(to, name))
    shutil.copytree(os.path.join(mod, "override"), os.path.join(to, "override"))
    for name in os.listdir(os.path.join(mod, "modules")):
        put(os.path.join(mod, "modules", name), os.path.join(to, "modules", name))


def game_folder(kjk):
    """Where kjk finds KOTOR 1, as `kjk installs` says it."""
    done = subprocess.run([kjk, "games"], capture_output=True, text=True, errors="replace")
    for line in (done.stdout + done.stderr).splitlines():
        found = re.search(r"k1\b.*?([A-Za-z]:\\[^\r\n]*|/[^\r\n]*)$", line.strip())
        if found and os.path.isdir(found.group(1).strip()):
            return found.group(1).strip()
    raise SystemExit("kjk games names no folder for k1")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    suffix = ".exe" if os.name == "nt" else ""
    release = os.path.join(REPO, "target", "release")
    parser.add_argument("--editor", default=os.path.join(release, "kjk-level-editor" + suffix))
    parser.add_argument("--kjk", default=os.path.join(release, "kjk" + suffix))
    parser.add_argument("--play", action="store_true")
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--export-to")
    args = parser.parse_args()
    if not os.path.isfile(args.editor):
        raise SystemExit("%s is not there: build the editor first" % args.editor)

    run = tempfile.mkdtemp(prefix="kjk-three-maps-")
    home = os.path.join(run, "kjk")
    os.makedirs(home)
    settings = os.path.join(replay.kjk_folder(), "settings.json")
    if os.path.isfile(settings):
        shutil.copy(settings, home)
    env = dict(os.environ, KJK_HOME=home, KJK_EDITOR_LINK="1")
    log = open(os.path.join(run, "editor.log"), "w")
    began = time.time()
    editor = subprocess.Popen(
        [args.editor, "k1", HUB, "--project", PROJECT, "--new-map", "korr_m38ab", "--title", "The Three Halls"],
        env=env, stdout=log, stderr=log, cwd=REPO,
    )
    link = replay.Link(home)
    failed = []

    def check(name, passed, shown):
        print("%s %s: %s" % ("PASS" if passed else "FAIL", name, str(shown)[:400]))
        if not passed:
            failed.append(name)

    try:
        link.wait(180)
        sent, refused, restarts = 0, [], 0
        for step in paragraphs():
            if step[0] == "@settle":
                # The rooms' floor is built a moment after they are added.
                time.sleep(5)
                continue
            if step[0] in ("@new", "@level"):
                verb = "new" if step[0] == "@new" else "level"
                answers = link.send(["save", "ui file %s %s" % (verb, step[1])])
                sent += 2
                refused += [(line, answer) for line, answer in zip(("save", step[0]), answers) if not answer.startswith("ok")]
                wait_level(link, step[1])
                restarts += 1
                continue
            lines = step
            if any("{entry}" in line for line in lines):
                # A walkable place 3 metres from where the level's camera starts.
                camera = link.read()["camera"]["at"]
                at = "%.2f %.2f" % (camera[0] + 3.0, camera[1])
                floor = link.send(["floor " + at])[0]
                if not floor.startswith("ok"):
                    raise SystemExit("no floor near the level's start: %s" % floor)
                place = "%s %s" % (at, floor.split(":")[1].split()[0])
                lines = [line.replace("{entry}", place) for line in lines]
            answers = link.send(lines)
            sent += len(lines)
            refused += [(line, answer) for line, answer in zip(lines, answers) if not answer.startswith("ok")]
        for line, answer in refused[:12]:
            print("   refused: %s -> %s" % (line[:90], answer[:200]))
        check("every command is answered ok", not refused,
              "%d commands, %d restarts of the editor, %d refused" % (sent, restarts, len(refused)))

        # The project's levels.
        link.send(["save", "level"])
        level = link.file("level.json")
        levels = sorted(module for module, _ in level.get("levels_of_the_project", []))
        check("the project holds the four levels", levels == sorted(MAPS + [GAME_LEVEL]), levels)
        folder = os.path.join(home, "projects", PROJECT)
        maps = sorted(name[:-4] for name in os.listdir(os.path.join(folder, "modules")) if name.endswith(".mod"))
        check("three of them are new maps", maps == sorted(MAPS), maps)

        # The export.
        answer = link.send(["export"])[0]
        mod = os.path.join(home, "exports", PROJECT)
        there = [os.path.isfile(os.path.join(mod, "modules", name + ".mod")) for name in MAPS]
        there += [os.path.isfile(os.path.join(mod, "override", name)) for name in (
            "kjk-mod-%s.toml" % PROJECT, "kjk-journal-%s.jrl" % PROJECT, "kje_trio_seal.uti", "m33aa.git")]
        check("the project exports as one mod", answer.startswith("ok") and all(there), (answer[:60], there))
        check("the mod brings no copy of the game's journal",
              not os.path.exists(os.path.join(mod, "override", "global.jrl")), "")
        link.send(["ui quit"])
        time.sleep(2)
        if args.export_to:
            shutil.rmtree(args.export_to, ignore_errors=True)
            shutil.copytree(mod, args.export_to)

        # The walk.
        if args.play:
            real = game_folder(args.kjk)
            shadow = os.path.join(run, "k1")
            before = sum(len(files) for _, _, files in os.walk(real))
            stand_in(real, shadow, mod)
            play_env = dict(os.environ, KJK_KOTOR1=shadow, KJK_HOME=os.path.join(run, "play-home"))
            os.makedirs(play_env["KJK_HOME"])
            if os.path.isfile(settings):
                shutil.copy(settings, play_env["KJK_HOME"])
            done = subprocess.run(
                [args.kjk, "playthrough", "k1", GAME_LEVEL, ROUTE, "--god", "--goal-seconds", "120"],
                env=play_env, capture_output=True, text=True, errors="replace", cwd=run, timeout=1500,
            )
            out = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in (done.stdout + done.stderr).splitlines()]
            with open(os.path.join(run, "walk.log"), "w", encoding="utf-8") as file:
                file.write("\n".join(out))
            shown = [line.strip()[:160] for line in out if "goal" in line.lower() or "block" in line.lower() or "exit" in line.lower()]
            check("kjk walks hub, wing A, hub, wing B, hub and out", done.returncode == 0, shown[-8:])
            after = sum(len(files) for _, _, files in os.walk(real))
            check("the game's own folder is untouched", before == after, (before, after))
    finally:
        stop(link, editor)
        log.close()
        if args.keep:
            print("the run's folder is kept: %s" % run)
        else:
            time.sleep(1)
            shutil.rmtree(run, ignore_errors=True)

    print("%.0f s in all" % (time.time() - began))
    print("THREE MAPS PASSES" if not failed else "THREE MAPS FAILS: " + "; ".join(failed))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
