#!/usr/bin/env python3
"""Generate vector-icons/icon-v2.svg (second vector replica of src-tauri/icons/icon.png).

Shape and colour parameters were measured / least-squares fitted on icon.png's
512px pixel grid ("PNG frame"), then mapped uniformly onto a pure white
macOS-grid base (412px rounded square at 50,50 -- same base as icon.svg).

Usage:
    python3 gen_icon_v2.py
    rsvg-convert -w 512 -h 512 icon-v2.svg -o /tmp/icon-v2.png
"""
import math
from pathlib import Path

OUT = Path(__file__).with_name('icon-v2.svg')

# ---------- base (final canvas) ----------
CANVAS = 512
BASE_X, BASE_Y, BASE_SIZE, BASE_R = 50.0, 50.0, 412.0, 92.5
# the (slightly irregular) base of icon.png, used to map PNG frame -> final canvas
PNG_BASE_CX, PNG_BASE_CY, PNG_BASE_SIZE = 255.0, 255.5, 465.5

# ---------- star (PNG frame) ----------
STAR_C = (252.0, 258.2)
# radii of the sharp (un-rounded) star vertices; the two lower arms are a bit longer
STAR_R_TIP, STAR_R_BOTTOM, STAR_R_IN = 300.1, 330.5, 100.7
TIP_R, VALLEY_R = 41.7, 24.25         # corner rounding radii: tips / valleys
RING = 25.2                           # dark outline thickness (fill = outline inset by RING)

# ---------- marker (PNG frame) ----------
# local frame: origin on the barrel axis level with the nib tip, +u along the pen
# axis (towards the lower right), +y perpendicular (towards the lower left)
PEN_ORIGIN = (293.1, 304.0)
PEN_ANGLE = 44.0
NIB_Y, NIB_R = 3.75, 10.5       # nib tip cap: centre sits slightly off-axis, radius
# flare from the tip cap to the barrel, one cubic per side: (handle 1 u, handle 2 u, end u)
FLARE_UP = (65.7, 75.2, 95.9)   # y < 0 side
FLARE_DN = (80.8, 66.7, 91.0)   # y > 0 side
BODY_HW = 69.3                  # barrel half width
BODY_END = 260.0                # well past the base edge (clipped)
WIN_HW = 45.3                   # inner window half width
BAND_TOP, BAND_FLAT = 81.5, 24.5  # white band: flat top and its half width
BAND_SIDE = 92.4                # the band's chamfers meet the window sides here
BAND_ROUND = (5.0, 3.5)         # corner softening (flat/chamfer, chamfer/side)
BLUE_START = 102.6
SPLIT = 6.2                     # light (y < SPLIT) / dark (y > SPLIT) stripe boundary
KEYLINE = 19.5                  # white gap between marker and star

# ---------- colours (vertical gradients, stops keyed by PNG-frame y) ----------
RING_STOPS = [(80, '#01B8FA'), (200, '#0274C3'), (250, '#024E8C'), (350, '#013157'), (420, '#012C4B')]
FILL_STOPS = [(95, '#03E2FE'), (170, '#01CDFF'), (330, '#027BF2'), (400, '#0263F5')]
LIGHT_STOPS = [(348, '#01CAFF'), (449, '#0180FC')]
DARK_STOPS = [(385, '#0182F3'), (470, '#0156EA')]
NAVY = '#022E4D'


def scale():
    return BASE_SIZE / PNG_BASE_SIZE


def tf(p):
    """PNG frame -> final canvas."""
    s = scale()
    return (BASE_X + BASE_SIZE / 2 + s * (p[0] - PNG_BASE_CX),
            BASE_Y + BASE_SIZE / 2 + s * (p[1] - PNG_BASE_CY))


def num(v):
    s = f'{v:.2f}'.rstrip('0').rstrip('.')
    return '0' if s == '-0' else s


def pt(p):
    return f'{num(p[0])} {num(p[1])}'


# ---------- star geometry ----------
def star_vertices():
    cx, cy = STAR_C
    out = []
    for k in range(10):
        a = math.radians(-90 + 36 * k)
        r = STAR_R_IN if k % 2 else (STAR_R_BOTTOM if k in (4, 6) else STAR_R_TIP)
        out.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return out


def inset(poly, d):
    """Move every edge line towards the star centre by d, re-intersect neighbours."""
    cx, cy = STAR_C
    lines = []
    n = len(poly)
    for i in range(n):
        (ax, ay), (bx, by) = poly[i], poly[(i + 1) % n]
        ex, ey = bx - ax, by - ay
        L = math.hypot(ex, ey)
        ex, ey = ex / L, ey / L
        nx, ny = -ey, ex
        if (cx - ax) * nx + (cy - ay) * ny < 0:
            nx, ny = -nx, -ny
        lines.append(((ax + nx * d, ay + ny * d), (ex, ey)))
    out = []
    for i in range(n):
        (p1, e1), (p2, e2) = lines[i - 1], lines[i]
        det = e1[0] * -e2[1] - e1[1] * -e2[0]
        t = ((p2[0] - p1[0]) * -e2[1] - (p2[1] - p1[1]) * -e2[0]) / det
        out.append((p1[0] + e1[0] * t, p1[1] + e1[1] * t))
    return out


def rounded_path(poly, radii):
    """Closed path with every vertex rounded by a tangent circular arc (PNG frame in, canvas out)."""
    s = scale()
    n = len(poly)
    d = ''
    for i in range(n):
        vx, vy = poly[i]
        px, py = poly[i - 1]
        qx, qy = poly[(i + 1) % n]
        l1, l2 = math.hypot(px - vx, py - vy), math.hypot(qx - vx, qy - vy)
        e1 = ((px - vx) / l1, (py - vy) / l1)
        e2 = ((qx - vx) / l2, (qy - vy) / l2)
        half = math.acos(max(-1.0, min(1.0, e1[0] * e2[0] + e1[1] * e2[1]))) / 2
        t = radii[i] / math.tan(half)
        t1 = (vx + e1[0] * t, vy + e1[1] * t)
        t2 = (vx + e2[0] * t, vy + e2[1] * t)
        # arc turns the same way as the polygon at this vertex
        turn = (vx - px) * (qy - vy) - (vy - py) * (qx - vx)
        sweep = 1 if turn > 0 else 0
        d += f"{'M' if i == 0 else 'L'}{pt(tf(t1))}A{num(radii[i] * s)} {num(radii[i] * s)} 0 0 {sweep} {pt(tf(t2))}"
    return d + 'Z'


# ---------- marker geometry ----------
def pen(u, y):
    a = math.radians(PEN_ANGLE)
    ox, oy = PEN_ORIGIN
    return tf((ox + u * math.cos(a) - y * math.sin(a), oy + u * math.sin(a) + y * math.cos(a)))


def P(u, y):
    return pt(pen(u, y))


def marker_outline():
    r = NIB_R * scale()
    top, bot = NIB_Y - NIB_R, NIB_Y + NIB_R
    (a1, a2, a3), (b1, b2, b3) = FLARE_UP, FLARE_DN
    return (f'M{P(0, top)}'
            f'C{P(a1, top)} {P(a2, -BODY_HW)} {P(a3, -BODY_HW)}'
            f'L{P(BODY_END, -BODY_HW)}L{P(BODY_END, BODY_HW)}L{P(b3, BODY_HW)}'
            f'C{P(b2, BODY_HW)} {P(b1, bot)} {P(0, bot)}'
            f'A{num(r)} {num(r)} 0 0 1 {P(-NIB_R, NIB_Y)}'
            f'A{num(r)} {num(r)} 0 0 1 {P(0, top)}Z')


def window_outline():
    """Window hole: chamfered top (the white band's upper edge), softened corners."""
    pts = [(BODY_END, -WIN_HW), (BAND_SIDE, -WIN_HW), (BAND_TOP, -BAND_FLAT),
           (BAND_TOP, BAND_FLAT), (BAND_SIDE, WIN_HW), (BODY_END, WIN_HW)]
    cut = [0, BAND_ROUND[1], BAND_ROUND[0], BAND_ROUND[0], BAND_ROUND[1], 0]
    d = f'M{P(*pts[0])}'
    for i in range(1, 5):
        (pu, py), (vu, vy), (nu, ny) = pts[i - 1], pts[i], pts[i + 1]
        l1, l2 = math.hypot(pu - vu, py - vy), math.hypot(nu - vu, ny - vy)
        t1 = (vu + (pu - vu) / l1 * cut[i], vy + (py - vy) / l1 * cut[i])
        t2 = (vu + (nu - vu) / l2 * cut[i], vy + (ny - vy) / l2 * cut[i])
        d += f'L{P(*t1)}Q{P(vu, vy)} {P(*t2)}'
    return d + f'L{P(*pts[5])}Z'


def quad(u0, u1, y0, y1):
    return f'M{P(u0, y0)}L{P(u1, y0)}L{P(u1, y1)}L{P(u0, y1)}Z'


def gradient(gid, stops):
    y0, y1 = tf((0, stops[0][0]))[1], tf((0, stops[-1][0]))[1]
    s = ''.join(f'<stop offset="{num((tf((0, y))[1] - y0) / (y1 - y0))}" stop-color="{c}"/>' for y, c in stops)
    return (f'<linearGradient id="{gid}" x1="0" y1="{num(y0)}" x2="0" y2="{num(y1)}" '
            f'gradientUnits="userSpaceOnUse">{s}</linearGradient>')


def build():
    s = scale()
    star = star_vertices()
    fill = inset(star, RING)
    radii = [TIP_R if k % 2 == 0 else VALLEY_R for k in range(10)]
    fill_radii = [TIP_R - RING if k % 2 == 0 else VALLEY_R + RING for k in range(10)]
    over = 4.0  # window contents run this far under the navy frame, so its edges stay crisp
    base = (f'x="{num(BASE_X)}" y="{num(BASE_Y)}" width="{num(BASE_SIZE)}" '
            f'height="{num(BASE_SIZE)}" rx="{num(BASE_R)}"')
    silhouette = marker_outline()
    return f'''<svg width="{CANVAS}" height="{CANVAS}" viewBox="0 0 {CANVAS} {CANVAS}" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <!-- a mask (not clip-path) so the rounded edge is anti-aliased once, on the finished
         artwork: a white base + separately clipped marker would leave a white AA fringe -->
    <mask id="base" maskUnits="userSpaceOnUse" x="0" y="0" width="{CANVAS}" height="{CANVAS}"><rect {base} fill="#FFFFFF"/></mask>
    {gradient('ring', RING_STOPS)}
    {gradient('fill', FILL_STOPS)}
    {gradient('light', LIGHT_STOPS)}
    {gradient('dark', DARK_STOPS)}
  </defs>
  <g mask="url(#base)">
    <rect width="{CANVAS}" height="{CANVAS}" fill="#FFFFFF"/>
    <!-- star: dark outline, then the gradient face inset by the outline width -->
    <path d="{rounded_path(star, radii)}" fill="url(#ring)"/>
    <path d="{rounded_path(fill, fill_radii)}" fill="url(#fill)"/>
    <!-- marker: white keyline cut into the star -->
    <path d="{silhouette}" fill="#FFFFFF" stroke="#FFFFFF" stroke-width="{num(2 * KEYLINE * s)}" stroke-linejoin="round"/>
    <!-- window contents: white cap band, light + dark barrel stripes -->
    <path d="{quad(BAND_TOP - over, BLUE_START + 3, -WIN_HW - over, WIN_HW + over)}" fill="#FFFFFF"/>
    <path d="{quad(BLUE_START, BODY_END, -WIN_HW - over, WIN_HW + over)}" fill="url(#light)"/>
    <path d="{quad(BLUE_START, BODY_END, SPLIT, WIN_HW + over)}" fill="url(#dark)"/>
    <!-- navy body with the window cut out -->
    <path d="{silhouette}{window_outline()}" fill="{NAVY}" fill-rule="evenodd"/>
  </g>
</svg>
'''


if __name__ == '__main__':
    svg = build()
    OUT.write_text(svg)
    print('written', OUT, len(svg), 'bytes')
