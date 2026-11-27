#!/usr/bin/env python3
"""Builds a big level in kjk's level editor and measures how the editor holds it.

This is a benchmark of the level editor (docs/LEVEL-EDIT.md). It makes a
new map of the 17 rooms of Naga Sadow's tomb and fills it through the agent
link with 1,000 objects, 200 lights and 50 conversations. It then reads the
editor's own timings (`timings` in the link's live.json) and passes when:

- every command is answered ok,
- the level holds what was asked for,
- the editor draws 60 frames a second while idle (a frame within 17.5 ms),
- a save takes under 2 seconds,
- the scene is drawn again after an edit in under 1 second.

It needs KOTOR 1 installed where kjk finds it, a built editor, and a
window: the frame rate of a run without a window says nothing.

    cargo build --release -p level_editor --bin kjk-level-editor
    python tools/level-editor/big-level/measure.py

The editor runs with a kjk folder of its own, made for the run and removed
after it.

    --editor PATH   the editor to run (default: target/release)
    --objects N     how many objects (default 1000)
    --lights N      how many lights (default 200)
    --talks N       how many conversations (default 50)
    --keep          keep the run's kjk folder, and say where it is
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, os.path.join(HERE, "..", "tomb-replay"))
import replay  # noqa: E402

MAP = "kje_big"
ROOMS = ["m39aa_%02d" % n for n in (1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19)]
PLACEABLES = ["plc_footlker", "plc_statue1", "plc_statue2", "plc_hmnskel", "plc_runepillar",
              "plc_runeobelisk", "plc_gencorpse", "plc_datapad"]
CREATURES = ["g_tukata01", "g_wardroid01"]
BATCH = 190
FRAME_MS, SAVE_MS, REDRAW_MS = 17.5, 2000.0, 1000.0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    suffix = ".exe" if os.name == "nt" else ""
    parser.add_argument("--editor", default=os.path.join(REPO, "target", "release", "kjk-level-editor" + suffix))
    parser.add_argument("--objects", type=int, default=1000)
    parser.add_argument("--lights", type=int, default=200)
    parser.add_argument("--talks", type=int, default=50)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    if not os.path.isfile(args.editor):
        raise SystemExit("%s is not there: build the editor first" % args.editor)

    home = tempfile.mkdtemp(prefix="kjk-big-level-")
    settings = os.path.join(replay.kjk_folder(), "settings.json")
    if os.path.isfile(settings):
        shutil.copy(settings, home)
    env = dict(os.environ, KJK_HOME=home, KJK_EDITOR_LINK="1")
    log = open(os.path.join(home, "editor.log"), "w")
    began = time.time()
    editor = subprocess.Popen(
        [args.editor, "k1", MAP, "--new-map", "korr_m38ab", "--title", "The big level"],
        env=env, stdout=log, stderr=log, cwd=REPO,
    )
    link = replay.Link(home)
    failed = []
    refused = []

    def check(name, passed, shown):
        print("%s %s: %s" % ("PASS" if passed else "FAIL", name, str(shown)[:300]))
        if not passed:
            failed.append(name)

    def send(lines, what):
        """Sends the lines as batches, each one undo step, and says how long they took."""
        started = time.time()
        for at in range(0, len(lines), BATCH):
            part = ["begin"] + lines[at:at + BATCH] + ["end"]
            answers = link.send(part, seconds=900)
            refused.extend((line, answer) for line, answer in zip(part, answers) if not answer.startswith("ok"))
        took = time.time() - started
        print("     %d commands for %s in %.1f s (%.0f ms each)" % (len(lines), what, took, 1000 * took / max(1, len(lines))))
        return took

    def settled(seconds=120):
        """Waits until the scene has been drawn again and stays so."""
        last, since = None, time.time()
        waited = time.time()
        while time.time() - waited < seconds:
            link.send(["level"])
            redraws = link.file("level.json").get("redraws")
            if redraws != last:
                last, since = redraws, time.time()
            elif time.time() - since > 4:
                return
            time.sleep(1)

    try:
        link.wait(180)
        send(["add room %s home" % room for room in ROOMS], "the rooms")
        time.sleep(6)
        answer = link.send(["floormap 1"])[0]
        places = link.file("floor.json")["places"] if answer.startswith("ok") else []
        if len(places) < 500:
            raise SystemExit("the floor has too few places: %s" % answer)
        # Spread over the floor, the same places every run.
        step = max(1, len(places) // (args.objects + 1))
        spots = [places[(n * step) % len(places)] for n in range(args.objects)]

        objects = []
        for n, (x, y, z) in enumerate(spots):
            if n % 20 == 19:
                objects.append("add waypoint sw_post001 %s %s %s" % (x, y, z))
            elif n % 10 == 9:
                objects.append("add creature %s %s %s %s" % (CREATURES[n % len(CREATURES)], x, y, z))
            else:
                objects.append("add placeable %s %s %s %s" % (PLACEABLES[n % len(PLACEABLES)], x, y, z))
        took_objects = send(objects, "the objects")

        light_step = max(1, len(places) // (args.lights + 1))
        lights = ["add light point %s %s %s" % (x, y, z + 3.0)
                  for x, y, z in (places[(n * light_step + 7) % len(places)] for n in range(args.lights))]
        send(lights, "the lights")

        talks = []
        for n in range(args.talks):
            name = "kje_big_c%02d" % n
            talks += [
                "conversation new %s" % name,
                'conversation %s say start "This is the first line of conversation %d."' % (name, n),
                'conversation %s replies line 0 "Tell me more." "Goodbye."' % name,
                'conversation %s say reply 0 "There is no more to tell."' % name,
            ]
        send(talks, "the conversations")
        link.send(["ui conversation"])
        for line, answer in refused[:8]:
            print("   refused: %s -> %s" % (line[:90], answer[:160]))
        check("every command is answered ok", not refused, "%d refused" % len(refused))

        settled()
        link.send(["level"])
        level = link.file("level.json")
        counts = level.get("counts", {})
        held = (sum(counts.values()), len(level.get("lighting", {}).get("placed", [])), len(level.get("conversations", [])))
        check("the level holds what was asked for", held == (args.objects, args.lights, args.talks), held)

        # A save.
        link.send(["save"], seconds=300)
        time.sleep(1)
        save_ms = link.read()["timings"]["save_ms"]
        check("a save takes under 2 seconds", 0 < save_ms < SAVE_MS, "%.0f ms" % save_ms)

        # An edit, and the scene drawn again.
        link.send(["select placeable 0", "move %s %s %s" % tuple(places[3])])
        settled()
        redraw_ms = link.read()["timings"]["redraw_ms"]
        check("the scene is drawn again in under 1 second", 0 < redraw_ms < REDRAW_MS, "%.0f ms" % redraw_ms)

        # Idle.
        link.send(["select none"])
        time.sleep(8)
        frames = [link.read()["timings"] for _ in range(4) if not time.sleep(1.5)]
        frame_ms = max(frame["frame_ms"] for frame in frames)
        worst = max(frame["frame_worst_ms"] for frame in frames)
        check("the editor draws 60 frames a second while idle", 0 < frame_ms <= FRAME_MS,
              "%.1f ms a frame (%.0f a second), the longest %.1f ms" % (frame_ms, 1000 / frame_ms if frame_ms else 0, worst))
        print("     adding an object took %.0f ms on average" % (1000 * took_objects / max(1, args.objects)))
        link.send(["ui quit"])
        time.sleep(1)
    finally:
        if editor.poll() is None:
            editor.kill()
        log.close()
        if args.keep:
            print("the run's kjk folder is kept: %s" % home)
        else:
            time.sleep(1)
            shutil.rmtree(home, ignore_errors=True)

    print("%.0f s in all" % (time.time() - began))
    print("BIG LEVEL PASSES" if not failed else "BIG LEVEL FAILS: " + "; ".join(failed))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
