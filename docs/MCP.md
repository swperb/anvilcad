# AnvilCAD for AI agents: the MCP server

`anvilcad-agent mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server.
Claude Code, Cursor, Claude Desktop and other MCP clients use it to model parts with no
window: create or open a part, add sketches and extrusions, change dimensions and variables,
measure the body, export STEP or STL and look at screenshots.

It is built on the [headless harness](AGENT_HARNESS.md), so it never opens a window, takes
focus or touches your mouse and keyboard, and runs on macOS, Linux and Windows with no
display. Its modelling tools call the same API as Lua scripts ([SCRIPTING.md](SCRIPTING.md)),
and `run_lua` runs any script in that API.

## Setup

Build and install the binary, then register it with your client.

```bash
cargo install --path crates/anvil-app --bin anvilcad-agent
```

Claude Code:

```bash
claude mcp add anvilcad -- anvilcad-agent mcp
```

Cursor, Claude Desktop and other clients take the same command in their JSON
configuration (for Cursor, `.cursor/mcp.json` in the project or `~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "anvilcad": { "command": "anvilcad-agent", "args": ["mcp"] }
  }
}
```

If `~/.cargo/bin` is not on the client's `PATH`, give the full path to `anvilcad-agent`.

## Tools

All lengths are millimetres and angles degrees. Relative paths are relative to the folder
the client started the server in (for Claude Code, the project). Folders for files the
server writes are created as needed.

| Tool | Arguments | Does |
|---|---|---|
| `new_part` | `material` | starts a new, empty part, discarding the current one |
| `open_part` | `path` | opens an `.anvprt` part, template, STEP or SOLIDWORKS file; returns its feature tree, material and body |
| `save_part` | `path` | saves as `.anvprt`, to `path` or the part's file |
| `list_features` | | the feature tree: id, name, kind, rebuild error and settable dimensions (value and equation) of each feature, and the global variables |
| `get_state` | | everything the harness's `state` command reports |
| `add_sketch` | `plane` (`front`, `top`, `right`) or `on_feature` and `face` (`end`, `start`); `rects` `[[x0, y0, x1, y1], ...]`; `circles` `[[x, y, r], ...]` | adds a sketch and returns its id, or fails at once if the face cannot carry a sketch (a cut's faces cannot). Front is the XY plane, top XZ and right YZ. On a face, the sketch's x is world X (world Y on a face normal to X) and its origin is the world origin projected onto the face |
| `add_extrude` | `sketch`, `depth` (mm, an equation, or `"through"`), `cut`, `reverse` | extrudes a boss, or a cut; returns its id and the body after the rebuild, with a `warning` if the body did not change. A boss goes along the plane's normal (+Z from front, +Y from top, +X from right, out of a face); a cut goes the other way (into a face); `reverse` flips either |
| `delete_feature` | `feature` (id or name) | deletes the feature and those that depend on it (a sketch takes its extrusion with it); returns the ids removed |
| `set_variable` | `name`, `value` (number or equation) | sets or adds a global variable and rebuilds |
| `set_dimension` | `feature` (id or name), `dimension`, `value` | sets `depth`, `angle`, `offset`, `spacing`, `count`, `radius`, `distance`, `chamfer angle` or `thickness`; if the rebuild fails, the old value is kept and the error returned |
| `set_material` | `name` | assigns a library material, such as `6061 Alloy` or `Plain Carbon Steel` |
| `rebuild` | | rebuilds and reports the body and any feature errors |
| `mass_properties` | | volume (mm³), surface area (mm²), centroid, bounding box and size (mm); mass (g) and principal moments of inertia (g·mm²) when a material is set. They are computed from the tessellated body, so curved faces make them slightly approximate; `exact` says whether STEP export writes an exact B-rep |
| `export` | `path` | `.step` or `.stp` (exact B-rep where possible), `.stl` or `.obj` |
| `screenshot` | `path`, `view`, `fit` (default true), `crop` (`viewport` or `window`), `width`, `height`, `inline` (default true) | renders a PNG to `path` and, unless `inline` is false, returns it as an image |
| `run_lua` | `code` | runs Lua with the `anvil` API, for anything else; returns the chunk's return value as `result` and what it printed as `printed` |
| `run_command` | `name` | runs a built-in command, such as `Undo`, `Redo` or `Isometric` |

A tool that fails returns a result with `isError: true` and a message that says what was
wrong, and the session carries on. Unknown tools and malformed requests get JSON-RPC errors.
The server never exits on bad input; it stops when the client closes its stdin.

## Example

What an agent does to make a 100 × 60 × 10 mm plate with a 10 mm hole, then make it 12 mm
thick:

```text
new_part        { "material": "6061 Alloy" }
add_sketch      { "plane": "top", "rects": [[-50, -30, 50, 30]] }        -> { "sketch": 1 }
add_extrude     { "sketch": 1, "depth": "thickness" }                    -> error: unknown variable
set_variable    { "name": "thickness", "value": 10 }
add_extrude     { "sketch": 1, "depth": "thickness" }                    -> { "feature": 2, "body": ... }
add_sketch      { "on_feature": 2, "circles": [[0, 0, 5]] }              -> { "sketch": 3 }
add_extrude     { "sketch": 3, "depth": "through", "cut": true }
set_variable    { "name": "thickness", "value": 12 }
mass_properties {}                                                       -> { "volume": ..., "mass": ... }
export          { "path": "out/plate.step" }
screenshot      { "path": "out/plate.png", "view": "isometric" }
```

## Protocol

JSON-RPC 2.0 over stdio, one message per line (the MCP stdio transport). The server
answers `initialize` with protocol revision 2025-06-18, or with the client's revision when it
is 2025-03-26 or 2024-11-05. It offers tools only; `resources/list` and `prompts/list` are
empty. Results carry the JSON as text and as `structuredContent`. Stdout carries nothing but
protocol messages; anything else goes to stderr.

## Testing

`crates/anvil-app/src/mcp/tests.rs` drives the server with JSON-RPC and checks exact values
(the 10 × 20 × 30 mm block's volume, area, centroid and mass, dimension and variable
changes, cuts, deleting features, face sketches that must stay put when a variable changes,
STEP and STL round trips, and every kind of error), and
`crates/anvil-app/tests/mcp_stdio.rs` runs the binary as a client would.
