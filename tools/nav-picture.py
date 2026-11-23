"""Draws a navigation mesh as seen from above, each island in a colour.

    python tools/nav-picture.py MESH_JSON OUT_PNG [--size 2000] [--box X0,Y0,X1,Y1]
        [--below Z] [--above Z]

MESH_JSON is the `mesh.json` that `kjk collision ja LEVEL` writes beside its
`facts.json`: each face's three corners (metres) and its island, and each
place (a start or a waypoint) where its soles land and its island.

The island that holds the most places is drawn grey, every other island in
a colour of its own, and the faces of islands without places dark. Higher
faces are drawn over lower ones. A place is a dot in its island's colour
with a black rim. --box draws only that part of the level (metres), and
--below and --above leave out faces lower or higher than that height.

Plain Python, with no package beyond the standard library: the picture is
written as a PNG by hand.
"""

import argparse
import json
import struct
import zlib


def colour(island, main, holding):
    if island == main:
        return (150, 150, 150)
    if island not in holding:
        return (55, 55, 60)
    hue = (island * 0.618033988749895) % 1.0
    h = hue * 6.0
    k = lambda n: (n + h) % 6.0
    part = lambda n: int(255 * (1.0 - 0.75 * max(0.0, min(1.0, min(k(n), 4.0 - k(n))))))
    return (part(5.0), part(3.0), part(1.0))


def write_png(path, width, height, pixels):
    rows = b"".join(b"\x00" + bytes(pixels[y * width * 3:(y + 1) * width * 3]) for y in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    with open(path, "wb") as out:
        out.write(b"\x89PNG\r\n\x1a\n")
        out.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        out.write(chunk(b"IDAT", zlib.compress(rows, 6)))
        out.write(chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mesh")
    parser.add_argument("out")
    parser.add_argument("--size", type=int, default=2000)
    parser.add_argument("--box")
    parser.add_argument("--below", type=float)
    parser.add_argument("--above", type=float)
    args = parser.parse_args()
    with open(args.mesh, encoding="utf-8") as source:
        mesh = json.load(source)
    faces = mesh["faces"]
    places = mesh["places"]
    if args.below is not None or args.above is not None:
        low = args.above if args.above is not None else float("-inf")
        high = args.below if args.below is not None else float("inf")
        faces = [f for f in faces if low <= max(c[2] for c in f[0]) and min(c[2] for c in f[0]) <= high]
    counts = {}
    for _, island in places:
        counts[island] = counts.get(island, 0) + 1
    main_island = max(counts, key=lambda island: counts[island]) if counts else None
    if args.box:
        x0, y0, x1, y1 = [float(v) for v in args.box.split(",")]
    else:
        xs = [c[0] for f in faces for c in f[0]]
        ys = [c[1] for f in faces for c in f[0]]
        x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    span = max(x1 - x0, y1 - y0) or 1.0
    width = max(1, int(args.size * (x1 - x0) / span))
    height = max(1, int(args.size * (y1 - y0) / span))
    scale = (width - 1) / max(x1 - x0, 1e-6)
    pixels = bytearray(b"\x10" * (width * height * 3))

    def to_pixel(point):
        return ((point[0] - x0) * scale, (y1 - point[1]) * scale)

    for corners, island in sorted(faces, key=lambda f: sum(c[2] for c in f[0])):
        rgb = colour(island, main_island, counts)
        a, b, c = [to_pixel(p) for p in corners]
        lo_y = max(0, int(min(a[1], b[1], c[1])))
        hi_y = min(height - 1, int(max(a[1], b[1], c[1])) + 1)
        for y in range(lo_y, hi_y + 1):
            fy = y + 0.5
            xs = []
            for p, q in ((a, b), (b, c), (c, a)):
                if (p[1] <= fy < q[1]) or (q[1] <= fy < p[1]):
                    xs.append(p[0] + (fy - p[1]) * (q[0] - p[0]) / (q[1] - p[1]))
            if len(xs) < 2:
                continue
            for x in range(max(0, int(min(xs))), min(width - 1, int(max(xs))) + 1):
                at = (y * width + x) * 3
                pixels[at:at + 3] = bytes(rgb)
    for at, island in places:
        px, py = to_pixel(at)
        rgb = colour(island, main_island, counts)
        for dy in range(-4, 5):
            for dx in range(-4, 5):
                distance = dx * dx + dy * dy
                x, y = int(px) + dx, int(py) + dy
                if distance > 16 or not (0 <= x < width and 0 <= y < height):
                    continue
                spot = (0, 0, 0) if distance > 6 else rgb
                pixels[(y * width + x) * 3:(y * width + x) * 3 + 3] = bytes(spot)
    write_png(args.out, width, height, pixels)
    print(f"{args.out}: {width} by {height} pixels, {len(faces)} faces, {len(places)} places")


if __name__ == "__main__":
    main()
