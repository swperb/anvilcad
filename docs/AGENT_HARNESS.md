# Driving AnvilCAD headlessly

`anvilcad-agent` runs the real AnvilCAD UI with no window. Coding agents, scripts and
tests use it to click through the app, read its state and take screenshots. It is built
so that it can run in the background while you keep working:

- **No window, no focus.** It never creates an OS window or registers as a GUI app, so it
  cannot take focus or show up in the Dock, taskbar or app switcher. This holds on macOS,
  Windows and Linux. On Linux it needs no X or Wayland display.
- **No real input.** Mouse and keyboard events are made up and passed straight to egui.
  The harness never reads or moves your cursor and never sees what you type, and nothing
  it does reaches other apps.
- **Repeatable.** Time is virtual (each frame advances 1/60 s), so the same script gives
  the same result and the same pixels every run. Any number of copies can run at once.
- **No native dialogs.** Open, Save, Save As and Export dialogs are answered by the
  script (see `file_dialog`). If no answer is queued, the dialog is cancelled.

Screenshots are drawn with wgpu into an offscreen texture: Metal on macOS, DX12 or Vulkan
on Windows, and Vulkan on Linux. Without a GPU it falls back to a software adapter (WARP
on Windows, Mesa's lavapipe on Linux, which comes in the `mesa-vulkan-drivers` package).
Set `ANVIL_SOFTWARE_RENDER=1` to force the software adapter. Everything except
screenshots also works with no adapter at all.

## Running it

```bash
cargo run -p anvil-app --bin anvilcad-agent -- --sample --screenshot target/agent/sample.png
```

```bash
cargo run -p anvil-app --bin anvilcad-agent -- --script docs/agent/plate-with-hole.jsonl
```

Options: `--open FILE` or `--sample` to start with a part, `--size WxH` for the screen
size in points (default 1400x880), `--scale S` for pixels per point (default 1, use 2
for Retina-sharp screenshots), `--script FILE` to run a file of commands, `--keep-going`
to keep running a script after a command fails, and `--screenshot OUT.png` to save a
screenshot at the end.

With no `--script`, it reads commands from stdin until `{"cmd":"quit"}` or end of input,
so an agent can keep one session open and send commands one at a time.

## Protocol

There is one JSON object per line in each direction. Every reply has `"ok"`. A failure adds
`"error"` with a message that says what to do next, for example the widgets that matched
when a label was ambiguous. If a command has an `"id"`, the reply echoes it. A reply also
has `"file_dialogs"` when the command opened a file dialog, and `"closed": true` when the
app asked to quit. In a script, blank lines and lines starting with `#` or `//` are
skipped, and the run stops at the first failure (exit code 1) unless `--keep-going` is
given.

### Targets

Commands that act on a place on screen take a target. A target is one of:

| Target | Meaning |
|---|---|
| `"x": 500, "y": 300` | a screen position in points (0,0 is the top left) |
| `"label": "Extruded Boss/Base"` | the centre of the widget with that label: an exact match first, then a case-insensitive substring. `"role": "Button"` and `"index": n` choose between several matches |
| `"model": [x, y, z]` | where a model point appears in the viewport |
| `"sketch": [x, y]` | where a point in the sketch being edited appears |

Add `"offset": [dx, dy]` to move the target. If a label matches several different widgets,
the command fails and lists them with their indexes and positions. The Sketch tab and the
Sketch command are an example. Matches that are the same command in two places, such as a
sketch tool on the command bar and in the sketch panel, count as one.

### Commands

| Command | Fields | Reply |
|---|---|---|
| `state` | | `state`: title, path, unsaved changes, status bar, active tab, selection, features (id, name, kind, suppressed, rolled back, rebuild error, sketch status), body (volume, surface area, centroid, bounds, open edges, whether it is exact, triangle count), material, variables, the open sketch (tool, solver status, degrees of freedom, counts), the open feature editor, open windows, the message dialog, the pending confirmation, undo and redo depth, and the viewport rectangle |
| `document` | | `document`: the whole part, as saved in `.anvprt` |
| `widgets` | `filter`, `role` | `widgets`: each one's `label` or `value`, `role`, `rect`, `center`, and `disabled`, `checked` and `selected` where they apply |
| `find` | `label`, `role`, `index` | `widget` |
| `click` | target, `button` (`left`, `right`, `middle`), `count`, `modifiers` | `at` |
| `double_click` | target, `button` | `at` |
| `hover` | target | `at` |
| `drag` | `from` and `to` targets, `button`, `steps`, `modifiers` | |
| `scroll` | target (default: viewport centre), `dx`, `dy` (positive `dy` zooms in) | |
| `key` | `key` (egui key name: `Enter`, `Escape`, `Delete`, `A`, `7`, `F2`...), `modifiers` | |
| `type` | `text`, typed into the focused field | |
| `screenshot` | `path`, and `label` or `region` `[x0,y0,x1,y1]` to crop | `path`, `width`, `height`, `adapter` |
| `resize` | `width`, `height` (points), `scale` | |
| `file_dialog` | `path`, or `null` to cancel; answers the next dialog | |
| `open` | `path` (part, template or STEP file) | |
| `sample`, `new` | | |
| `frames` | `n` | |
| `wait` | `seconds` of virtual time, for example for hover tooltips | |
| `project` | target | `at`: the screen position, without acting on it |
| `help` | | the command list |
| `quit` | | |

`modifiers` is a list of `cmd`, `ctrl`, `shift` and `alt`. `cmd` means Command on macOS
and Ctrl elsewhere, so `{"cmd":"key","key":"Z","modifiers":["cmd"]}` is Undo on every
platform. Each input command keeps running frames until the UI settles, so the next
`state` already shows its effect.

### A session

```
> {"cmd":"sample"}
< {"ok":true}
> {"cmd":"click","label":"Evaluate"}
< {"at":[148.9,33.0],"ok":true}
> {"cmd":"widgets","filter":"mass"}
< {"ok":true,"widgets":[{"center":[...],"label":"Mass Properties","role":"Button",...}]}
> {"cmd":"file_dialog","path":"/tmp/out.step"}
< {"ok":true}
> {"cmd":"click","label":"File"}
> {"cmd":"click","label":"Export"}
< {"file_dialogs":[{"answer":"/tmp/out.step","title":"Export"}],"ok":true,...}
```

## Tips for agents

- Start with `widgets` to see what is on screen and how it is labelled, use `state` to
  check the outcome of each step, and take a `screenshot` (then look at it) when layout or
  rendering matters.
- In sketches, prefer `sketch` targets to pixel positions. They follow the camera and
  snap like a real click on that point would.
- Press `Escape` to end a line chain or cancel a tool before you exit a sketch.
- Write screenshots under `target/` (it is ignored by git) or a scratch directory.

## In Rust tests

`anvil_app::headless::Headless` is the same harness as a library. `Headless::command`
takes the same JSON, and there are typed methods (`click`, `drag`, `key`, `find`,
`widgets`, `render`) as well as `app` for direct access. See the tests in
`crates/anvil-app/src/headless.rs`.
