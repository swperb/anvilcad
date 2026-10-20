-- An example AnvilCAD configuration. Copy it to ~/.config/anvilcad/init.lua
-- (%APPDATA%\AnvilCAD\init.lua on Windows) and change what you like; AnvilCAD reloads it
-- whenever you save. docs/SCRIPTING.md describes everything used here.
--
-- CI loads this file, so it always matches what the app accepts.

-- Options ---------------------------------------------------------------------------------
anvil.opt.theme = "dark"
anvil.opt.ui_scale = 1.1
anvil.opt.material = "6061 Alloy"   -- new parts start as 6061
anvil.opt.view = "isometric"
anvil.opt.edges = true
anvil.opt.snap_px = 10
-- anvil.opt.font = "~/.local/share/fonts/Inter-Regular.ttf"
-- anvil.opt.mono_font = "~/.local/share/fonts/JetBrainsMono-Regular.ttf"
-- anvil.opt.template = "~/parts/house-style.anvprtdot"
-- anvil.opt.open = "sample"

-- Theme: a gruvbox-ish dark palette ------------------------------------------------------
local gruvbox = {
  chrome = "#1d2021", panel = "#282828", inset = "#1d2021", border = "#3c3836",
  text = "#ebdbb2", text_weak = "#a89984", hover = "#32302f",
  accent = "#fe8019", on_accent = "#1d2021",
  select_fill = "#2c3a3a", select = "#83a598",
  error = "#fb4934", warning = "#fabd2f", ok = "#b8bb26",
  view_top = "#3c3836", view_bottom = "#1d2021",
}
for name, color in pairs(gruvbox) do
  anvil.theme.dark[name] = color
end
anvil.theme.body = "#bdae93"
anvil.theme.edge = "#1d2021"

-- Keys ------------------------------------------------------------------------------------
anvil.leader = "space"

-- A command name binds the key straight to that command...
anvil.key("<leader>e", "Extruded Boss/Base")
anvil.key("<leader>c", "Extruded Cut")
anvil.key("<leader>f", "Fillet")
anvil.key("<leader>s", "Sketch")
anvil.key("<leader>p", "Command Palette")
anvil.key("g g", "Zoom to Fit")
anvil.key("ctrl+shift+t", "Toggle Theme")

-- ...and a function can do anything the scripting API can.
anvil.key("<leader>v", function()
  local s = anvil.state()
  anvil.status(string.format("Volume: %.1f mm³", s.body.empty and 0 or s.body.volume))
end, "Show the volume in the status bar")

-- Commands --------------------------------------------------------------------------------
-- These appear in the command palette (Ctrl+Shift+P) and the Tools menu.
local function plate(w, d, t)
  anvil.var("plate_t", t)
  local s = anvil.sketch{ plane = "top", rects = { { -w / 2, -d / 2, w / 2, d / 2 } } }
  return anvil.extrude{ sketch = s, depth = "plate_t" }
end

anvil.command("Test Plate", function()
  anvil.new()   -- refuses if the open part has unsaved changes
  local base = plate(100, 60, 10)
  local holes = anvil.sketch{
    plane = { on = base },
    circles = { { -40, -20, 3.3 }, { 40, -20, 3.3 }, { -40, 20, 3.3 }, { 40, 20, 3.3 } },
  }
  anvil.extrude{ sketch = holes, depth = "through", cut = true }
  anvil.run("Zoom to Fit")
end, { description = "A 100 x 60 x 10 plate with four M3 clearance holes" })

anvil.command("Export STEP Next to the Part", function()
  local s = anvil.state()
  if not s.path then
    error("save the part first")
  end
  anvil.export((s.path:gsub("%.anvprt$", "")) .. ".step")
  anvil.status("Exported STEP")
end, { description = "Writes PART.step beside PART.anvprt" })
