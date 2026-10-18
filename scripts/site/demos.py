"""The demos shown on the website. Run from the repository root:

    cargo build --release -p anvil-app --bin anvilcad-agent
    python3 scripts/site/demos.py            # every demo
    python3 scripts/site/demos.py build      # just one

Videos and posters are written to docs/media/.
"""

import sys

from recorder import ROOT, Recorder

OUT = ROOT / "docs" / "media"


def sketch(r):
    """Sketch a plate with a hole on the Front plane and dimension it."""
    r.click(label="Front Plane", hold=0.3)
    r.click(label="Sketch", role="Button", hold=0.3)
    r.click(label="Sketch", index=1, hold=0.4)
    r.zoom(2, hold=0.3)
    r.click(label="Rectangle", hold=0.2)
    r.click(sketch=[-30, -18], hold=0.1)
    r.click(sketch=[30, 18], hold=0.4)
    r.click(label="Circle", hold=0.2)
    r.click(sketch=[0, 0], hold=0.1)
    r.click(sketch=[9, 0], hold=0.4)
    r.key("Escape")
    r.click(label="Dimension", hold=0.2)
    r.click(sketch=[-10, -18], hold=0.1)
    r.click(sketch=[-10, -26], hold=0.6)
    r.click(sketch=[30, 6], hold=0.1)
    r.click(sketch=[38, 6], hold=0.6)
    r.click(sketch=[9, 0], hold=0.1)
    r.click(sketch=[16, 12], hold=0.6)
    r.key("Escape", hold=0.8)


def build():
    r = Recorder("build")
    r.mark("sketch")
    sketch(r)
    r.mark("build")
    r.click(label="Exit Sketch", hold=0.4)
    r.click(label="Features", role="Button", hold=0.3)
    r.click(label="Extruded Boss/Base", hold=0.6)
    r.triple_click(label="Depth (mm)", offset=[70, 0])
    r.type("16", hold=0.6)
    r.click(label="OK", role="Button", hold=0.4)
    r.click(x=1400, y=840, hold=0.2)
    r.key("7", modifiers=["cmd"], hold=0.2)
    r.click(label="Zoom to Fit", hold=0.6)
    r.mark("turn")
    r.orbit(150, 0, steps=75, hold=1.2)
    s = r.state()
    print("build:", s["status"], s["body"]["volume"])
    r.render(OUT, gif=True)


def plate_quietly(r):
    """Build the 60 x 36 x 16 plate from the build demo without recording it."""
    for c in [
        {"cmd": "click", "label": "Front Plane"},
        {"cmd": "click", "label": "Sketch", "role": "Button"},
        {"cmd": "click", "label": "Sketch", "index": 1},
        {"cmd": "click", "label": "Rectangle"},
        {"cmd": "click", "sketch": [-30, -18]},
        {"cmd": "click", "sketch": [30, 18]},
        {"cmd": "click", "label": "Circle"},
        {"cmd": "click", "sketch": [0, 0]},
        {"cmd": "click", "sketch": [9, 0]},
        {"cmd": "key", "key": "Escape"},
        {"cmd": "click", "label": "Exit Sketch"},
        {"cmd": "click", "label": "Features", "role": "Button"},
        {"cmd": "click", "label": "Extruded Boss/Base"},
        {"cmd": "click", "label": "Depth (mm)", "offset": [70, 0]},
        {"cmd": "key", "key": "A", "modifiers": ["cmd"]},
        {"cmd": "type", "text": "16"},
        {"cmd": "click", "label": "OK", "role": "Button"},
        {"cmd": "click", "x": 1400, "y": 840},
        {"cmd": "key", "key": "7", "modifiers": ["cmd"]},
        {"cmd": "click", "label": "Zoom to Fit"},
    ]:
        r.cmd(**c)
    # Start the recording on the finished plate, not the empty part before it.
    r.timeline.clear()
    r.snap(0.6)


def refine():
    """Round the four corners of the plate, then shell it."""
    r = Recorder("refine")
    plate_quietly(r)
    r.click(label="Fillet", role="Button", hold=0.5)
    for p in ([30, 18, 8], [30, -18, 8], [-30, -18, 8], [-30, 18, 8]):
        r.click(model=p, hold=0.25)
    r.triple_click(label="Radius (mm)", offset=[70, 0])
    r.type("6", hold=0.8)
    r.click(label="OK", role="Button", hold=0.6)
    r.click(x=1400, y=840, hold=0.4)
    r.orbit(-120, 0, steps=60, hold=1.0)
    s = r.state()
    print("refine:", s["status"], s["body"]["volume"], [f["name"] for f in s["features"]])
    r.render(OUT)


def params():
    """Change two global variables on the sample part and watch it rebuild."""
    r = Recorder("params", sample=True)
    r.click(label="Tools", hold=0.3)
    r.click(label="Equations...", hold=0.8)
    for name, value in (("width", "110"), ("boss_d", "34"), ("thickness", "6")):
        cur = r.state()["variables"][name]
        r.triple_click(label=f"{cur:g}", role="TextInput")
        r.type(value, hold=0.2, whole=True)
        r.key("Enter", hold=1.0)
    r.click(label="Close window", hold=0.4)
    r.orbit(120, 0, steps=60, hold=1.0)
    s = r.state()
    print("params:", s["status"], s["variables"], s["body"]["volume"])
    r.render(OUT)


def showcase():
    """A slow turn around the sample part."""
    r = Recorder("showcase", sample=True)
    r.cmd(cmd="click", label="Zoom to Fit")
    r.cursor = None
    r.timeline.clear()
    r.snap(0.3)
    # Sway one way and back so the clip loops without a jump.
    r.orbit(110, 0, steps=70)
    r.orbit(-220, 0, steps=140)
    r.orbit(110, 0, steps=70)
    r.render(OUT, crop=(270, 144, 1440, 868), gif=True)


def evaluate():
    """Open the mass properties of the sample part."""
    r = Recorder("evaluate", sample=True)
    r.click(label="Evaluate", role="Button", hold=0.5)
    r.click(label="Mass Properties", hold=2.5)
    r.render(OUT)


DEMOS = {"build": build, "refine": refine, "params": params, "showcase": showcase,
         "evaluate": evaluate}

if __name__ == "__main__":
    for name in sys.argv[1:] or DEMOS:
        DEMOS[name]()
