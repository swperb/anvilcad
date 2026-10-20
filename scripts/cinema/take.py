"""A take: one run of `anvilcad-agent`, recorded for a film.

`Take` extends the website recorder (scripts/site/recorder.py) with three things a film
needs: a dark studio look for hero shots, a log of every command the script sent (for the
agent overlay), and clips, which mark a stretch of the recording to use as one shot.

Every frame is a real screenshot of the real app, driven only through the harness's JSON
protocol. Nothing here opens a window or touches the mouse.
"""

import math
import sys
from contextlib import contextmanager
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "site"))

from recorder import ROOT, Recorder  # noqa: E402

# Screen size in points. Screenshots are taken at SCALE pixels per point, so a full-screen
# frame has room for the post-production camera to push in without going soft.
SIZE = (1920, 1080)
SCALE = 2.0

# How much slower than the website demos the films run; films.py --pace changes it.
PACE = [1.25]

# The studio look: a near-black viewport, a cool metal body and quiet edges. It is an
# ordinary Lua configuration (docs/SCRIPTING.md), so it changes only the colours.
STUDIO = """
anvil.opt.theme = "dark"
anvil.opt.edges = true
anvil.theme.dark.view_top = "#1a1e26"
anvil.theme.dark.view_bottom = "#040507"
anvil.theme.dark.float_fill = "#00000000"
anvil.theme.body = "#a4adb8"
anvil.theme.edge = "#262b33"
anvil.theme.hover_face = "#c8d0da"
anvil.theme.selected_face = "#ff7a2f"
"""

# The light look, for shots that show the interface the way a user first sees it.
INTERFACE = """
anvil.opt.theme = "light"
anvil.opt.edges = true
"""

# Commands that only read state or take frames; the agent overlay leaves them out.
QUIET = {"state", "project", "hover", "screenshot", "widgets", "find", "document", "frames",
         "wait", "help"}


class Clip:
    """A stretch of a take's timeline, used as one shot.

    `mode` is "hero" (full-bleed, no interface, no pointer) or "ui" (the whole app in a
    framed window, with the pointer and a camera that follows it). The other fields are
    read by post.py.
    """

    def __init__(self, take, start, end, mode, **opts):
        self.take = take
        self.start = start
        self.end = end
        self.mode = mode
        self.opts = opts

    def __len__(self):
        return max(0, self.end - self.start)


class Take(Recorder):
    # Shots are cached so films can share them, which keeps their harness processes
    # running until films.py closes them all at the end of the run.
    open_takes = []

    def __init__(self, name, sample=False, look="studio", open=None, scale=SCALE):
        Take.open_takes.append(self)
        self.log = []  # (timeline frame, command) for every command that acts
        self.logging = True
        self.marks = {}
        # While tracking, each screenshot also records where the body's bounding box lands
        # on screen, so hero shots can keep the part centred and framed as it turns.
        self.track = True
        self.bounds = {}
        super().__init__(f"cinema-{name}", size=SIZE, scale=scale, sample=sample)
        with self.quietly():
            self.cmd(cmd="config", source=STUDIO if look == "studio" else INTERFACE)
            if open:
                self.cmd(cmd="open", path=str(open))
                # Opening a STEP file reports what it imported; close that message.
                if self.cmd(cmd="state")["state"].get("message"):
                    self.cmd(cmd="key", key="Enter")
                    if self.cmd(cmd="state")["state"].get("message"):
                        self.cmd(cmd="click", label="OK", role="Button")
            self.cmd(cmd="click", label="Zoom to Fit")
            self.cmd(cmd="hover", x=SIZE[0] - 30, y=SIZE[1] - 60)
        self.cursor = None
        self.timeline.clear()
        self.mark_at = 0
        self.snap()

    def cmd(self, **c):
        if self.logging and c.get("cmd") not in QUIET:
            self.log.append((len(self.timeline), c))
        return super().cmd(**c)

    @contextmanager
    def quietly(self):
        """Run commands without recording them in the agent log, for setup the film
        does not show."""
        was = self.logging
        self.logging = False
        try:
            yield
        finally:
            self.logging = was

    def look(self, look):
        """Switch between the studio and interface colours mid-take."""
        with self.quietly():
            self.cmd(cmd="config", source=STUDIO if look == "studio" else INTERFACE)
        self.snap()

    def begin(self):
        """Start a new clip at the current point in the recording."""
        self.mark_at = len(self.timeline)

    def clip(self, mode, **opts):
        """End the clip begun with `begin` and return it."""
        c = Clip(self, self.mark_at, len(self.timeline), mode, **opts)
        self.mark_at = len(self.timeline)
        return c

    def mark(self, name):
        """Remember this point in the recording, so a film can cut exactly here."""
        self.marks[name] = len(self.timeline)

    # -- pacing -------------------------------------------------------------------------
    # Every pause, pointer move and orbit is stretched by PACE, so the whole film can be
    # slowed or sped up from one setting (films.py --pace).

    def hold(self, seconds):
        super().hold(seconds * PACE[0])

    def move_to(self, pos, seconds=None):
        # After a hero shot the pointer is hidden; bring it back in from below right.
        if self.cursor is None:
            self.cursor = (min(SIZE[0] - 20, pos[0] + 260), min(SIZE[1] - 20, pos[1] + 180))
        if seconds is None:
            seconds = min(0.75, 0.22 + math.dist(self.cursor, pos) / 1800)
        super().move_to(pos, seconds * PACE[0])

    def orbit(self, dx, dy, steps=60, **kw):
        super().orbit(dx, dy, steps=max(1, int(round(steps * PACE[0]))), **kw)

    # -- framing ------------------------------------------------------------------------

    def snap(self, hold=0.0):
        path = super().snap(hold)
        if self.track:
            self.bounds[path] = self.screen_bounds()
        return path

    def screen_bounds(self):
        """The screen rectangle (points) that holds the body's bounding box, or None."""
        was = self.logging
        self.logging = False
        try:
            body = self.cmd(cmd="state")["state"].get("body") or {}
            b = body.get("bounds")
            if not b or body.get("empty"):
                return None
            lo, hi = b["min"], b["max"]
            pts = [self.cmd(cmd="project", model=[x, y, z])["at"]
                   for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]
        except (RuntimeError, KeyError, TypeError):
            return None
        finally:
            self.logging = was
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        return (min(xs), min(ys), max(xs), max(ys))

    def hide_cursor(self):
        self.cursor = None

    def show_cursor(self, at=None):
        self.cursor = at or (SIZE[0] * 0.62, SIZE[1] * 0.55)


__all__ = ["ROOT", "SIZE", "SCALE", "Take", "Clip"]
