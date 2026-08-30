#!/usr/bin/env python3
"""Generate vector-icons/icon.svg (vector replica of src-tauri/icons/icon.png).

Usage:
    python3 gen_icon.py
    rsvg-convert -w 512 -h 512 icon.svg -o /tmp/render.png
"""
import math

CX, CY = 252.5, 261.0
R_OUT, R_IN = 171.0, 87.0
D_OUT, D_IN = 24.0, 17.0  # corner rounding distances


def star_path():
    pts = []
    for k in range(5):
        a_out = math.radians(-90 + 72 * k)
        a_in = math.radians(-54 + 72 * k)
        pts.append((CX + R_OUT * math.cos(a_out), CY + R_OUT * math.sin(a_out), 'o'))
        pts.append((CX + R_IN * math.cos(a_in), CY + R_IN * math.sin(a_in), 'i'))
    n = len(pts)
    segs = []
    for i in range(n):
        vx, vy, t = pts[i]
        px, py, _ = pts[i - 1]
        nx, ny, _ = pts[(i + 1) % n]
        dd = D_OUT if t == 'o' else D_IN
        e1 = math.hypot(vx - px, vy - py)
        e2 = math.hypot(nx - vx, ny - vy)
        p1 = (vx + (px - vx) / e1 * dd, vy + (py - vy) / e1 * dd)
        p2 = (vx + (nx - vx) / e2 * dd, vy + (ny - vy) / e2 * dd)
        segs.append((p1, (vx, vy), p2))
    # start at the exit point of the last vertex, then round every vertex in order
    (p1, c, p2) = segs[-1]
    d = f"M {p2[0]:.1f} {p2[1]:.1f} "
    for p1, c, p2 in segs:
        d += f"L {p1[0]:.1f} {p1[1]:.1f} Q {c[0]:.1f} {c[1]:.1f} {p2[0]:.1f} {p2[1]:.1f} "
    d += "Z"
    return d


# marker: local coords, origin = star center, +x = pen axis (45 deg down-right)
# nib tip x=30 -> collar x=120; barrel rect x 120..335 half-width 60
def nib_path():
    tip, collar, ht, hc = 62.0, 120.0, 11.0, 28.0
    return (f"M {tip} {-ht} "
            f"Q {tip + 32} {-hc + 8} {collar} {-hc} "   # upper side, slight concave
            f"L {collar} {hc} "
            f"Q {tip + 32} {hc - 8} {tip} {ht} "        # lower side, symmetric
            f"Q {tip - 13} 0 {tip} {-ht} Z")            # rounded tip cap


def barrel_path(x0=120.0, x1=350.0, hw=64.0, rx=26.0):
    return (f"M {x0} {-hw + rx} Q {x0} {-hw} {x0 + rx} {-hw} "
            f"L {x1} {-hw} Q {x1 + rx} {-hw} {x1 + rx} {-hw + rx} "
            f"L {x1 + rx} {hw - rx} Q {x1 + rx} {hw} {x1} {hw} "
            f"L {x0 + rx} {hw} Q {x0} {hw} {x0} {hw - rx} Z")


def core_path(x0=134.0, x1=328.0, hw=44.0, rx=22.0):
    return barrel_path(x0, x1, hw, rx)


star = star_path()
nib = nib_path()
barrel = barrel_path()
core = core_path()

svg = f'''<svg width="512" height="512" viewBox="0 0 512 512" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <!-- macOS-style base: 412x412 rounded rect, radius 92.5 -->
    <rect id="base" x="50" y="50" width="412" height="412" rx="92.5"/>
    <clipPath id="baseClip"><rect x="50" y="50" width="412" height="412" rx="92.5"/></clipPath>

    <linearGradient id="starFill" x1="256" y1="80" x2="256" y2="448" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#06DCFF"/>
      <stop offset="0.39" stop-color="#01B4FF"/>
      <stop offset="1" stop-color="#0169EE"/>
    </linearGradient>
    <linearGradient id="starStroke" x1="256" y1="70" x2="256" y2="460" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#0FA0E4"/>
      <stop offset="0.4" stop-color="#0272C4"/>
      <stop offset="1" stop-color="#073F84"/>
    </linearGradient>
    <!-- barrel gradient along pen axis (45 deg, shoulder -> tip end) -->
    <linearGradient id="bodyGrad" x1="345" y1="345" x2="480" y2="480" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#4EC7FA"/>
      <stop offset="0.25" stop-color="#26B4F8"/>
      <stop offset="0.55" stop-color="#0084EE"/>
      <stop offset="1" stop-color="#0A50D6"/>
    </linearGradient>
  </defs>

  <use href="#base" fill="#FFFFFF"/>

  <g clip-path="url(#baseClip)">
    <g transform="translate(256 258) scale(0.92) translate(-256 -258)">
      <!-- star: dark outline layer + gradient fill -->
      <path d="{star}" fill="url(#starStroke)" stroke="url(#starStroke)" stroke-width="21"/>
      <path d="{star}" fill="url(#starFill)"/>

      <!-- marker -->
      <g transform="translate(256 258) rotate(45)">
        <!-- white keyline halo -->
        <g fill="#FFFFFF" stroke="#FFFFFF" stroke-width="21" stroke-linejoin="round">
          <path d="{nib}"/>
          <path d="{barrel}"/>
        </g>
        <!-- dark navy silhouette -->
        <g fill="#002B50">
          <path d="{nib}"/>
          <path d="{barrel}"/>
        </g>
        <!-- blue core with inner white ring -->
        <path d="{core}" fill="url(#bodyGrad)" stroke="#FFFFFF" stroke-width="12"/>
      </g>
    </g>
  </g>
</svg>
'''

with open('/Users/zhb/projects/anki-marker/vector-icons/icon.svg', 'w') as f:
    f.write(svg)
print("written", len(svg), "bytes")
