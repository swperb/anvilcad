# Configuration and scripting with Lua

AnvilCAD reads a Lua file at startup, `init.lua`, in the same spirit as Neovim or WezTerm.
It sets options, recolours the interface, binds keys and adds your own commands. The same
`anvil` API runs scripts that build and change parts. Lua 5.4 is built into AnvilCAD, so
there is nothing to install, and it works the same on Linux, macOS and Windows.

[docs/examples/init.lua](examples/init.lua) is a complete example; CI loads it and runs its
commands on all three platforms, so it always matches what the app accepts.

## Where the file lives

| Platform | Path |
|---|---|
| Linux, macOS | `$XDG_CONFIG_HOME/anvilcad/init.lua`, by default `~/.config/anvilcad/init.lua` |
| Windows | `%APPDATA%\AnvilCAD\init.lua` |
| Any | `$ANVILCAD_CONFIG`, if set, or `anvilcad --config FILE` |

`anvilcad --no-config` starts without it. **Tools > Edit Config** creates a starter file if
there is none and shows where it is. AnvilCAD checks the file about once a second and reloads it
when it changes, so edits show up as soon as you save. If the new version has a mistake,
AnvilCAD says what and where, and keeps the previous settings until you fix it.

`require("name")` loads `name.lua` or `lua/name.lua` from the same folder, so a large
configuration can be split into modules, kept in a dotfiles repository and symlinked into place.
The whole Lua standard library is available; the file is your own code, running as you.

## Options

```lua
anvil.opt.theme = "dark"              -- "light" or "dark"
anvil.opt.ui_scale = 1.25             -- 0.5 to 3
anvil.opt.font = "~/fonts/Inter.ttf"  -- interface font (.ttf or .otf)
anvil.opt.mono_font = "~/fonts/JetBrainsMono.ttf"
anvil.opt.material = "6061 Alloy"     -- library material for new parts
anvil.opt.template = "~/parts/house.anvprtdot"  -- template new parts start from
anvil.opt.open = "sample"             -- or a path: what opens when no file is given
anvil.opt.view = "isometric"          -- front, back, left, right, top, bottom, isometric
anvil.opt.edges = true                -- Shaded with edges
anvil.opt.snap_px = 10                -- sketch snap and pick radius, 2 to 40 pixels
```

A misspelled option, or a value of the wrong kind, is an error that lists the valid ones.
There is no units option yet: AnvilCAD models in millimetres throughout (the status bar's
MMGS), and a units system is future work rather than something a setting can switch.

## Theme

Each theme has its own colours, by the names the interface uses; the model's colours apply to
both themes. Colours are `#rgb`, `#rrggbb` or `#rrggbbaa`.

```lua
anvil.theme.dark.accent = "#fe8019"
anvil.theme.dark.panel = "#282828"
anvil.theme.light.view_top = "#dfe6ee"
anvil.theme.body = "#bdae93"         -- the part
anvil.theme.edge = "#1d2021"         -- model edges
```

Interface colours: `chrome`, `panel`, `inset`, `border`, `text`, `text_weak`, `hover`,
`accent`, `on_accent`, `select_fill`, `select`, `error`, `warning`, `ok`, `view_top`,
`view_bottom` (the viewport gradient) and `float_fill` (the view toolbar). Model colours:
`body`, `hover_face`, `selected_face` and `edge`.

## Keys

```lua
anvil.leader = "space"
anvil.key("<leader>e", "Extruded Boss/Base")     -- a command, by name
anvil.key("g g", "Zoom to Fit")                  -- a sequence: g, then g
anvil.key("ctrl+shift+t", "Toggle Theme")        -- a chord
anvil.key("<leader>v", function()                -- or any Lua function
  anvil.status(("Volume: %.1f"):format(anvil.state().body.volume))
end, "Show the volume")
```

- Modifiers: `cmd` (or `mod`) is Command on macOS and Ctrl elsewhere, so one binding works on
  every platform; `ctrl`, `shift` and `alt` (`opt`) are those keys everywhere.
- Keys use egui's names in any case: `a`, `F5`, `Enter`, `Escape`, `Space`, `Tab`,
  `PageDown`, `ArrowUp` (or `up`), `/`, `plus` for `+`.
- In a sequence, each key must follow within 1.5 s. A sequence that breaks hands its last key
  on, so `g` then `x` still runs a binding on `x`.
- Your bindings take priority over AnvilCAD's own shortcuts (such as Ctrl+S), and are not
  active while you type in a text field.

## Commands

```lua
anvil.command("Test Plate", function()
  local s = anvil.sketch{ plane = "top", rects = { { -50, -30, 50, 30 } } }
  anvil.extrude{ sketch = s, depth = 10 }
end, { description = "A 100 x 60 x 10 plate" })
```

User commands appear in the command palette and the Tools menu, and keys can bind them by
name. **Ctrl+Shift+P** (Cmd+Shift+P on macOS) opens the command palette, which finds any
built-in or user command by typing part of its name or description.

Built-in commands, by the names the toolbar and menus use: New, New Assembly, Insert
Component, Mate, Bill of Materials, Open, Save, Save As, Save as Part Template, Export, Open Sample Part, Undo, Redo, Rebuild, Delete, Extruded Boss/Base,
Revolved Boss/Base, Extruded Cut, Revolved Cut, Linear Pattern, Circular Pattern, Mirror,
Fillet, Chamfer, Shell, Hole, Sweep, Loft, Draft, Dome, Rib, Combine, Move/Copy Bodies,
Delete/Keep Bodies, Split, Scale, Plane, Axis, Point, Coordinate System, Sketch, Exit Sketch,
Convert Entities, Front, Back, Left, Right, Top, Bottom, Isometric, Normal To, Zoom to Fit,
Shaded with edges, Dark Theme, Light Theme, Toggle Theme, Features Tab, Sketch Tab, Evaluate
Tab, Mass Properties, Check Design (DFM), Measure, Section View, Equations, Custom Properties,
Configuration Table, Command Palette, Run Script..., Reload Config and Edit Config. Names are
not case-sensitive. A binding to a name that does not exist is an error when the file loads.

## The scripting API

Inside a command, a key binding's function, or a script, these drive the open part. A failing
call raises a Lua error with a message that says what was wrong; the part keeps every change
made before the error, which Undo reverses.

| Call | Does |
|---|---|
| `anvil.run(name)` | runs a built-in command |
| `anvil.state()` | the app's state as a table: `body` (`volume`, `surface_area`, `centroid`, `bounds`), `features`, `variables`, `material`, `path`, `status`, ... (the same as the headless harness's `state`) |
| `anvil.new([{ force = true }])` | starts a new part from the template and material options |
| `anvil.open(path [, { force = true }])` | opens a part, template, STEP or SOLIDWORKS file |
| `anvil.save([path])` | saves, to `path` or the part's file |
| `anvil.export(path)` | writes STEP (exact where possible), STL or OBJ, by extension |
| `anvil.var(name, value)` | sets a global variable to a number or an equation, such as `"width / 2"` |
| `anvil.material(name)` | sets a library material |
| `anvil.sketch{ plane, rects, circles }` | adds a sketch and returns its id; `plane` is `"front"`, `"top"`, `"right"` or `{ on = feature_id, face = "end" \| "start" }`; `rects` are `{x0, y0, x1, y1}` corners and `circles` are `{x, y, radius}`, in the sketch's coordinates (mm) |
| `anvil.extrude{ sketch, depth, cut, reverse }` | extrudes a sketch and returns the feature id; `depth` is mm, an equation, or `"through"` |
| `anvil.status(text)` | shows text in the status bar |

`anvil.new` and `anvil.open` refuse to throw away unsaved changes unless you pass
`{ force = true }`. A script that runs for more than 10 seconds is stopped, so a mistake such as
an endless loop cannot freeze the app.

## Running scripts

- In the app: **Tools > Run Script...**.
- At launch: `anvilcad --script build.lua`.
- With no window: `anvilcad-agent --lua build.lua --screenshot out.png`, or the headless
  harness's `lua`, `config` and `command` commands ([AGENT_HARNESS.md](AGENT_HARNESS.md)).
  The headless harness reads no `init.lua` unless told to, so its runs are repeatable.
