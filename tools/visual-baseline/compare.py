#!/usr/bin/env python3
"""Compare two visual-baseline capture labels using `kjk compare-pictures`."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LARGEST_RE = re.compile(r"largest difference:\s*(\d+)\s+of 255", re.MULTILINE)
MEAN_RE = re.compile(r"mean difference:\s*([0-9.]+)\s+of 255", re.MULTILINE)
CHANGED_RE = re.compile(r"pixels differing by more than \d+:\s*(\d+)\s+of\s+(\d+)\s+\(([0-9.]+)%\)", re.MULTILINE)


def resolve_dir(label_or_path: str) -> Path:
    candidate = Path(label_or_path).expanduser()
    if candidate.is_dir():
        return candidate.resolve()
    return (REPO_ROOT / "target" / "visual-baseline" / label_or_path).resolve()


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


def parse_compare_output(text: str) -> dict:
    largest = LARGEST_RE.search(text)
    mean = MEAN_RE.search(text)
    changed = CHANGED_RE.search(text)
    return {
        "largest": int(largest.group(1)) if largest else None,
        "mean": float(mean.group(1)) if mean else None,
        "changed_pixels": int(changed.group(1)) if changed else None,
        "total_pixels": int(changed.group(2)) if changed else None,
        "changed_pct": float(changed.group(3)) if changed else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline_a", help="First label under target/visual-baseline/ or directory path")
    parser.add_argument("baseline_b", help="Second label under target/visual-baseline/ or directory path")
    parser.add_argument("--modes", default=None, help="Comma-separated modes to compare (default: common modes)")
    parser.add_argument("--up-to", type=int, default=255, help="Compare channels at or below this value (0..255)")
    parser.add_argument("--exe", default=None, help="Path to kjk binary")
    parser.add_argument("--out", default=None, help="Folder for diff PNGs and comparison report")
    parser.add_argument("--dry-run", action="store_true", help="Print planned compare-pictures calls without running kjk")
    args = parser.parse_args()

    dir_a = resolve_dir(args.baseline_a)
    dir_b = resolve_dir(args.baseline_b)
    exe = find_exe(args.exe)

    if args.modes:
        modes = [m.strip() for m in args.modes.split(",") if m.strip()]
    elif dir_a.is_dir() and dir_b.is_dir():
        modes_a = {p.name for p in dir_a.iterdir() if p.is_dir() and not p.name.startswith("_")}
        modes_b = {p.name for p in dir_b.iterdir() if p.is_dir() and not p.name.startswith("_")}
        modes = sorted(modes_a & modes_b)
    else:
        modes = ["classic", "enhanced"]

    if args.dry_run:
        print(f"compare {dir_a} vs {dir_b} (modes={modes}, exe={exe})")
        return 0

    if not dir_a.is_dir() or not dir_b.is_dir():
        raise SystemExit(f"both baseline folders must exist: {dir_a}, {dir_b}")
    if not exe.exists():
        raise SystemExit(f"kjk binary not found at {exe}; build it first or pass --exe")

    out_dir = Path(args.out).resolve() if args.out else (
        REPO_ROOT / "target" / "visual-baseline" / f"diff-{dir_a.name}-vs-{dir_b.name}"
    )
    out_dir.mkdir(parents=True, exist_ok=True)

    rows: list[dict] = []
    for mode in modes:
        pngs_a = {p.stem: p for p in (dir_a / mode).glob("*.png")}
        pngs_b = {p.stem: p for p in (dir_b / mode).glob("*.png")}
        for scene in sorted(set(pngs_a) & set(pngs_b)):
            diff_png = out_dir / mode / f"{scene}-diff.png"
            diff_png.parent.mkdir(parents=True, exist_ok=True)
            cmd = [
                str(exe),
                "compare-pictures",
                str(pngs_a[scene]),
                str(pngs_b[scene]),
                "--up-to",
                str(args.up_to),
                "--out",
                str(diff_png),
            ]
            proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
            metrics = parse_compare_output(proc.stdout)
            metrics["mode"] = mode
            metrics["scene"] = scene
            metrics["exit_code"] = proc.returncode
            metrics["diff_png"] = str(diff_png)
            rows.append(metrics)

    print("| Mode | Scene | Mean / 255 | Max / 255 | Changed >2 (%) |")
    print("|---|---|---:|---:|---:|")
    for row in rows:
        mean = f"{row['mean']:.3f}" if row["mean"] is not None else "n/a"
        largest = str(row["largest"]) if row["largest"] is not None else "n/a"
        pct = f"{row['changed_pct']:.3f}%" if row["changed_pct"] is not None else "n/a"
        print(f"| {row['mode']} | {row['scene']} | {mean} | {largest} | {pct} |")

    (out_dir / "compare.json").write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
