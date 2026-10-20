"""Post-production for AnvilCAD films: camera moves, titles, captions, the agent overlay,
transitions, grading and encoding.

The harness gives us exact screenshots and nothing else, so everything cinematic happens
here, in 2D, on top of real frames:

- Hero shots fill the frame with the model on a dark studio backdrop. A slow push-in and
  drift on a 2x supersampled frame reads as a camera dolly, and jumps between states
  (a feature appearing, a component snapping into place) dissolve instead of popping.
- Interface shots put the whole app in a framed window. A camera follows the pointer and
  pushes in on the action, the way product films zoom into the part of the screen that
  matters.
- Cards are full-frame titles with staggered, eased reveals. Captions are lower thirds.
- The agent overlay is a terminal card that types out the exact JSON commands the script
  sent to anvilcad-agent, as they were sent.

Frames are streamed straight into ffmpeg, so a film never needs gigabytes of PNGs.
"""

import functools
import json
import math
import subprocess
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

from recorder import ACCENT, FPS, draw_pointer
from take import SIZE

# Output settings; films.py calls configure() before rendering.
OUT = {"w": 1920, "h": 1080, "draft": False, "grain": False, "letterbox": False}

K_FADE = 14   # frames a crossfade overlaps
K_BLACK = 10  # frames of each half of a dip to black

# Where hero shots look by default, in screen points: the middle of the viewport, clear of
# the view toolbar above it and the axis triad in its lower left corner.
STAGE = (375.0, 190.0, 1815.0, 1000.0)

# The part of the screen a hero shot may show: the viewport, below the view toolbar and
# to the right of the axis triad.
HERO_REGION = (340.0, 190.0, 1920.0, 1048.0)

INK = (240, 242, 246)
INK_WEAK = (150, 158, 170)


def configure(draft=False, grain=False, letterbox=False):
    OUT.update(draft=draft, grain=grain, letterbox=letterbox,
               w=960 if draft else 1920, h=540 if draft else 1080)


def k():
    """Scale from the 1920-wide design to the output size."""
    return OUT["w"] / 1920


# -- easing ---------------------------------------------------------------------------

def clamp01(t):
    return max(0.0, min(1.0, t))


def smooth(t):
    t = clamp01(t)
    return t * t * (3 - 2 * t)


def ease_out(t):
    t = clamp01(t)
    return 1 - (1 - t) ** 3


def ease_in_out(t):
    t = clamp01(t)
    return 0.5 - 0.5 * math.cos(math.pi * t)


def lerp(a, b, t):
    return a + (b - a) * t


# -- fonts ----------------------------------------------------------------------------

FONTS = {
    "bold": [("/System/Library/Fonts/HelveticaNeue.ttc", 1),
             ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 0),
             ("C:/Windows/Fonts/segoeuib.ttf", 0)],
    "medium": [("/System/Library/Fonts/HelveticaNeue.ttc", 10),
               ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
               ("C:/Windows/Fonts/segoeui.ttf", 0)],
    "light": [("/System/Library/Fonts/HelveticaNeue.ttc", 7),
              ("/usr/share/fonts/truetype/dejavu/DejaVuSans-ExtraLight.ttf", 0),
              ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
              ("C:/Windows/Fonts/segoeuil.ttf", 0)],
    "mono": [("/System/Library/Fonts/Menlo.ttc", 0),
             ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 0),
             ("C:/Windows/Fonts/consola.ttf", 0)],
}


@functools.cache
def font(kind, size):
    size = max(6, int(round(size * k())))
    for path, index in FONTS[kind]:
        if Path(path).exists():
            return ImageFont.truetype(path, size, index=index)
    return ImageFont.load_default(size)


def text_width(text, f, tracking=0.0):
    if not text:
        return 0
    return sum(f.getlength(ch) for ch in text) + tracking * f.size * (len(text) - 1)


TEXT_SS = 4  # text is drawn this many times larger, then reduced, for sub-pixel motion


@functools.cache
def _big(path, index, size):
    return ImageFont.truetype(path, size * TEXT_SS, index=index)


def draw_text(img, xy, text, f, fill, alpha=1.0, tracking=0.0, anchor="ls"):
    """Draw text with letter spacing (`tracking`, in ems) and opacity onto an RGB image.
    `anchor` is "ls" (left), "ms" (centred) or "rs" (right), on the baseline.

    Positions are kept to a quarter of a pixel: the text is drawn four times larger and
    reduced, so a title that rises or tightens its spacing moves smoothly instead of
    stepping a whole pixel at a time."""
    if alpha <= 0.003 or not text:
        return
    w = text_width(text, f, tracking)
    x, y = xy
    if anchor[0] == "m":
        x -= w / 2
    elif anchor[0] == "r":
        x -= w
    ascent, descent = f.getmetrics()
    pad = int(f.size * 0.3) + 2
    ix, iy = math.floor(x), math.floor(y)
    lw, lh = int(w) + 2 * pad + 2, ascent + descent + 2 * pad + 2
    path = getattr(f, "path", None)
    ss = TEXT_SS if path else 1
    big = _big(path, f.index, f.size) if path else f
    layer = Image.new("L", (lw * ss, lh * ss), 0)
    d = ImageDraw.Draw(layer)
    cx = (pad + x - ix) * ss
    base = (pad + ascent + y - iy) * ss
    for ch in text:
        d.text((cx, base), ch, font=big, fill=255, anchor="ls")
        cx += big.getlength(ch) + tracking * big.size
    if ss > 1:
        layer = layer.resize((lw, lh), Image.BOX)
    if alpha < 1:
        layer = layer.point(lambda v: int(v * alpha))
    img.paste(Image.new("RGB", layer.size, fill), (ix - pad, iy - ascent - pad), layer)


def rounded_mask(w, h, r):
    m = Image.new("L", (w, h), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, w - 1, h - 1], radius=r, fill=255)
    return m


def overlay_rect(img, box, fill, alpha, radius=0):
    """Blend a filled, optionally rounded rectangle onto img."""
    x0, y0, x1, y1 = [int(v) for v in box]
    if x1 <= x0 or y1 <= y0 or alpha <= 0:
        return
    m = rounded_mask(x1 - x0, y1 - y0, int(radius)) if radius else Image.new("L", (x1 - x0, y1 - y0), 255)
    m = m.point(lambda v: int(v * alpha))
    img.paste(Image.new("RGB", m.size, fill), (x0, y0), m)


# -- logo -----------------------------------------------------------------------------

def _arc(cx, cy, r, a0, a1, n=48):
    return [(cx + r * math.cos(math.radians(a0 + (a1 - a0) * i / n)),
             cy + r * math.sin(math.radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def _quad(p0, p1, p2, n=16):
    return [((1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t * t * p2[0],
             (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t * t * p2[1])
            for t in (i / n for i in range(n + 1))]


@functools.cache
def logo(height, ink=INK):
    """docs/assets/logo.svg (the anvil mark and the wordmark), drawn at `height` pixels.
    The geometry is copied from the SVG's own coordinates (a 236 x 48 box)."""
    ss = 4
    u = height * ss / 48
    img = Image.new("RGBA", (int(236 * u) + 1, int(48 * u) + 1), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def stroke(pts, width, closed=False):
        p = [(x * u, y * u) for x, y in pts]
        if closed:
            p = p + p[:2]
        d.line(p, fill=ink + (255,), width=int(width * u), joint="curve")
        if not closed:
            r = width * u / 2
            for x, y in (p[0], p[-1]):
                d.ellipse([x - r, y - r, x + r, y + r], fill=ink + (255,))

    mark = [(3, 13), (45, 13), (45, 20), (35, 20)] + _quad((35, 20), (31, 22), (31, 30))[1:] + \
        [(38, 36), (38, 41), (12, 41), (12, 36), (19, 30)] + _quad((19, 30), (19, 23), (15, 21.5))[1:] + \
        _quad((15, 21.5), (8, 19), (3, 13))[1:]
    stroke(mark, 3.5, closed=True)
    d.rounded_rectangle([22 * u, 7.5 * u, 35 * u, 12 * u], radius=1.25 * u, fill=(194, 65, 12, 255))
    for cx in (69, 192, 218):
        stroke(_arc(cx, 28, 10, 0, 360, 72), 4.5, closed=True)
    for line in ([(79, 18), (79, 38)], [(144, 18), (144, 38)], [(152, 8), (152, 38)],
                 [(202, 18), (202, 38)], [(228, 8), (228, 38)], [(116, 18), (126, 38), (136, 18)]):
        stroke(line, 4.5)
    stroke([(88, 18), (88, 38)], 4.5)
    stroke(_arc(98, 28, 10, 180, 360), 4.5)
    stroke([(108, 28), (108, 38)], 4.5)
    stroke(_arc(170, 28, 10, 315, 45), 4.5)
    r = 2.6 * u
    d.ellipse([144 * u - r, 9.5 * u - r, 144 * u + r, 9.5 * u + r], fill=ink + (255,))
    return img.resize((img.width // ss, img.height // ss), Image.LANCZOS)


def paste_logo(img, center, height, alpha=1.0):
    if alpha <= 0.003:
        return
    lg = logo(int(height))
    x, y = center[0] - lg.width / 2, center[1] - lg.height / 2
    ix, iy = math.floor(x), math.floor(y)
    # Shift by the fraction of a pixel too, so the logo glides as it rises.
    lg = lg.transform(lg.size, Image.AFFINE, (1, 0, ix - x, 0, 1, iy - y), Image.BICUBIC)
    a = lg.getchannel("A").point(lambda v: int(v * alpha))
    img.paste(lg.convert("RGB"), (ix, iy), a)


# -- backdrop and grade ---------------------------------------------------------------

@functools.cache
def backdrop(w, h):
    """A deep blue-black gradient with a faint warm glow, behind cards and windows."""
    y = np.linspace(0, 1, h)[:, None, None]
    x = np.linspace(0, 1, w)[None, :, None]
    top = np.array([24, 28, 37], float)
    bottom = np.array([4, 5, 8], float)
    base = top * (1 - y) + bottom * y
    d = np.sqrt(((x - 0.5) * 1.5) ** 2 + ((y - 0.22) * 1.0) ** 2)
    glow = np.exp(-((d / 0.42) ** 2)) * np.array(ACCENT, float) * 0.13
    cool = np.exp(-(((x - 0.85) * 1.2) ** 2 + ((y - 0.9) * 1.4) ** 2) / 0.12) * np.array([40, 70, 120], float) * 0.18
    arr = np.clip(base + glow + cool, 0, 255).astype(np.uint8)
    return Image.fromarray(arr, "RGB")


@functools.cache
def vignette(w, h):
    y = np.linspace(-1, 1, h)[:, None]
    x = np.linspace(-1, 1, w)[None, :]
    r = np.sqrt((x * 0.9) ** 2 + (y * 1.0) ** 2)
    return (1.0 - 0.28 * smooth_np(np.clip((r - 0.55) / 0.85, 0, 1)))[..., None]


def smooth_np(t):
    return t * t * (3 - 2 * t)


@functools.cache
def curve():
    """A gentle S-curve: a little more contrast without crushing the blacks."""
    u = np.arange(256) / 255.0
    s = 0.5 - 0.5 * np.cos(np.pi * u)
    v = u * 0.72 + s * 0.28
    return np.clip(v * 255 + 0.5, 0, 255).astype(np.uint8)


def finish(img):
    """Grade a finished frame and return its RGB bytes."""
    arr = np.asarray(img, dtype=np.uint8)
    arr = curve()[arr]
    out = arr.astype(np.float32) * vignette(OUT["w"], OUT["h"])
    if OUT["grain"]:
        out += np.random.default_rng().normal(0, 2.2, (OUT["h"], OUT["w"], 1)).astype(np.float32)
    out = np.clip(out, 0, 255).astype(np.uint8)
    if OUT["letterbox"]:
        bar = int(round((OUT["h"] - OUT["w"] / 2.39) / 2))
        out[:bar] = 0
        out[OUT["h"] - bar:] = 0
    return out.tobytes()


# -- overlays -------------------------------------------------------------------------

def caption(img, spec, t, n_seconds, anchor=None):
    """A lower third: an accent bar, a headline and an optional line under it.
    `spec` is a string or (headline, subline). It eases in after 0.35 s and out at the end."""
    if not spec:
        return
    head, sub = (spec, None) if isinstance(spec, str) else spec
    tin = ease_out((t - 0.35) / 0.55)
    tout = 1 - smooth((t - (n_seconds - 0.5)) / 0.45)
    a = min(tin, tout)
    if a <= 0:
        return
    s = k()
    x, y = anchor or (110 * s, OUT["h"] - 150 * s)
    slide = (1 - tin) * -26 * s
    hf, sf = font("bold", 56), font("light", 30)
    bar_h = (56 + (44 if sub else 0)) * s * ease_out((t - 0.3) / 0.4)
    # A soft dark plate keeps the words legible over busy footage.
    pw = max(text_width(head, hf, -0.01), text_width(sub or "", sf)) + 80 * s
    overlay_rect(img, (x - 50 * s, y - 72 * s, x + pw, y + (70 if sub else 26) * s), (0, 0, 0), 0.42 * a, 14 * s)
    overlay_rect(img, (x - 26 * s, y - 50 * s, x - 20 * s, y - 50 * s + bar_h), ACCENT, a)
    draw_text(img, (x + slide, y), head, hf, INK, a, tracking=-0.01)
    if sub:
        draw_text(img, (x + slide * 1.4, y + 44 * s), sub, sf, INK_WEAK, a * smooth((t - 0.55) / 0.5))


def title(img, spec, t, n_seconds, dim=True):
    """A big centred title over footage. `spec` is (kicker, headline, subline), any of
    which may be None."""
    kicker, head, sub = spec
    s = k()
    a_out = 1 - smooth((t - (n_seconds - 0.45)) / 0.4)
    if dim:
        overlay_rect(img, (0, 0, OUT["w"], OUT["h"]), (0, 0, 0), 0.38 * smooth(t / 0.5) * a_out)
    cy = OUT["h"] * 0.5
    if kicker:
        a = ease_out((t - 0.05) / 0.5) * a_out
        draw_text(img, (OUT["w"] / 2, cy - 92 * s), kicker.upper(), font("medium", 24), ACCENT, a,
                  tracking=0.32, anchor="ms")
    if head:
        a = ease_out((t - 0.15) / 0.7) * a_out
        rise = (1 - ease_out((t - 0.15) / 0.9)) * 34 * s
        draw_text(img, (OUT["w"] / 2, cy + 20 * s + rise), head, font("bold", 112), INK, a,
                  tracking=lerp(0.06, -0.015, ease_out((t - 0.15) / 1.2)), anchor="ms")
    if sub:
        a = ease_out((t - 0.55) / 0.6) * a_out
        draw_text(img, (OUT["w"] / 2, cy + 88 * s), sub, font("light", 40), INK_WEAK, a, anchor="ms")


def agent_panel(img, take, frame, alpha=1.0):
    """A terminal card typing out the commands the script sent, as it sent them."""
    if alpha <= 0:
        return
    s = k()
    w, h = 820 * s, 330 * s
    x0 = OUT["w"] - w - 70 * s
    y0 = OUT["h"] - h - 70 * s
    overlay_rect(img, (x0 + 8 * s, y0 + 14 * s, x0 + w + 8 * s, y0 + h + 14 * s), (0, 0, 0), 0.35 * alpha, 18 * s)
    overlay_rect(img, (x0, y0, x0 + w, y0 + h), (11, 13, 18), 0.9 * alpha, 16 * s)
    overlay_rect(img, (x0, y0, x0 + w, y0 + 40 * s), (26, 29, 37), 0.95 * alpha, 16 * s)
    for i, col in enumerate([(255, 95, 87), (254, 188, 46), (40, 200, 64)]):
        cx, cy = x0 + (24 + 20 * i) * s, y0 + 20 * s
        overlay_rect(img, (cx - 6 * s, cy - 6 * s, cx + 6 * s, cy + 6 * s), col, alpha, 6 * s)
    draw_text(img, (x0 + w / 2, y0 + 27 * s), "anvilcad-agent  ·  JSON over stdin", font("medium", 19),
              INK_WEAK, alpha, anchor="ms")
    mono = font("mono", 19)
    shown = [(f, c) for f, c in take.log if f <= frame][-9:]
    line_h = 30 * s
    y = y0 + 76 * s
    for i, (f, c) in enumerate(shown):
        text = json.dumps(c, separators=(", ", ": "))
        if len(text) > 66:
            text = text[:65] + "…"
        latest = i == len(shown) - 1
        typed = len(text) if not latest else min(len(text), int((frame - f + 1) * 4))
        fade = 0.55 + 0.45 * (i + 1) / len(shown)
        draw_text(img, (x0 + 24 * s, y), "›", mono, ACCENT, alpha * fade)
        draw_text(img, (x0 + 46 * s, y), text[:typed], mono, INK if latest else INK_WEAK, alpha * fade)
        if typed == len(text):
            draw_text(img, (x0 + w - 24 * s, y), "ok", mono, (90, 210, 120), alpha * fade * 0.9, anchor="rs")
        elif latest and (frame // 8) % 2 == 0:
            cx = x0 + 46 * s + text_width(text[:typed], mono)
            overlay_rect(img, (cx + 2 * s, y - 18 * s, cx + 12 * s, y + 4 * s), INK, alpha)
        y += line_h


# -- clips ----------------------------------------------------------------------------

class Frames:
    """Loads screenshots, keeping only the most recent one in memory."""

    def __init__(self):
        self.path = None
        self.img = None

    def get(self, path):
        if path != self.path:
            self.path = path
            self.img = Image.open(path).convert("RGB")
        return self.img


def run_lengths(timeline):
    """For each frame, how many frames in a row (from it) show the same screenshot."""
    out = [0] * len(timeline)
    run = 0
    for i in range(len(timeline) - 1, -1, -1):
        run = run + 1 if i + 1 < len(timeline) and timeline[i + 1][0] == timeline[i][0] else 1
        out[i] = run
    return out


def moving_average(values, radius):
    """Smooth a list of numbers with a centred window, shrinking the window at the ends."""
    out = []
    for i in range(len(values)):
        a, b = max(0, i - radius), min(len(values), i + radius + 1)
        out.append(sum(values[a:b]) / (b - a))
    return out


def hero_camera(clip, tl):
    """Where a hero shot looks on each frame: (centre x, centre y, crop height) in points.

    By default it frames the part itself: the centre of its bounding box on screen, with
    the crop sized so the part fills `fill` of the frame, smoothed over about a second so
    the camera glides as the part turns or grows. `push=(start, end)` then zooms in or out
    from that framing over the shot, and `focus`/`drift` override or offset the centre."""
    o = clip.opts
    region = o.get("region", HERO_REGION)
    fill = o.get("fill", 0.6)
    aspect = SIZE[1] / SIZE[0]
    stage = o.get("stage", STAGE)
    sx, sy = (stage[0] + stage[2]) / 2, (stage[1] + stage[3]) / 2
    sh = stage[3] - stage[1]
    cxs, cys, chs = [], [], []
    last = None
    for path, _c, _r in tl:
        bb = clip.take.bounds.get(path) or last
        last = bb
        if bb and o.get("frame", True):
            cx, cy = (bb[0] + bb[2]) / 2, (bb[1] + bb[3]) / 2
            ch = max(bb[3] - bb[1], (bb[2] - bb[0]) * aspect) / fill
        else:
            cx, cy, ch = sx, sy, sh
        cxs.append(cx)
        cys.append(cy)
        chs.append(ch)
    rad = o.get("smooth", 15)
    cxs, cys, chs = moving_average(cxs, rad), moving_average(cys, rad), moving_average(chs, rad)
    if "focus" in o:
        cxs = [o["focus"][0]] * len(tl)
        cys = [o["focus"][1]] * len(tl)
    max_h = min(region[3] - region[1], (region[2] - region[0]) * aspect)
    z0, z1 = o.get("push", (1.0, 1.08))
    dx, dy = o.get("drift", (0.0, 0.0))
    n = len(tl)
    cams = []
    for i in range(n):
        t = i / max(1, n - 1)
        h = min(max_h, chs[i] / lerp(z0, z1, ease_in_out(t)))
        w = h / aspect
        x0 = cxs[i] + dx * (t - 0.5) - w / 2
        y0 = cys[i] + dy * (t - 0.5) - h / 2
        # Stay inside the viewport, so no panel or toolbar slides into a hero shot.
        x0 = min(max(x0, region[0]), region[2] - w)
        y0 = min(max(y0, region[1]), region[3] - h)
        cams.append((x0, y0, w, h))
    return cams


def hero_frames(clip):
    o = clip.opts
    take = clip.take
    tl = take.timeline[clip.start:clip.end]
    n = len(tl)
    cams = hero_camera(clip, tl)
    dissolve = o.get("dissolve", 8)
    sc = take.scale
    loader = Frames()
    runs = run_lengths(tl)
    prev_out, prev_path, blend_from, blend_i = None, None, None, 0
    secs = n / FPS
    for i, (path, _cursor, _ring) in enumerate(tl):
        x0, y0, sw, sh = cams[i]
        src = loader.get(path)
        frame = src.resize((OUT["w"], OUT["h"]), Image.BICUBIC,
                           box=(x0 * sc, y0 * sc, (x0 + sw) * sc, (y0 + sh) * sc))
        # A state change that then holds dissolves in; a run of changing frames (an orbit)
        # passes straight through.
        if prev_path is not None and path != prev_path and runs[i] >= 3 and dissolve:
            blend_from, blend_i = prev_out, 0
        if blend_from is not None:
            blend_i += 1
            if blend_i <= dissolve:
                frame = Image.blend(blend_from, frame, smooth(blend_i / (dissolve + 1)))
            else:
                blend_from = None
        prev_out, prev_path = frame, path
        out = frame.copy()
        decorate(out, clip, i, i / FPS, secs)
        yield out


@functools.cache
def window_base(w, h):
    """The backdrop with a soft shadow where the app window sits."""
    m, ww, wh, x0, y0 = window_geometry(w, h)
    base = backdrop(w, h).copy()
    s = k()
    for i, (grow, a) in enumerate([(40, 0.10), (24, 0.14), (12, 0.2)]):
        g = grow * s
        overlay_rect(base, (x0 - g, y0 - g + 18 * s, x0 + ww + g, y0 + wh + g + 18 * s), (0, 0, 0), a, 30 * s + g)
    overlay_rect(base, (x0 - 1, y0 - 1, x0 + ww + 1, y0 + wh + 1), (70, 76, 88), 0.8, 15 * s)
    return base


def window_geometry(w, h):
    m = round(w * 0.06)
    ww = w - 2 * m
    wh = round(ww * SIZE[1] / SIZE[0])
    return m, ww, wh, m, (h - wh) // 2


def ui_frames(clip):
    o = clip.opts
    take = clip.take
    tl = take.timeline[clip.start:clip.end]
    n = len(tl)
    zmax = o.get("zoom", 1.45)
    follow = o.get("follow", 0.075)
    ramp_in, ramp_out = o.get("ramp", (0.9, 0.7))
    _, ww, wh, wx, wy = window_geometry(OUT["w"], OUT["h"])
    mask = rounded_mask(ww, wh, int(14 * k()))
    sc = take.scale
    loader = Frames()
    # The camera path: a centre that chases the pointer, and a zoom that rises after the
    # shot opens and settles back before it ends.
    cx, cy = o.get("start", (SIZE[0] / 2, SIZE[1] / 2))
    target = (cx, cy)
    cams = []
    lean = o.get("lean", 0.45)
    part = None
    for i, (path, cursor, _r) in enumerate(tl):
        bb = take.bounds.get(path)
        if bb:
            part = ((bb[0] + bb[2]) / 2, (bb[1] + bb[3]) / 2)
        if cursor is not None:
            target = cursor
            # Aim between the pointer and the part, so the action and the model it acts
            # on share the frame.
            if part is not None:
                target = (cursor[0] + (part[0] - cursor[0]) * lean, cursor[1] + (part[1] - cursor[1]) * lean)
        if "focus" in o:
            target = o["focus"]
        cx += (target[0] - cx) * follow
        cy += (target[1] - cy) * follow
        t = i / FPS
        env = min(ease_in_out(t / ramp_in), ease_in_out((n / FPS - t) / ramp_out)) if n > 1 else 0
        cams.append((cx, cy, 1 + (zmax - 1) * env))
    secs = n / FPS
    for i, (path, cursor, ring) in enumerate(tl):
        cx, cy, z = cams[i]
        sw, sh = SIZE[0] / z, SIZE[1] / z
        x0 = min(max(cx - sw / 2, 0), SIZE[0] - sw)
        y0 = min(max(cy - sh / 2, 0), SIZE[1] - sh)
        src = loader.get(path)
        view = src.resize((ww, wh), Image.BICUBIC, box=(x0 * sc, y0 * sc, (x0 + sw) * sc, (y0 + sh) * sc))
        f = ww / sw
        if cursor is not None:
            d = ImageDraw.Draw(view)
            px, py = (cursor[0] - x0) * f, (cursor[1] - y0) * f
            if ring is not None:
                t = ring / (0.35 * FPS)
                r = (6 + 22 * t) * f
                col = tuple(int(c * (1 - t) + 255 * t) for c in ACCENT)
                d.ellipse([px - r, py - r, px + r, py + r], outline=col, width=max(1, int(3 * f)))
            draw_pointer(d, px, py, f)
        out = window_base(OUT["w"], OUT["h"]).copy()
        out.paste(view, (wx, wy), mask)
        decorate(out, clip, i, i / FPS, secs)
        yield out


def decorate(img, clip, i, t, secs):
    o = clip.opts
    if o.get("title"):
        title(img, o["title"], t, secs, dim=o.get("dim", True))
    if o.get("caption"):
        if clip.mode == "ui":
            _, _, wh, wx, wy = window_geometry(OUT["w"], OUT["h"])
            caption(img, o["caption"], t, secs, anchor=(wx + 70 * k(), wy + wh - 60 * k()))
        else:
            caption(img, o["caption"], t, secs)
    if o.get("agent"):
        a = min(ease_out((t - 0.2) / 0.5), 1 - smooth((t - (secs - 0.4)) / 0.35))
        agent_panel(img, clip.take, clip.start + i, a)


def clip_frames(clip):
    return hero_frames(clip) if clip.mode == "hero" else ui_frames(clip)


# -- cards ----------------------------------------------------------------------------

class Card:
    """A full-frame title on the studio backdrop.

    kicker: small accent capitals above; head: the title; sub: a line under it;
    foot: a monospace line near the bottom (a URL or a command); note: fine print.
    """

    def __init__(self, head=None, sub=None, kicker=None, foot=None, note=None, seconds=2.6, logo=False):
        self.head, self.sub, self.kicker = head, sub, kicker
        self.foot, self.note, self.logo = foot, note, logo
        self.seconds = seconds

    def __len__(self):
        return int(round(self.seconds * FPS))

    def frames(self):
        n = len(self)
        s = k()
        w, h = OUT["w"], OUT["h"]
        for i in range(n):
            t = i / FPS
            img = backdrop(w, h).copy()
            cy = h * 0.5
            if self.logo:
                # The wordmark stands in for the title: it rises and settles in the middle.
                a = ease_out((t - 0.1) / 0.8)
                rise = (1 - ease_out((t - 0.1) / 1.1)) * 30 * s
                paste_logo(img, (w / 2, cy - 10 * s + rise), 120 * s, a)
                rule = 160 * s * ease_out((t - 0.6) / 0.7)
                overlay_rect(img, (w / 2 - rule / 2, cy + 62 * s, w / 2 + rule / 2, cy + 66 * s), ACCENT, a)
            if self.kicker:
                a = ease_out((t - 0.1) / 0.5)
                draw_text(img, (w / 2, cy - (122 if self.logo else 78) * s), self.kicker.upper(), font("medium", 24), ACCENT, a,
                          tracking=0.32, anchor="ms")
            if self.head:
                a = ease_out((t - 0.2) / 0.7)
                rise = (1 - ease_out((t - 0.2) / 0.9)) * 40 * s
                draw_text(img, (w / 2, cy + 30 * s + rise), self.head, font("bold", 120), INK, a,
                          tracking=lerp(0.08, -0.02, ease_out((t - 0.2) / 1.4)), anchor="ms")
                rule = 160 * s * ease_out((t - 0.5) / 0.7)
                overlay_rect(img, (w / 2 - rule / 2, cy + 62 * s, w / 2 + rule / 2, cy + 66 * s), ACCENT, a)
            if self.sub:
                a = ease_out((t - 0.6) / 0.6)
                draw_text(img, (w / 2, cy + 128 * s), self.sub, font("light", 42), INK_WEAK, a, anchor="ms")
            if self.foot:
                a = ease_out((t - 0.9) / 0.6)
                draw_text(img, (w / 2, h - 150 * s), self.foot, font("mono", 30), INK, a, anchor="ms")
            if self.note:
                a = ease_out((t - 1.1) / 0.6)
                draw_text(img, (w / 2, h - 80 * s), self.note, font("light", 20), INK_WEAK, a * 0.8, anchor="ms")
            yield img


# -- films ----------------------------------------------------------------------------

def item_frames(item):
    return item.frames() if isinstance(item, Card) else clip_frames(item)


class Film:
    """A sequence of clips and cards. Each entry is (item, transition into it), where the
    transition is "cut", "fade" (a crossfade) or "black" (a dip through black). The first
    item always comes up from black and the last goes down to it."""

    def __init__(self, name, items):
        self.name = name
        self.items = [(it, tr) for it, tr in items if len(it) > 0]

    def render(self, out_dir, music=None, poster=0.5):
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        mp4 = out_dir / f"{self.name}.mp4"
        w, h = OUT["w"], OUT["h"]
        enc = subprocess.Popen(
            ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24",
             "-s", f"{w}x{h}", "-r", str(FPS), "-i", "-", "-c:v", "libx264",
             "-preset", "veryfast" if OUT["draft"] else "slow", "-crf", "23" if OUT["draft"] else "16",
             "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(mp4)],
            stdin=subprocess.PIPE,
        )
        total = self.length()
        poster_at = int(total * poster)
        black = Image.new("RGB", (w, h), (0, 0, 0))
        written = 0

        def write(img):
            nonlocal written
            if written == poster_at:
                img.save(out_dir / f"{self.name}-poster.jpg", quality=90)
            enc.stdin.write(finish(img))
            written += 1

        pending = []
        items = self.items
        for idx, (item, trans) in enumerate(items):
            if idx == 0:
                trans = "black"
            nxt = items[idx + 1][1] if idx + 1 < len(items) else "black"
            n = len(item)
            hold = {"fade": K_FADE, "black": K_BLACK}.get(nxt, 0)
            hold = min(hold, n // 3)
            buf = deque()
            for j, f in enumerate(item_frames(item)):
                if trans == "fade" and j < len(pending):
                    f = Image.blend(pending[j], f, smooth((j + 1) / (len(pending) + 1)))
                elif trans == "black" and j < K_BLACK:
                    f = Image.blend(black, f, smooth((j + 1) / (K_BLACK + 1)))
                buf.append(f)
                if len(buf) > hold:
                    write(buf.popleft())
            if nxt == "fade":
                pending = list(buf)
            else:
                for m, f in enumerate(buf):
                    a = smooth((m + 1) / (len(buf) + 1)) if nxt == "black" else 0
                    write(Image.blend(f, black, a) if a else f)
                pending = []
        enc.stdin.close()
        enc.wait()
        if enc.returncode:
            raise RuntimeError(f"ffmpeg failed for {mp4}")
        if music:
            add_music(mp4, Path(music), written / FPS)
        print(f"{self.name}: {written / FPS:.1f} s -> {mp4}")
        return mp4

    def length(self):
        n = 0
        for idx, (item, trans) in enumerate(self.items):
            n += len(item)
            if idx and trans == "fade":
                n -= min(K_FADE, len(self.items[idx - 1][0]) // 3)
        return n


def add_music(mp4, music, seconds):
    """Lay a soundtrack under the film, fading in and out. Bring your own licensed track;
    the repository ships none."""
    tmp = mp4.with_suffix(".music.mp4")
    fade_out = max(0.0, seconds - 2.5)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(mp4), "-i", str(music),
         "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
         "-af", f"afade=t=in:d=0.6,afade=t=out:st={fade_out:.2f}:d=2.5", "-shortest", str(tmp)],
        check=True,
    )
    tmp.replace(mp4)
