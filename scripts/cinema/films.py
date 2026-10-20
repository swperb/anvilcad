"""AnvilCAD's product films, driven end to end by anvilcad-agent.

Run from the repository root:

    cargo build --release -p anvil-app --bin anvilcad-agent
    python3 scripts/cinema/films.py                 # every film
    python3 scripts/cinema/films.py trailer         # one film
    python3 scripts/cinema/films.py --list          # what there is
    python3 scripts/cinema/films.py fillet --draft  # quick half-size preview

Films are written to target/cinema/ as MP4s with a poster frame. See README.md in this
folder for the options, how a film is put together and how to add one.

Every frame is a real screenshot of AnvilCAD, and every action in it is a command the
script sends to the headless harness. Captions say only what the README's "What works
today" says; check them against it when the app changes.
"""

import argparse
import functools
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "site"))

import post  # noqa: E402
from demos import plate_quietly  # noqa: E402
from post import Card, Film  # noqa: E402
from post import ease_in_out as ease  # noqa: E402
from post import smooth as smooth_step  # noqa: E402
from take import PACE, ROOT, SCALE, Clip, Take  # noqa: E402

OUT = ROOT / "target" / "cinema"
PARTS = OUT / "parts"
NIST = ROOT / "crates" / "anvil-step" / "tests" / "data" / "nist_ctc_03_asme1_rc.stp"
REPO = "github.com/swperb/anvilcad"
NIST_NOTE = "STEP model: NIST MBE PMI test case (free of restrictions). Use does not imply endorsement by NIST."

# Takes are recorded at SCALE; --draft records at 1x and renders at half size.
TAKE_SCALE = [SCALE]


def take(name, **kw):
    return Take(name, scale=TAKE_SCALE[0], **kw)


def sub(clip, a=0.0, b=None, **opts):
    """A clip cut from another: seconds `a` to `b` of it (b negative counts from the end),
    with extra or replaced options."""
    fps = post.FPS
    start = clip.start + int(round(a * fps))
    end = clip.end if b is None else (clip.start + int(round(b * fps)) if b >= 0 else clip.end + int(round(b * fps)))
    return Clip(clip.take, start, max(start, end), clip.mode, **{**clip.opts, **opts})


def deselect(r):
    """Click empty space in the viewport and settle on the isometric view, off camera."""
    with r.quietly():
        r.cmd(cmd="click", x=1880, y=1000)
        r.cmd(cmd="key", key="7", modifiers=["cmd"])
        r.cmd(cmd="click", label="Zoom to Fit")
        r.cmd(cmd="hover", x=1900, y=1060)
    r.hide_cursor()
    r.snap()


# -- shots ----------------------------------------------------------------------------
# Each shot records one take and returns its clips. They are cached, so films that share
# a shot record it once per run.


@functools.cache
def cold_open():
    r = take("cold-open", sample=True)
    r.begin()
    r.orbit(80, -6, steps=165, hold=0.2)
    return {"hero": r.clip("hero", push=(0.95, 1.1))}


@functools.cache
def sketch_to_solid():
    """A 60 x 36 mm plate with an 18 mm hole: the rectangle and the circle are one sketch,
    and extruding it leaves the circle as a hole. The same steps as the website's build
    demo (scripts/site/demos.py), with marks so films can cut between its stages."""
    r = take("sketch-to-solid")
    r.show_cursor()
    r.begin()
    r.click(label="Front Plane", hold=0.3)
    r.click(label="Sketch", role="Button", hold=0.3)
    r.click(label="Sketch", index=1, hold=0.4)
    r.zoom(2, hold=0.3)
    r.mark("rectangle")
    r.click(label="Rectangle", hold=0.2)
    r.click(sketch=[-30, -18], hold=0.1)
    r.click(sketch=[30, 18], hold=0.4)
    r.click(label="Circle", hold=0.2)
    r.click(sketch=[0, 0], hold=0.1)
    r.click(sketch=[9, 0], hold=0.4)
    r.key("Escape")
    r.mark("dimension")
    r.click(label="Dimension", hold=0.2)
    r.click(sketch=[-10, -18], hold=0.1)
    r.click(sketch=[-10, -26], hold=0.6)
    r.click(sketch=[30, 6], hold=0.1)
    r.click(sketch=[38, 6], hold=0.6)
    r.mark("two dimensions")
    r.click(sketch=[9, 0], hold=0.1)
    r.click(sketch=[16, 12], hold=0.6)
    r.key("Escape", hold=0.8)
    r.mark("extrude")
    r.click(label="Exit Sketch", hold=0.4)
    r.click(label="Features", role="Button", hold=0.3)
    r.click(label="Extruded Boss/Base", hold=0.6)
    r.triple_click(label="Depth (mm)", offset=[70, 0])
    r.type("16", hold=0.6)
    r.click(label="OK", role="Button", hold=0.6)
    ui = r.clip("ui", zoom=1.4, caption=("Sketch it. Dimension it.", "One sketch: the circle inside becomes a hole"))
    deselect(r)
    r.begin()
    r.orbit(90, -8, steps=150, hold=0.3)
    hero = r.clip("hero", push=(1.0, 1.12), caption=("Extruded Boss/Base", "Blind, 16 mm"))
    report(r, "sketch-to-solid")
    return {"ui": ui, "hero": hero, "marks": r.marks}


def span(clip, start, end, **opts):
    """The part of a clip between two timeline marks, with extra or replaced options."""
    return Clip(clip.take, max(clip.start, start), min(clip.end, end), clip.mode, **{**clip.opts, **opts})


@functools.cache
def fillet():
    r = take("fillet")
    plate_quietly(r)
    r.timeline.clear()
    r.show_cursor()
    r.snap(0.4)
    r.begin()
    r.click(label="Fillet", role="Button", hold=0.5)
    for p in ([30, 18, 8], [30, -18, 8], [-30, -18, 8], [-30, 18, 8]):
        r.click(model=p, hold=0.25)
    r.triple_click(label="Radius (mm)", offset=[70, 0])
    r.type("6", hold=0.8)
    r.click(label="OK", role="Button", hold=0.6)
    ui = r.clip("ui", zoom=1.35, caption=("Fillets, built exactly", "Exact rounds, not facets"))
    deselect(r)
    r.begin()
    r.orbit(-90, -6, steps=150, hold=0.3)
    hero = r.clip("hero", push=(1.0, 1.12))
    report(r, "fillet")
    return {"ui": ui, "hero": hero}


@functools.cache
def equations():
    r = take("equations", sample=True)
    r.show_cursor()
    r.begin()
    r.click(label="Tools", hold=0.3)
    r.click(label="Equations...", hold=0.8)
    for name, value in (("width", "110"), ("boss_d", "34")):
        cur = r.state()["variables"][name]
        r.triple_click(label=f"{cur:g}", role="TextInput")
        r.type(value, hold=0.2, whole=True)
        r.key("Enter", hold=1.0)
    r.click(label="Close window", hold=0.5)
    ui = r.clip("ui", zoom=1.4, caption=("Change a number. The part follows.", "Global variables and equations"))
    # The same idea as a hero shot: the width grows in steps while the camera drifts.
    deselect(r)
    r.begin()
    r.hold(0.4)
    for w in (110, 98, 86, 80, 90, 100, 120, 130):
        r.cmd(cmd="lua", code=f'anvil.var("width", {w})')
        r.snap(0.34)
    r.hold(0.4)
    hero = r.clip("hero", push=(1.0, 1.05), dissolve=4, caption=("width = 80 … 130", "Every feature rebuilds from the equations"))
    report(r, "equations")
    return {"ui": ui, "hero": hero}


@functools.cache
def section():
    r = take("section", sample=True)
    r.show_cursor()
    r.begin()
    r.click(label="Evaluate", role="Button", hold=0.4)
    r.click(label="Section View", role="Button", hold=0.6)
    # Drag the panel down over the feature tree, so the viewport is clear for the cut.
    win = r.cmd(cmd="find", label="Section View", role="Window")["widget"]["rect"]
    grab = ((win[0] + win[2]) / 2, win[1] + 27)
    drop = (200.0, 700.0)
    r.move_to(grab)
    steps = 18
    for i in range(steps):
        a = ease(i / steps)
        b = ease((i + 1) / steps)
        p0 = (grab[0] + (drop[0] - grab[0]) * a, grab[1] + (drop[1] - grab[1]) * a)
        p1 = (grab[0] + (drop[0] - grab[0]) * b, grab[1] + (drop[1] - grab[1]) * b)
        r.cmd(cmd="drag", **{"from": {"x": p0[0], "y": p0[1]}, "to": {"x": p1[0], "y": p1[1]}}, steps=2)
        r.cursor = p1
        r.snap(1 / post.FPS)
    r.hold(0.5)
    ui = r.clip("ui", zoom=1.3, caption=("Section view", "Cut the view open; the part never changes"))
    with r.quietly():
        r.cmd(cmd="hover", x=1900, y=1060)
    r.hide_cursor()
    r.snap()
    r.begin()
    field = {"label": "Offset (mm)", "offset": [65, 0]}
    # From a thin slice near the front to a cut through the middle of the boss.
    for i in range(31):
        v = -16 + 16 * smooth_step(i / 30)
        r.cmd(cmd="click", **field)
        r.cmd(cmd="key", key="A", modifiers=["cmd"])
        r.cmd(cmd="type", text=f"{v:.1f}")
        r.cmd(cmd="key", key="Enter")
        r.snap(1 / post.FPS * 2)
    r.hold(0.3)
    r.orbit(45, -4, steps=75, hold=0.3)
    # The panel now sits over the feature tree; keep just clear of its right edge.
    hero = r.clip("hero", region=(400.0, 190.0, 1920.0, 1048.0), push=(1.0, 1.08), dissolve=0,
                  caption=("Section view", "Cut the view open; the part never changes"))
    report(r, "section")
    return {"ui": ui, "hero": hero}


@functools.cache
def step_import():
    r = take("step-import")
    r.show_cursor()
    r.begin()
    r.cmd(cmd="file_dialog", path=str(NIST))  # answers the Open dialog the next click opens
    r.click(label="File", hold=0.3)
    r.click(label="Open...", hold=1.2)
    r.click(label="OK", role="Button", hold=0.4)
    r.click(label="Zoom to Fit", hold=0.8)
    ui = r.clip("ui", zoom=1.3, caption=("Open a STEP file", "AP203, AP214 and AP242"))
    s = r.state()
    exact = bool(s["body"].get("exact"))
    deselect(r)
    r.begin()
    r.orbit(110, -8, steps=180, hold=0.3)
    cap = ("STEP in. Exact solids out.", "A NIST test part, imported as an exact B-rep") if exact else \
        ("STEP in", "A NIST test part, imported and watertight")
    hero = r.clip("hero", push=(1.0, 1.1), caption=cap)
    report(r, "step-import")
    return {"ui": ui, "hero": hero}


@functools.cache
def mass():
    r = take("mass", sample=True)
    r.show_cursor()
    r.begin()
    r.click(label="Evaluate", role="Button", hold=0.5)
    r.click(label="Mass Properties", hold=2.6)
    ui = r.clip("ui", zoom=1.6, caption=("Mass properties", "Mass, centre of mass and inertia, from the material"))
    report(r, "mass")
    return {"ui": ui}


@functools.cache
def check_design():
    r = take("check-design")
    # A plate with walls too thin to machine and a small, deep hole.
    with r.quietly():
        r.cmd(cmd="lua", code=(
            "anvil.new{force=true} anvil.material('6061 Alloy')"
            " local s = anvil.sketch{plane='top', rects={{-30,-20,30,20}}}"
            " local b = anvil.extrude{sketch=s, depth=14}"
            " local p = anvil.sketch{plane={on=b, face='end'}, rects={{-29.4,-19.4,29.4,19.4}}}"
            " anvil.extrude{sketch=p, depth=11, cut=true}"
            " local h = anvil.sketch{plane={on=b, face='end'}, circles={{0,0,0.9}}}"
            " anvil.extrude{sketch=h, depth='through', cut=true}"))
        r.cmd(cmd="key", key="7", modifiers=["cmd"])
        r.cmd(cmd="click", label="Zoom to Fit")
    r.timeline.clear()
    r.show_cursor()
    r.snap(0.4)
    r.begin()
    r.click(label="Evaluate", role="Button", hold=0.4)
    r.click(label="Check Design", role="Button", hold=0.6)
    r.click(label="Run", role="Button", hold=1.0)
    findings = [w for w in r.cmd(cmd="widgets")["widgets"]
                if w["role"] == "Button" and ("wall" in (w.get("label") or "").lower()
                                              or "hole" in (w.get("label") or "").lower())]
    if findings:
        r.click(x=findings[0]["center"][0], y=findings[0]["center"][1], hold=1.6)
    ui = r.clip("ui", zoom=1.45, caption=("Check it before you send it", "Design checks for CNC machining and sheet metal"))
    report(r, "check-design")
    return {"ui": ui}


@functools.cache
def assembly():
    PARTS.mkdir(parents=True, exist_ok=True)
    r = take("assembly")
    with r.quietly():
        for name, (w, d, h) in {"base": (40, 30, 10), "cube": (10, 10, 10)}.items():
            r.cmd(cmd="lua", code=(
                f"anvil.new{{force=true}} local s = anvil.sketch{{plane='front', rects={{{{0,0,{w},{d}}}}}}}"
                f" anvil.extrude{{sketch=s, depth={h}}} anvil.save('{(PARTS / name).as_posix()}.anvprt')"))
        r.cmd(cmd="command", name="New Assembly")
        for name in ("base", "cube"):
            r.cmd(cmd="file_dialog", path=str(PARTS / f"{name}.anvprt"))
            r.cmd(cmd="command", name="Insert Component")
        r.cmd(cmd="click", label="Zoom to Fit")
    r.timeline.clear()
    r.show_cursor()
    r.snap(0.4)

    def cube():
        return r.state()["assembly"]["components"][1]["origin"]

    def mate(kind, base_pt, cube_off, align="Aligned", value=None):
        r.click(label="Mate", role="Button", hold=0.3)
        o = cube()
        r.click(model=base_pt, hold=0.25)
        r.click(model=[o[0] + cube_off[0], o[1] + cube_off[1], o[2] + cube_off[2]], hold=0.3)
        if kind != "Coincident":
            r.click(role="ComboBox", x=80, y=298, hold=0.3)
            r.click(label=kind, hold=0.3)
        if value is not None:
            r.triple_click(label="Value", offset=[68, 0])
            r.type(value, hold=0.3, whole=True)
        r.click(label=align, role="Button", hold=0.3)
        r.click(label="OK", role="Button", hold=0.9)

    r.begin()
    mate("Distance", [20, 30, 5], [5, 10, 5], value="10")   # lift the cube's top 10 mm above the base's
    mate("Coincident", [20, 15, 10], [5, 5, 10])            # front faces flush
    r.mark("last mate")
    mate("Coincident", [40, 15, 5], [10, 5, 5])             # right faces flush
    ui = r.clip("ui", zoom=1.35, caption=("Assemblies with mates", "Solved live, with what each part can still do"))
    deselect(r)
    r.begin()
    r.orbit(80, -6, steps=140, hold=0.3)
    hero = r.clip("hero", push=(1.0, 1.12))
    report(r, "assembly")
    return {"ui": ui, "hero": hero}


def report(r, name):
    s = r.state()
    body = s.get("body") or {}
    asm = s.get("assembly")
    extra = ""
    if asm:
        extra = " " + ", ".join(f"{c['name']} {c['definition']} at {[round(v, 3) for v in c['origin']]}"
                                for c in asm["components"])
    print(f"{name}: {s['status']}; volume {body.get('volume')}; exact {body.get('exact')}{extra}")


# -- films ----------------------------------------------------------------------------

def end_card(note=None, seconds=3.6):
    return Card(logo=True, sub="Free and open source  ·  MIT or Apache-2.0", foot=REPO, note=note, seconds=seconds)


def spot(name, kicker, head, clips, note=None):
    items = [(Card(kicker=kicker, head=head, seconds=2.2), "black")]
    for i, c in enumerate(clips):
        items.append((c, "fade" if i == 0 else "cut"))
    items.append((end_card(note, 3.0), "fade"))
    return Film(name, items)


def trailer():
    co, sk, fi, eq = cold_open(), sketch_to_solid(), fillet(), equations()
    se, st, asm = section(), step_import(), assembly()
    m, am = sk["marks"], asm["ui"].take.marks
    return Film("trailer", [
        (co["hero"], "black"),
        (Card(kicker="Introducing", logo=True, sub="Parametric CAD. Free and open source.", seconds=3.4), "fade"),
        (span(sk["ui"], m["rectangle"], m["dimension"],
              caption=("Sketch it.", "A rectangle and a circle, in one sketch")), "fade"),
        (span(sk["ui"], m["dimension"], m["two dimensions"],
              caption=("Dimension it.", "Smart dimensions that infer what you meant")), "cut"),
        (span(sk["ui"], m["extrude"], sk["ui"].end,
              caption=("Extrude it.", "The circle inside the rectangle becomes a hole")), "cut"),
        (sub(sk["hero"], 0, 3.6, caption=None), "cut"),
        (fi["ui"], "fade"),
        (sub(fi["hero"], 0, 3.6), "cut"),
        (eq["hero"], "fade"),
        (se["ui"], "fade"),
        (sub(se["hero"], 0, 4.6, caption=None), "cut"),
        (st["ui"], "fade"),
        (sub(st["hero"], 0, 4.4), "cut"),
        (span(asm["ui"], am["last mate"], asm["ui"].end), "fade"),
        (sub(asm["hero"], 0, 3.4), "cut"),
        (Card(kicker="Built for agents", head="No window. No mouse.",
              sub="Every frame you just saw was driven by a script.", seconds=3.4), "black"),
        (sub(sk["ui"], 0, 5.6, agent=True, caption=("Driven by an agent", "JSON commands to anvilcad-agent, headless")), "fade"),
        (end_card(NIST_NOTE, 4.2), "black"),
    ])


FILMS = {
    "trailer": trailer,
    "sketch": lambda: spot("sketch", "Modelling", "Sketch to solid",
                           [sketch_to_solid()["ui"], sketch_to_solid()["hero"]]),
    "fillet": lambda: spot("fillet", "Features", "Exact fillets", [fillet()["ui"], fillet()["hero"]]),
    "equations": lambda: spot("equations", "Parameters", "Design by numbers",
                              [equations()["ui"], equations()["hero"]]),
    "section": lambda: spot("section", "Evaluate", "Look inside", [section()["ui"], section()["hero"]]),
    "step": lambda: spot("step", "Interoperability", "STEP, exactly",
                         [step_import()["ui"], step_import()["hero"]], NIST_NOTE),
    "mass": lambda: spot("mass", "Evaluate", "Know what it weighs", [mass()["ui"]]),
    "check-design": lambda: spot("check-design", "Manufacturing", "Check design", [check_design()["ui"]]),
    "assembly": lambda: spot("assembly", "Assemblies", "Put it together", [assembly()["ui"], assembly()["hero"]]),
    "agent": lambda: spot("agent", "Built for agents", "No window. No mouse.",
                          [sub(sketch_to_solid()["ui"], agent=True,
                               caption=("Driven by an agent", "Every click is a JSON command to anvilcad-agent"))]),
}


def main():
    ap = argparse.ArgumentParser(description="Record AnvilCAD's product films with anvilcad-agent.")
    ap.add_argument("films", nargs="*", help="films to make (default: all)")
    ap.add_argument("--list", action="store_true", help="list the films and exit")
    ap.add_argument("--draft", action="store_true", help="1x takes, half-size output, fast encode")
    ap.add_argument("--letterbox", action="store_true", help="2.39:1 bars")
    ap.add_argument("--grain", action="store_true", help="a little film grain")
    ap.add_argument("--pace", type=float, default=PACE[0],
                    help=f"how much slower than the website demos to run (default {PACE[0]})")
    ap.add_argument("--music", help="an audio file you have the rights to, laid under each film")
    ap.add_argument("--out", default=str(OUT), help=f"output folder (default {OUT})")
    args = ap.parse_args()
    if args.list:
        for name in FILMS:
            print(name)
        return
    unknown = [f for f in args.films if f not in FILMS]
    if unknown:
        sys.exit(f"unknown film(s): {', '.join(unknown)}; try --list")
    post.configure(draft=args.draft, grain=args.grain, letterbox=args.letterbox)
    PACE[0] = args.pace
    if args.draft:
        TAKE_SCALE[0] = 1.0
    if not (ROOT / "target" / "release" / "anvilcad-agent").exists():
        sys.exit("build the harness first: cargo build --release -p anvil-app --bin anvilcad-agent")
    try:
        for name in args.films or FILMS:
            FILMS[name]().render(Path(args.out), music=args.music)
    finally:
        for t in Take.open_takes:
            t.close()


if __name__ == "__main__":
    main()
