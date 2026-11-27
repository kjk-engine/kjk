#!/usr/bin/env python3
"""Capture the 8-scene visual baseline under Classic, Enhanced, and Baked modes.

Uses `kjk play --stage <module> --walk --headless` with isolated scratch
settings and saves so the user's own config/saves are never touched.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCENES_FILE = Path(__file__).resolve().with_name("scenes.json")
RUN_FOLDER_RE = re.compile(r"^run folder:\s*(.+)$", re.MULTILINE)

VALID_MODES = ("classic", "enhanced", "baked")


def find_exe(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).expanduser().resolve()
    release = REPO_ROOT / "target" / "release" / "kjk"
    if release.exists():
        return release
    debug = REPO_ROOT / "target" / "debug" / "kjk"
    if debug.exists():
        return debug
    return release


def load_scenes(scenes_filter: str | None, game_filter: str | None) -> list[dict]:
    data = json.loads(SCENES_FILE.read_text(encoding="utf-8"))
    scenes = data["scenes"]
    if game_filter:
        scenes = [s for s in scenes if s["game"] == game_filter]
    if scenes_filter:
        wanted = {name.strip() for name in scenes_filter.split(",") if name.strip()}
        known = {s["id"] for s in scenes}
        unknown = wanted - known
        if unknown:
            raise SystemExit(f"unknown scenes: {', '.join(sorted(unknown))}")
        scenes = [s for s in scenes if s["id"] in wanted]
    return scenes


def write_settings(path: Path, mode: str, msaa: str | None, shadows: str) -> None:
    lighting = {
        "classic": "classic",
        "enhanced": "enhanced",
        "baked": "baked",
    }[mode]
    effective_msaa = msaa if msaa else ("4" if mode == "classic" else "taa")
    payload = {
        "display.mode": "window",
        "display.size": "1280x720",
        "display.vsync": "false",
        "display.lighting": lighting,
        "display.msaa": effective_msaa,
        "display.shadows": shadows,
        "game.autosave": "false",
        "feedback.tutorials": "false",
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def build_commands(scene: dict, wait_frames: int, pose_wait_frames: int, gpu_times: bool) -> str:
    cmds = ["wait world", f"wait {wait_frames}f"]
    pose = scene.get("camera")
    if pose:
        if len(pose) == 6:
            x, y, z, yaw, pitch, fov = pose
            cmds.append(f"camera {x} {y} {z} {yaw} {pitch} {fov}")
        elif len(pose) == 5:
            x, y, z, yaw, pitch = pose
            cmds.append(f"camera {x} {y} {z} {yaw} {pitch}")
        else:
            raise ValueError(f"{scene['id']}: camera pose must have 5 or 6 numbers")
        cmds.append(f"wait {pose_wait_frames}f")
    if gpu_times:
        cmds.append(f"dump {scene['id']}")
    cmds.extend([f"screenshot {scene['id']}", "quit"])
    return "; ".join(cmds)


def extract_gpu_summary(run_dir: Path, scene_id: str) -> dict | None:
    dump_path = run_dir / f"{scene_id}.json"
    if not dump_path.exists():
        return None
    try:
        dump = json.loads(dump_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    facts = dump.get("facts") or {}
    return {
        "frame": dump.get("frame"),
        "elapsed_ms": dump.get("elapsed_ms"),
        "gpu": facts.get("gpu"),
        "vitals": facts.get("vitals"),
        "render_adapter": facts.get("render_adapter"),
        "anti_aliasing": facts.get("anti_aliasing"),
        "lighting": facts.get("lighting"),
    }


def make_contact_sheet(out_dir: Path, modes: list[str], scenes: list[dict]) -> None:
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        print("note: Pillow is not installed; skipping contact sheet generation (--sheet).")
        return

    thumb_w, thumb_h = 640, 360
    cols = len(modes)
    rows = len(scenes)
    if cols == 0 or rows == 0:
        return
    sheet = Image.new("RGB", (thumb_w * cols, thumb_h * rows), (16, 16, 16))
    draw = ImageDraw.Draw(sheet)
    for r, scene in enumerate(scenes):
        for c, mode in enumerate(modes):
            img_path = out_dir / mode / f"{scene['id']}.png"
            x0, y0 = c * thumb_w, r * thumb_h
            if img_path.exists():
                with Image.open(img_path) as im:
                    tile = im.convert("RGB").resize((thumb_w, thumb_h), Image.Resampling.LANCZOS)
                sheet.paste(tile, (x0, y0))
            draw.text((x0 + 10, y0 + 8), f"{scene['id']} [{mode}]", fill=(255, 230, 80))
    sheet_path = out_dir / "sheet.png"
    sheet.save(sheet_path)
    print(f"wrote contact sheet: {sheet_path}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", default="baseline", help="Output folder name under target/visual-baseline/")
    parser.add_argument("--modes", default="classic,enhanced", help="Comma-separated modes: classic,enhanced,baked")
    parser.add_argument("--scenes", default=None, help="Comma-separated scene ids (default: all)")
    parser.add_argument("--game", choices=("k1", "tsl"), default=None, help="Filter scenes by game")
    parser.add_argument("--size", default="1280x720", help="Headless capture size WxH")
    parser.add_argument("--msaa", default=None, help="Override display.msaa (default: 4 for classic, taa otherwise)")
    parser.add_argument("--shadows", default="high", help="display.shadows setting (off, low, high, ultra)")
    parser.add_argument("--exe", default=None, help="Path to kjk binary")
    parser.add_argument("--wait-frames", type=int, default=60, help="Frames to wait after world loads")
    parser.add_argument("--pose-wait-frames", type=int, default=14, help="Frames to wait after setting camera pose")
    parser.add_argument("--gpu-times", action="store_true", help="Enable KJK_GPU_TIMES=1 and capture dump stats")
    parser.add_argument("--timeout", type=int, default=180, help="Per-scene timeout in seconds")
    parser.add_argument("--sheet", action="store_true", help="Build a side-by-side contact sheet (requires Pillow)")
    parser.add_argument("--dry-run", action="store_true", help="Print planned commands without launching kjk")
    args = parser.parse_args()

    modes = [m.strip() for m in args.modes.split(",") if m.strip()]
    bad_modes = [m for m in modes if m not in VALID_MODES]
    if bad_modes:
        raise SystemExit(f"invalid modes: {', '.join(bad_modes)} (valid: {', '.join(VALID_MODES)})")

    scenes = load_scenes(args.scenes, args.game)
    exe = find_exe(args.exe)
    out_dir = REPO_ROOT / "target" / "visual-baseline" / args.label
    scratch_dir = out_dir / "_scratch"
    saves_dir = scratch_dir / "saves"
    if not args.dry_run:
        if not exe.exists():
            raise SystemExit(f"kjk binary not found at {exe}; build it first (`cargo build --release -p launcher`) or pass --exe")
        saves_dir.mkdir(parents=True, exist_ok=True)

    results: list[dict] = []
    any_failed = False

    for mode in modes:
        settings_path = scratch_dir / f"settings-{mode}.json"
        if not args.dry_run:
            write_settings(settings_path, mode, args.msaa, args.shadows)
            (out_dir / mode).mkdir(parents=True, exist_ok=True)

        for scene in scenes:
            cmds = build_commands(scene, args.wait_frames, args.pose_wait_frames, args.gpu_times)
            cmd = [
                str(exe),
                "play",
                "--stage",
                scene["module"],
                "--walk",
                "--headless",
                "--size",
                args.size,
                "--cmds",
                cmds,
            ]
            env = os.environ.copy()
            env["KJK_SETTINGS"] = str(settings_path)
            env["KJK_SAVES"] = str(saves_dir)
            env["KJK_MUTE"] = "1"
            env["KJK_MOVIES"] = "0"
            env["KJK_UNCAPPED"] = "1"
            if args.gpu_times:
                env["KJK_GPU_TIMES"] = "1"
            if mode in ("classic", "enhanced"):
                env["KJK_FITTED_LIGHTS"] = "off"
            elif "KJK_FITTED_LIGHTS" not in env:
                if env.get("KJK_DETAIL_BAKE") or env.get("KJK_DETAIL_BAKES"):
                    env["KJK_FITTED_LIGHTS"] = "baked"

            if args.dry_run:
                print(f"[{mode}] {scene['id']} ({scene['game']}:{scene['module']}): {' '.join(cmd)}")
                continue

            print(f"[{mode}] capturing {scene['id']} ({scene['module']})...", flush=True)
            proc = subprocess.run(
                cmd,
                cwd=REPO_ROOT,
                env=env,
                capture_output=True,
                text=True,
                timeout=args.timeout,
                check=False,
            )
            combined = proc.stdout + "\n" + proc.stderr
            match = RUN_FOLDER_RE.search(combined)
            run_dir = Path(match.group(1).strip()) if match else None
            log_out = out_dir / mode / f"{scene['id']}.log"
            log_out.write_text(combined, encoding="utf-8")

            entry = {
                "mode": mode,
                "scene": scene["id"],
                "game": scene["game"],
                "module": scene["module"],
                "exit_code": proc.returncode,
                "run_folder": str(run_dir) if run_dir else None,
                "screenshot": None,
                "stats": None,
            }
            if proc.returncode == 0 and run_dir:
                src_png = run_dir / f"{scene['id']}.png"
                if src_png.exists():
                    dst_png = out_dir / mode / f"{scene['id']}.png"
                    shutil.copy2(src_png, dst_png)
                    entry["screenshot"] = str(dst_png.relative_to(REPO_ROOT))
                else:
                    any_failed = True
                    entry["error"] = f"missing screenshot {src_png}"
                if args.gpu_times:
                    entry["stats"] = extract_gpu_summary(run_dir, scene["id"])
            else:
                any_failed = True
                entry["error"] = f"exit code {proc.returncode}"
            results.append(entry)

    if args.dry_run:
        return 0

    summary_path = out_dir / "summary.json"
    summary_path.write_text(
        json.dumps(
            {
                "label": args.label,
                "size": args.size,
                "shadows": args.shadows,
                "modes": modes,
                "results": results,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote summary: {summary_path}")

    if args.sheet:
        make_contact_sheet(out_dir, modes, scenes)

    return 1 if any_failed else 0


if __name__ == "__main__":
    sys.exit(main())
