# Product films

Scripts that record AnvilCAD's product films, the trailer and one short spot per key feature,
by driving the real app through the headless harness (`anvilcad-agent`, see
[docs/AGENT_HARNESS.md](../../docs/AGENT_HARNESS.md)). Every frame is a real screenshot and
every action is a JSON command the script sends; nothing opens a window or touches your
mouse, so they can run in the background while you work.

```bash
cargo build --release -p anvil-app --bin anvilcad-agent
python3 scripts/cinema/films.py                  # every film, a few minutes on an M-series Mac
python3 scripts/cinema/films.py trailer          # one film
python3 scripts/cinema/films.py --list
python3 scripts/cinema/films.py fillet --draft   # half size, 1x takes, fast encode: for iterating
```

Films go to `target/cinema/<name>.mp4`, 1920x1080 at 30 fps, H.264, each with a
`<name>-poster.jpg`. Options:

| Option | Does |
|---|---|
| `--draft` | Records at 1x and renders at 960x540 with a fast encode |
| `--letterbox` | 2.39:1 cinema bars |
| `--grain` | A little film grain |
| `--music FILE` | Lays a soundtrack under each film, fading in and out. Use a track you have the rights to; the repository ships none |
| `--out DIR` | Output folder |

Needs Python 3 with Pillow and NumPy, and `ffmpeg` on the `PATH`. Fonts are Helvetica
Neue and Menlo on macOS, DejaVu on Linux (`fonts-dejavu`) and Segoe UI and Consolas on
Windows.

## The films

| Film | Shows |
|---|---|
| `trailer` | About 70 s: a cold open, the title, sketch, dimension and extrude, fillets, equations, a section sweep, STEP import, mates, and the agent overlay |
| `sketch` | A plate sketched, dimensioned and extruded, then a hero turn |
| `fillet` | Four edges rounded, then a hero turn |
| `equations` | Variables changed in the Equations window, then the width stepped from 80 to 130 mm |
| `section` | A section plane swept through the sample part |
| `step` | A NIST test part imported from STEP as an exact solid |
| `mass` | Mass properties of the sample part |
| `check-design` | Design checks for CNC finding thin walls and a small, deep hole |
| `assembly` | Two parts inserted and mated until fully defined |
| `agent` | The sketch-to-solid build with a terminal showing the JSON commands as they are sent |

## How a film is made

Three files, in layers:

- `take.py`: a **take** is one run of `anvilcad-agent`. `Take` extends the website recorder
  (`scripts/site/recorder.py`), which takes a screenshot after every step and records where
  each click lands. It adds a dark studio look (an ordinary Lua configuration that changes
  only colours), a log of the commands sent for the agent overlay, and clips:
  `take.begin()` ... `take.clip("hero" | "ui", **options)` marks a stretch to use as a shot.
- `post.py`: the look. **Hero** clips fill the frame with the model on the studio backdrop,
  push in and drift slowly over a 2x supersampled frame, and dissolve across state changes.
  **UI** clips put the whole app in a framed window, with a pointer drawn in and a camera that
  follows it and pushes in on the action. **Cards** are full-frame titles; captions are lower
  thirds; the agent panel types out the logged commands. A `Film` strings clips and cards
  together with cuts, crossfades and dips to black, grades every frame (a gentle contrast
  curve and a vignette) and streams it into ffmpeg.
- `films.py`: the shots (one function per take, cached so a run records each once) and the
  films made from them.

Clip options: `push=(start, end)` zoom, `focus=(x, y)` and `drift=(dx, dy)` in screen
points, `stage=(x0, y0, x1, y1)` (the part of the screen a hero shot looks at), `dissolve`
(frames), `zoom` and `follow` for UI clips, `caption="text"` or `("headline", "subline")`,
`title=(kicker, headline, subline)` for a centred title over footage, and `agent=True` for
the terminal overlay. `sub(clip, a, b, **options)` in `films.py` cuts a clip to seconds `a`
to `b` (negative counts from the end) and can change its options.

## Adding a film

1. Write a shot: open a take, set the scene inside `with r.quietly():` (setup the agent
   overlay should not show), then `r.begin()`, drive the app with `r.click`, `r.type`,
   `r.key`, `r.orbit`, `r.cmd(cmd="lua", ...)` and so on, and end with `r.clip(...)`.
   Call `report(r, name)` so the run prints the part's state.
2. Add a film to `FILMS`: usually `spot(name, kicker, headline, [clips])`.
3. Render it with `--draft`, make a contact sheet and look at it before rendering it full
   size:
   `ffmpeg -i target/cinema/NAME.mp4 -vf "fps=1,scale=480:-1,tile=4x5" -frames:v 1 sheet.png`

## Rules for what the films say

- Captions say only what the README's "What works today" says, and the `report` lines
  printed during a run must back them up: the STEP shot only says "exact" when the harness
  reports the imported body as exact, for example.
- Show only parts the project may publish: the bundled sample, parts the scripts build, and
  the NIST test models (NIST allows their use without restriction; keep the note that this
  implies no endorsement).
- No SOLIDWORKS names, files, icons or artwork in the films. See
  [docs/POSITIONING.md](../../docs/POSITIONING.md) and [INTEROPERABILITY.md](../../INTEROPERABILITY.md).
- Do not commit rendered films to the repository; attach them to a release or upload them
  where they will be shown. The website loops in `docs/media/` are made by
  `scripts/site/demos.py`.
