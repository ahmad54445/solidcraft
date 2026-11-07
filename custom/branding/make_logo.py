"""Draw the Factory placeholder logo (pure Python): PNG 256 + multi-size ICO (PNG entries).

Usage: python make_logo.py <out_dir>
Mark: rounded blue square, a white "F" made of a column and two beams, an orange diagonal brace.
"""
import struct
import sys
import zlib
from pathlib import Path

BG = (30, 78, 140)
WHITE = (255, 255, 255)
ORANGE = (245, 158, 11)


def inside_round_rect(x, y, x0, y0, x1, y1, r):
    if not (x0 <= x <= x1 and y0 <= y <= y1):
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def inside_poly(x, y, pts):
    hit = False
    for i in range(len(pts)):
        (ax, ay), (bx, by) = pts[i], pts[(i + 1) % len(pts)]
        if (ay > y) != (by > y) and x < ax + (y - ay) / (by - ay) * (bx - ax):
            hit = not hit
    return hit


def colour_at(u, v):
    """u, v in 0..1 -> RGBA or None (transparent)."""
    if not inside_round_rect(u, v, 0.0, 0.0, 1.0, 1.0, 0.2):
        return None
    # "F": column, top beam, middle beam (unit square coordinates)
    rects = [(0.27, 0.18, 0.40, 0.82), (0.27, 0.18, 0.76, 0.30), (0.27, 0.45, 0.64, 0.56)]
    for x0, y0, x1, y1 in rects:
        if x0 <= u <= x1 and y0 <= v <= y1:
            return WHITE + (255,)
    # brace between column and top beam
    brace = [(0.40, 0.37), (0.50, 0.30), (0.63, 0.30), (0.40, 0.46)]
    if inside_poly(u, v, brace):
        return ORANGE + (255,)
    return BG + (255,)


def render(size, ss=4):
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            acc = [0, 0, 0]
            cov = 0
            for j in range(ss):
                for i in range(ss):
                    c = colour_at((x + (i + 0.5) / ss) / size, (y + (j + 0.5) / ss) / size)
                    if c:
                        cov += 1
                        for k in range(3):
                            acc[k] += c[k]
            if cov:
                row += bytes([acc[0] // cov, acc[1] // cov, acc[2] // cov, 255 * cov // (ss * ss)])
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return rows


def png(size):
    raw = b"".join(b"\0" + r for r in render(size))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(sizes):
    images = [png(s) for s in sizes]
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for s, data in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return head + entries + b"".join(images)


if __name__ == "__main__":
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    (out / "logo.png").write_bytes(png(256))
    (out / "logo.ico").write_bytes(ico([16, 24, 32, 48, 64, 256]))
    print("written", out)
