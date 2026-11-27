#!/usr/bin/env python3
"""Rebuilds the Tomb of Vessk Tarn in kjk's level editor and checks the result.

This is a benchmark of the level editor (docs/LEVEL-EDIT.md), not a test of
the game. It starts the editor on a new map, sends the commands of
commands.txt through the agent link (docs/LEVEL-EDIT-AGENT.md), and passes
when:

- every command is answered ok,
- the level's facts are the ones expected.json holds,
- a trial of the terminal's quiz with the right answers opens the seal,
- the level exports as a mod.

It needs KOTOR 1 installed where kjk finds it, and a built editor:

    cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
    python tools/level-editor/tomb-replay/replay.py

The editor runs with a kjk folder of its own, made for the run and removed
after it, so nothing of the user's projects or settings is touched. The
user's settings.json is copied into it, so the same installs are found.

    --editor PATH        the editor to run (default: target/release)
    --keep               keep the run's kjk folder, and say where it is
    --write-expected     write expected.json from this run, to be reviewed
    --export-to DIR      also copy the exported mod to this folder
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
MAP = "vessk_bench"
BASE = "korr_m38ab"
BATCH = 200


def kjk_folder():
    """kjk's own folder on this machine, for its settings.json."""
    if os.environ.get("KJK_HOME"):
        return os.environ["KJK_HOME"]
    if os.name == "nt":
        return os.path.join(os.environ.get("APPDATA", ""), "kjk")
    if sys.platform == "darwin":
        return os.path.expanduser("~/Library/Application Support/kjk")
    return os.path.expanduser("~/.config/kjk")


class Link:
    """The editor's agent link in one kjk folder."""

    def __init__(self, home):
        self.folder = os.path.join(home, "editor-link")
        self.live = os.path.join(self.folder, "live.json")

    def read(self):
        for _ in range(100):
            try:
                with open(self.live, encoding="utf-8") as file:
                    return json.load(file)
            except (OSError, ValueError):
                time.sleep(0.05)
        raise SystemExit("live.json does not read")

    def wait(self, seconds):
        began = time.time()
        while time.time() - began < seconds:
            if os.path.exists(self.live) and "answered" in self.read():
                time.sleep(1.5)
                return time.time() - began
            time.sleep(0.5)
        raise SystemExit("the editor did not start its agent link")

    def send(self, lines, seconds=60):
        """Sends one batch and returns each line's answer."""
        before = self.read().get("answered", 0)
        fresh = os.path.join(self.folder, "inbox.tmp")
        with open(fresh, "w", encoding="utf-8", newline="\n") as file:
            file.write("\n".join(lines) + "\n")
        os.replace(fresh, os.path.join(self.folder, "inbox.txt"))
        began = time.time()
        while time.time() - began < seconds:
            time.sleep(0.15)
            live = self.read()
            if live["answered"] >= before + len(lines):
                mine = [a for a in live["answers"] if before < a["n"] <= before + len(lines)]
                return [a["answer"] for a in mine]
        raise SystemExit("no answers to %r" % (lines,))

    def file(self, name):
        with open(os.path.join(self.folder, name), encoding="utf-8") as file:
            return json.load(file)


def batches(scripts):
    """commands.txt as batches: each paragraph, without its comments."""
    with open(os.path.join(HERE, "commands.txt"), encoding="utf-8") as file:
        text = file.read().replace("{scripts}", scripts)
    found = []
    for paragraph in text.split("\n\n"):
        lines = [line.strip() for line in paragraph.splitlines()]
        lines = [line for line in lines if line and not line.startswith("#")]
        if not lines:
            continue
        if len(lines) > BATCH:
            raise SystemExit("a batch of commands.txt has %d lines, more than %d" % (len(lines), BATCH))
        found.append(lines)
    return found


def facts_of(level):
    """What of the level's facts the benchmark compares."""
    lighting = level.get("lighting", {})
    nav = level.get("nav") or {}
    return {
        "counts": level.get("counts"),
        "boxes": len(level.get("boxes", [])),
        "lights_placed": len(lighting.get("placed", [])),
        "nav_faces": nav.get("faces"),
        "nav_islands": nav.get("islands"),
        "nav_faces_apart": nav.get("apart"),
        "entry_off_the_mesh": nav.get("entry_off"),
        "conversations": sorted(level.get("conversations", [])),
        "project_items": sorted(
            (item["name"], item["copied_from"], item["shown"], item["properties"])
            for item in level.get("project_items", [])
        ),
        "project_journal": level.get("project_journal"),
        "project_scripts": sorted(level.get("project_scripts", [])),
        "script_errors": level.get("script_errors"),
        "other_areas": sorted(level.get("other_areas", [])),
        "problems": level.get("problems"),
        "object_tags": sorted(
            thing["tag"] for thing in level.get("objects", []) if thing.get("tag", "").startswith("kje_vt_")
        ),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    suffix = ".exe" if os.name == "nt" else ""
    parser.add_argument("--editor", default=os.path.join(REPO, "target", "release", "kjk-level-editor" + suffix))
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--write-expected", action="store_true")
    parser.add_argument("--export-to")
    args = parser.parse_args()
    if not os.path.isfile(args.editor):
        raise SystemExit("%s is not there: build the editor first" % args.editor)

    home = tempfile.mkdtemp(prefix="kjk-tomb-replay-")
    settings = os.path.join(kjk_folder(), "settings.json")
    if os.path.isfile(settings):
        shutil.copy(settings, home)
    env = dict(os.environ, KJK_HOME=home, KJK_EDITOR_LINK="1")
    log = open(os.path.join(home, "editor.log"), "w")
    began = time.time()
    editor = subprocess.Popen(
        [args.editor, "k1", MAP, "--new-map", BASE, "--title", "Tomb of Vessk Tarn"],
        env=env, stdout=log, stderr=log, cwd=REPO,
    )
    link = Link(home)
    failed = []

    def check(name, passed, shown):
        print("%s %s: %s" % ("PASS" if passed else "FAIL", name, str(shown)[:300]))
        if not passed:
            failed.append(name)

    try:
        started = link.wait(180)
        print("the editor started in %.0f s" % started)

        # The commands.
        sent, refused = 0, []
        building = time.time()
        for lines in batches(os.path.join(HERE, "scripts")):
            answers = link.send(lines)
            sent += len(lines)
            for line, answer in zip(lines, answers):
                if not answer.startswith("ok"):
                    refused.append((line, answer))
        built = time.time() - building
        for line, answer in refused[:12]:
            print("   refused: %s -> %s" % (line[:90], answer[:160]))
        check("every command is answered ok", not refused, "%d commands in %.0f s, %d refused" % (sent, built, len(refused)))

        # The level's facts.
        time.sleep(4)  # the scene is drawn again a moment after the last edit
        link.send(["level"])
        facts = facts_of(link.file("level.json"))
        expected_path = os.path.join(HERE, "expected.json")
        if args.write_expected:
            with open(expected_path, "w", encoding="utf-8", newline="\n") as file:
                json.dump(facts, file, indent=2, sort_keys=True)
                file.write("\n")
            print("wrote %s: review it before keeping it" % expected_path)
        with open(expected_path, encoding="utf-8") as file:
            expected = json.load(file)
        # JSON has no tuples: compare as it reads back.
        facts = json.loads(json.dumps(facts))
        differing = sorted(key for key in expected if facts.get(key) != expected[key])
        for key in differing:
            print("   %s: expected %s, found %s" % (key, json.dumps(expected[key])[:200], json.dumps(facts.get(key))[:200]))
        check("the level's facts are as expected", not differing, "%d of %d facts differ" % (len(differing), len(expected)))

        # What the link says of the level without a person looking: the
        # floor as places, a thing's details without selecting it, and the
        # quiz read back.
        answer = link.send(["floormap 2"])[0]
        floor = link.file("floor.json") if answer.startswith("ok") else {}
        check("the floor is listed as places, with the rooms' bounds",
              len(floor.get("places", [])) > 500 and len(floor.get("rooms", [])) == 17,
              (answer, len(floor.get("places", [])), len(floor.get("rooms", []))))
        before = link.read()["selected"]
        answer = link.send(["details kje_vt_seal"])[0]
        seal = link.file("details.json") if answer.startswith("ok") else {}
        check("a thing's details are read without selecting it",
              seal.get("tag") == "kje_vt_seal" and link.read()["selected"] == before, (answer, seal.get("tag")))
        answer = link.send(["conversation kje_vt_c_term show"])[0]
        quiz_read = link.file("conversation.json") if answer.startswith("ok") else {}
        shape = (len(quiz_read.get("lines", [])), len(quiz_read.get("replies", [])), quiz_read.get("screen"))
        check("the quiz reads back: 5 lines, 13 replies, on a computer's screen", shape == (5, 13, "computer"), shape)

        # The quiz, tried with the right answers and with a wrong one.
        def quiz(picks):
            link.send(["conversation try kje_vt_c_term kje_vt_term " + picks], seconds=120)
            time.sleep(1)
            waited = time.time()
            while time.time() - waited < 180:
                live = link.read()
                if live["trying"] is None and live["last_try"]:
                    return live["last_try"]
                time.sleep(0.3)
            raise SystemExit("the quiz's trial did not end")

        right = quiz("2 3 1")
        lines = " | ".join(right["lines"])
        check("the right answers open the seal", right["ok"] and "kje_vessk at 30" in lines and "seal is open" in lines,
              [line.strip()[:90] for line in right["lines"] if "journal" in line or "E3" in line])
        # The trial lists what the scripts called: the seal is unlocked.
        unlocked = [line.strip()[:90] for line in right["lines"] if "SetLocked" in line]
        check("the trial shows the script unlocking the seal", bool(unlocked), unlocked[:2])
        # A new map starts without its base level's scripts, which its own
        # module does not have.
        check("the trial names no missing script", "missing script" not in lines,
              [line.strip()[:90] for line in right["lines"] if "missing" in line])
        wrong = quiz("1")
        lines = " | ".join(wrong["lines"])
        check("a wrong answer does not", wrong["ok"] and "kje_vessk at 30" not in lines and "not what a Sith knows" in lines,
              [line.strip()[:90] for line in wrong["lines"] if "E4" in line])

        # The export.
        answer = link.send(["export"])[0]
        mod = os.path.join(home, "exports", MAP)
        there = [
            os.path.isfile(os.path.join(mod, "modules", MAP + ".mod")),
            os.path.isfile(os.path.join(mod, "override", "kjk-area-%s.toml" % MAP)),
            # The mod's own quests, in a file of its own name, and what the
            # mod says of itself. It brings no copy of the game's journal.
            os.path.isfile(os.path.join(mod, "override", "kjk-journal-%s.jrl" % MAP)),
            os.path.isfile(os.path.join(mod, "override", "kjk-mod-%s.toml" % MAP)),
            not os.path.exists(os.path.join(mod, "override", "global.jrl")),
            os.path.isfile(os.path.join(mod, "override", "kje_vt_shroud.uti")),
            os.path.isfile(os.path.join(mod, "override", "m36aa.git")),
            # A creature copied from another level of the game.
            os.path.isfile(os.path.join(mod, "override", "k37_enc_dire.utc")),
        ]
        check("the level exports as a mod", answer.startswith("ok") and all(there), (answer, there))
        if args.export_to:
            shutil.rmtree(args.export_to, ignore_errors=True)
            shutil.copytree(mod, args.export_to)

        live = link.read()
        check("nothing is left unsaved", live["unsaved"] is False, live["unsaved"])
        link.send(["ui quit"])
        try:
            editor.wait(20)
        except subprocess.TimeoutExpired:
            pass
    finally:
        if editor.poll() is None:
            editor.kill()
        log.close()
        if args.keep:
            print("the run's kjk folder is kept: %s" % home)
        else:
            shutil.rmtree(home, ignore_errors=True)

    print("%.0f s in all" % (time.time() - began))
    print("TOMB REPLAY PASSES" if not failed else "TOMB REPLAY FAILS: " + "; ".join(failed))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
