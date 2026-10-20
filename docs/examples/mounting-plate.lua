-- A scripted part: a 120 x 70 plate, its thickness a variable, with four M8 clearance holes
-- and a 30 mm bore. Run it with Tools > Run Script..., `anvilcad --script`, or without a window:
--   anvilcad-agent --lua docs/examples/mounting-plate.lua --screenshot plate.png
anvil.new{ force = true }
anvil.var("thickness", 12)
anvil.material("6061 Alloy")

local outline = anvil.sketch{ plane = "top", rects = { { -60, -35, 60, 35 } } }
local plate = anvil.extrude{ sketch = outline, depth = "thickness" }

local holes = anvil.sketch{
  plane = { on = plate },
  circles = { { -45, -22, 4.5 }, { 45, -22, 4.5 }, { -45, 22, 4.5 }, { 45, 22, 4.5 }, { 0, 0, 15 } },
}
anvil.extrude{ sketch = holes, depth = "through", cut = true }

local s = anvil.state()
anvil.status(string.format("Mounting plate: %.0f mm³", s.body.volume))
