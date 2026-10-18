"""Record AnvilCAD demos for the website with the headless harness.

Each demo drives `anvilcad-agent` over its JSON protocol, takes a screenshot after every
step, and records where each click lands. The frames are then put together into a video:
the harness draws no mouse pointer, so this script draws one, moves it between clicks and
marks each click with a ring. Nothing here touches the real mouse or opens a window.

Usage: python3 scripts/site/recorder.py [demo ...]   (see demos.py for the names)
"""

import json
import math
import shutil
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
AGENT = ROOT / "target" / "release" / "anvilcad-agent"
WORK = ROOT / "target" / "site"
FPS = 30
ACCENT = (234, 88, 12)


class Recorder:
    def __init__(self, name, size=(1440, 900), scale=1.0, sample=False):
        self.name = name
        self.dir = WORK / name
        shutil.rmtree(self.dir, ignore_errors=True)
        self.dir.mkdir(parents=True)
        args = [str(AGENT), "--size", f"{size[0]}x{size[1]}", "--scale", str(scale)]
        if sample:
            args.append("--sample")
        self.proc = subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, cwd=ROOT
        )
        self.scale = scale
        self.n = 0
        # Each entry is (image path, cursor position or None, click ring age or None).
        self.timeline = []
        self.cursor = (size[0] * 0.62, size[1] * 0.55)
        self.image = None
        self.snap(hold=0.6)

    # -- protocol -------------------------------------------------------------------

    def cmd(self, **c):
        self.proc.stdin.write(json.dumps(c) + "\n")
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        if not reply.get("ok"):
            raise RuntimeError(f"{c} failed: {reply.get('error')}")
        return reply

    def state(self):
        return self.cmd(cmd="state")["state"]

    def where(self, **target):
        return tuple(self.cmd(cmd="project", **target)["at"])

    # -- frames ---------------------------------------------------------------------

    def snap(self, hold=0.0):
        """Take a screenshot and show it for `hold` seconds."""
        path = self.dir / f"s{self.n:04}.png"
        self.n += 1
        self.cmd(cmd="screenshot", path=str(path))
        self.image = path
        self.hold(hold)
        return path

    def mark(self, name):
        """Note the current time in the video, for chapter links on the page."""
        print(f"{self.name}: {name} at {len(self.timeline) / FPS:.2f} s")

    def hold(self, seconds):
        for _ in range(int(round(seconds * FPS))):
            self.timeline.append((self.image, self.cursor, None))

    def move_to(self, pos, seconds=None):
        start = self.cursor
        dist = math.dist(start, pos)
        if seconds is None:
            seconds = min(0.75, 0.22 + dist / 1800)
        frames = max(1, int(round(seconds * FPS)))
        for i in range(1, frames + 1):
            t = i / frames
            e = t * t * (3 - 2 * t)
            # A slight arc reads as a hand movement rather than a straight slide.
            bow = math.sin(math.pi * t) * min(40.0, dist * 0.08)
            x = start[0] + (pos[0] - start[0]) * e
            y = start[1] + (pos[1] - start[1]) * e - bow
            self.cursor = (x, y)
            self.timeline.append((self.image, self.cursor, None))
        self.cursor = pos

    def ring(self):
        for age in range(int(0.35 * FPS)):
            self.timeline.append((self.image, self.cursor, age))

    # -- actions --------------------------------------------------------------------

    def click(self, hold=0.5, button="left", move=None, **target):
        pos = self.where(**target)
        self.move_to(pos, move)
        self.cmd(cmd="hover", **target)
        self.snap(0.12)
        self.cmd(cmd="click", button=button, **target)
        self.snap()
        self.ring()
        self.hold(hold)
        return pos

    def triple_click(self, hold=0.2, **target):
        """Select all the text in a field: click it, then Cmd+A."""
        self.click(hold=0.1, **target)
        self.cmd(cmd="key", key="A", modifiers=["cmd"])
        self.snap(hold)

    def double_click(self, hold=0.5, **target):
        pos = self.where(**target)
        self.move_to(pos)
        self.cmd(cmd="double_click", **target)
        self.snap()
        self.ring()
        self.hold(hold)

    def key(self, key, modifiers=(), hold=0.4):
        self.cmd(cmd="key", key=key, modifiers=list(modifiers))
        self.snap(hold)

    def type(self, text, per_char=0.07, hold=0.3, whole=False):
        """Type text a character at a time, or all at once with `whole` (for fields
        that rebuild on every keystroke, where a partial value would flash an error)."""
        for ch in [text] if whole else text:
            self.cmd(cmd="type", text=ch)
            self.snap(per_char)
        self.hold(hold)

    def zoom(self, ticks, hold=0.0):
        """Zoom in (positive) or out at the viewport centre, a frame per wheel tick."""
        for _ in range(abs(ticks)):
            self.cmd(cmd="scroll", dy=60 if ticks > 0 else -60)
            self.snap(1 / FPS)
        self.hold(hold)

    def orbit(self, dx, dy, steps=60, at=None, hold=0.0, show_cursor=False):
        """Turn the view by middle-dragging in small steps, one frame per step."""
        if at is None:
            # Drag from an empty corner of the viewport, so the pointer never rests on
            # the part and lights up a face under it.
            vp = self.state()["viewport"]
            at = (vp[2] - 40, vp[3] - 30)
        if not show_cursor:
            self.cursor = None
        for i in range(steps):
            # Ease in and out so the turn starts and stops smoothly.
            a = 0.5 - 0.5 * math.cos(math.pi * i / steps)
            b = 0.5 - 0.5 * math.cos(math.pi * (i + 1) / steps)
            sx, sy = dx * (b - a), dy * (b - a)
            self.cmd(
                cmd="drag",
                **{"from": {"x": at[0], "y": at[1]}, "to": {"x": at[0] + sx, "y": at[1] + sy}},
                button="middle",
                steps=1,
            )
            self.snap()
            self.timeline.append((self.image, self.cursor, None))
        self.hold(hold)

    def close(self):
        try:
            self.cmd(cmd="quit")
        except Exception:
            pass
        self.proc.wait(timeout=10)

    # -- output ---------------------------------------------------------------------

    def render(self, out_dir, crop=None, width=None, speed=1.0, gif=False):
        """Composite the cursor onto the frames and encode an MP4 and a poster JPEG."""
        self.close()
        frames = self.dir / "frames"
        frames.mkdir()
        cache = {}
        step = max(1, int(round(speed)))
        timeline = self.timeline[::step]
        for i, (path, cursor, ring) in enumerate(timeline):
            if path not in cache:
                cache.clear()
                cache[path] = Image.open(path).convert("RGB")
            img = cache[path].copy()
            draw = ImageDraw.Draw(img)
            s = self.scale
            if cursor is not None:
                x, y = cursor[0] * s, cursor[1] * s
                if ring is not None:
                    t = ring / (0.35 * FPS)
                    r = (6 + 18 * t) * s
                    col = tuple(int(c * (1 - t) + 255 * t) for c in ACCENT)
                    draw.ellipse([x - r, y - r, x + r, y + r], outline=col, width=int(3 * s))
                draw_pointer(draw, x, y, s)
            if crop:
                img = img.crop([int(v * s) for v in crop])
            if width and img.width != width:
                h = int(round(img.height * width / img.width / 2)) * 2
                img = img.resize((width, h), Image.LANCZOS)
            img.save(frames / f"f{i:05}.png")
        out_dir.mkdir(parents=True, exist_ok=True)
        mp4 = out_dir / f"{self.name}.mp4"
        subprocess.run(
            [
                "ffmpeg", "-y", "-loglevel", "error", "-framerate", str(FPS),
                "-i", str(frames / "f%05d.png"), "-c:v", "libx264", "-preset", "slow",
                "-crf", "24", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(mp4),
            ],
            check=True,
        )
        poster = Image.open(sorted(frames.glob("f*.png"))[-1]).convert("RGB")
        poster.save(out_dir / f"{self.name}-poster.jpg", quality=84)
        if gif:
            subprocess.run(
                [
                    "gifski", "--quiet", "--fps", "15", "--width", "720", "--quality", "80",
                    "-o", str(out_dir / f"{self.name}.gif"),
                    *[str(p) for p in sorted(frames.glob("f*.png"))[::2]],
                ],
                check=True,
            )
        return mp4


def draw_pointer(draw, x, y, s):
    pts = [(0, 0), (0, 17), (4.2, 13.2), (7.2, 19.8), (10, 18.6), (7.1, 12.2), (12.6, 12.2)]
    poly = [(x + px * s * 1.1, y + py * s * 1.1) for px, py in pts]
    draw.polygon(poly, fill=(255, 255, 255), outline=(20, 20, 20), width=max(1, int(1.6 * s)))
