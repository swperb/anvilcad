# AnvilCAD site

This is the AnvilCAD landing page: plain static HTML, CSS and a small script (`site.js`),
with no framework and no build step. To preview it, serve the folder locally with
`python3 -m http.server 8000` from inside `docs/` and visit http://localhost:8000 (videos
need a server; opening `index.html` directly also works, but some browsers block them).
GitHub Pages publishes this folder from the `main` branch at
https://swperb.github.io/anvilcad/. `screenshot.png` is the README image and is not used
by the page.

The copy follows the audience, claims and messages in `POSITIONING.md`: lead with
migration that keeps the feature tree, and keep every claim true of the code on the day it
is published.

## Recordings

Every video in `media/` is a real recording of the app, made with the headless harness
(`anvilcad-agent`, see [AGENT_HARNESS.md](AGENT_HARNESS.md)) by the scripts in
`scripts/site/`. The harness draws no mouse pointer, so the recorder draws one and moves it
between the clicks the script makes. To re-record after a UI change (needs `ffmpeg`,
`gifski` and Python with Pillow):

```bash
cargo build --release -p anvil-app --bin anvilcad-agent
python3 scripts/site/demos.py            # every demo
python3 scripts/site/demos.py params     # just one
```

| File | What it shows |
|---|---|
| `build.mp4`, `build.gif` | Sketch a plate with a hole, dimension it, extrude it 16 mm |
| `refine.mp4` | Fillet the plate's four corners at 6 mm |
| `params.mp4` | Change the sample part's global variables and watch it rebuild |
| `evaluate.mp4` | Mass properties of the sample part |
| `showcase.mp4`, `showcase.gif` | The sample part turning in the viewport |

If a demo's chapter times change, `demos.py` prints them (`build: build at 21.20 s`);
update `data-at` on the matching button in `index.html`.
