# 3D mice

AnvilCAD navigates with a 3Dconnexion SpaceMouse or a compatible six-axis device. Plug it in
and move the cap: the model moves the way the cap moves ("object mode", as other CAD
software does by default). Push the cap right and the model moves right; lift it and the model
moves up; pull it toward you and the model comes closer (zoom in); tilt, twist or roll it and
the model turns the same way, about the middle of the view. A held deflection keeps the view
moving at a steady speed, faster the further you push.

No vendor driver or SDK is needed. AnvilCAD reads the device directly: hidraw on Linux,
hidapi on macOS and Windows. On Linux, if spacenavd is running, AnvilCAD reads from
spacenavd instead. The status bar says when a device connects or disconnects, or why one
cannot be read. Only the focused AnvilCAD window moves, so working in another window
leaves the model alone.

The device code is `crates/anvil-3dmouse`. The axis mapping and rates are in
`crates/anvil-app/src/mouse3d.rs`, and the app's handling is in `app/navigation.rs`.

## Settings

In `init.lua` (see [SCRIPTING.md](SCRIPTING.md)):

```lua
anvil.opt.mouse_3d = true             -- false: AnvilCAD leaves the device alone
anvil.opt.mouse_3d_speed = 1          -- 0.1 to 10
anvil.opt.mouse_3d_invert = { "tz" }  -- axes to reverse
anvil.mouse_3d_button("Fit", "Isometric")
```

The axes are `tx` (cap right), `ty` (cap up), `tz` (cap toward you), and `rx`, `ry` and `rz`,
rotations right-handed about those three directions. At speed 1 and full deflection, the
model turns at 180° per second, pans one view height per second, and zooms by a factor of
e^1.5 (about 4.5) per second. Below 4% of full deflection nothing moves, and above that the
speed rises with the square of the deflection, so small movements are fine.

## Buttons

Without a binding:

- Left or Menu opens the command palette.
- Right or Fit zooms to fit.
- `T`, `F`, `R`, `L` and `ISO` go to the Top, Front, Right, Left and Isometric views.

`anvil.mouse_3d_button(name, action)` binds a button to a command, a Lua function, or `false`
(nothing). A button is named by its label below, or `Button N` for any button (N is the
button number in the tables).

| Layout | Models | Button number: label |
|---|---|---|
| Two buttons | SpaceNavigator (and for Notebooks), SpaceMouse Compact, SpaceMouse Wireless | 1 Left, 2 Right |
| Pro | SpaceMouse Pro, Pro Wireless (all three IDs), Universal Receiver | 1 Menu, 2 Fit, 3 T, 5 R, 6 F, 9 Roll, 13–16 `1`–`4`, 23 Esc, 24 Alt, 25 Shift, 26 Ctrl, 27 Lock |
| Enterprise | SpaceMouse Enterprise | The Pro's, plus 11 ISO, 17–22 `5`–`10`, 36 Enter, 37 Delete, 77 `11`, 78 `12`, 103–105 V1–V3, 175 Tab, 176 Space |
| SpacePilot Pro | SpacePilot Pro | 1 Menu, 2 Fit, 3 T, 5 R, 6 F, 9 Roll, 11 ISO, 23 Esc, 24 Alt, 25 Shift, 26 Ctrl, 27 Lock, 28 Pan/Zoom, 29 Dominant, 30 Plus, 31 Minus. The number keys are `Button N` only: sources disagree on their numbers |
| SpaceExplorer | SpaceExplorer | 1 `1`, 2 `2`, 3 T, 4 L, 5 R, 6 F, 7 Esc, 8 Alt, 9 Shift, 10 Ctrl, 11 Fit, 12 Menu, 13 Plus, 14 Minus, 15 2D |
| Unnamed | SpacePilot, SpaceTraveler, Spaceball 5000, SpaceMouse Plus XT, CadMan, SpaceMouse Classic, SpaceMouse Module, unknown newer 3Dconnexion models | `Button N` only |
| spacenavd | Any device read through spacenavd, which does not say which model it is | `Button N` only; buttons 1 and 2 keep the Left/Menu and Right/Fit defaults |

Labels are matched without regard to case.

## Platforms

### Linux

Two ways work, and AnvilCAD uses whichever is there:

- **spacenavd running:** AnvilCAD connects to its socket and needs nothing else.
  spacenavd's own settings (sensitivity, axis mapping) apply before AnvilCAD's.
- **No spacenavd:** AnvilCAD reads `/dev/hidraw*`. Since late 2024, systemd's hardware
  database marks most 3Dconnexion devices as 3D mice and gives the logged-in user access.
  On older systems, and for the Pro Wireless BT (`256f:c638`) and the Universal Receiver
  (`256f:c652`), which that database does not list, install this rule. Then run
  `sudo udevadm control --reload && sudo udevadm trigger` and plug the device in again:

```
# /etc/udev/rules.d/70-anvilcad-3dmouse.rules
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="256f", TAG+="uaccess"
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="046d", ATTRS{idProduct}=="c603|c605|c606|c621|c623|c625|c626|c627|c628|c629|c62b", TAG+="uaccess"
SUBSYSTEM=="hidraw", KERNELS=="0005:256F:*", TAG+="uaccess"
```

If the device is there but cannot be opened, the status bar says
"*Model*: no permission to read /dev/hidrawN (docs/3D_MOUSE.md has the fix)".

### macOS and Windows

AnvilCAD reads the device through hidapi, opened in shared mode. Without 3Dconnexion's
3DxWare driver installed, that is all there is to it. **With 3DxWare installed, its driver may
hold the device so that other programs read nothing.** Users of other programs that read
the device directly report this. Then the status bar says "*Model* could not be opened; 3Dconnexion's driver may be
holding it". Quitting 3DconnexionHelper (macOS) or stopping the 3DxWare service (Windows),
then plugging the device in again, lets AnvilCAD read it.
Working alongside 3DxWare, through 3Dconnexion's own interface, is not done yet (see
"Not done" below).

## How the device is read

This section records the facts the code is built on and where each came from.

### Models

A device is a 3D mouse if its USB vendor and product ID are in `MODELS`
(`crates/anvil-3dmouse/src/devices.rs`), or if its vendor is 3Dconnexion (`256f`) and its
product is not a known non-3D device (CadMouse c650, c651, c654–c658; Keyboard Pro c664,
c668; Numpad Pro c665; c62c). An unknown `256f` device is read as a combined-format model
with unnamed buttons, so a model newer than the table still navigates.

When a device has several interfaces, AnvilCAD reads the one whose report descriptor
declares a top-level multi-axis controller collection (usage page 1, usage 8). For example,
the Universal Receiver also has a mouse interface for a paired CadMouse. The Logitech-era
models (`046d`) have a single interface and are read whatever their descriptor declares.

| VID:PID | Model | Axis reports |
|---|---|---|
| 046d:c603 / c605 / c606 | SpaceMouse Plus XT / CadMan / SpaceMouse Classic | separate (assumed) |
| 046d:c621 | Spaceball 5000 (USB) | separate (assumed) |
| 046d:c623 | SpaceTraveler | separate (assumed) |
| 046d:c625 | SpacePilot | separate |
| 046d:c626 | SpaceNavigator | separate |
| 046d:c627 | SpaceExplorer | separate |
| 046d:c628 | SpaceNavigator for Notebooks | separate (assumed) |
| 046d:c629 | SpacePilot Pro | separate |
| 046d:c62b | SpaceMouse Pro | separate |
| 256f:c62e / c62f | SpaceMouse Wireless (cable / receiver) | combined |
| 256f:c631 / c632 / c638 | SpaceMouse Pro Wireless (cable / receiver / BT model) | combined |
| 256f:c633 | SpaceMouse Enterprise | combined |
| 256f:c635 | SpaceMouse Compact | separate |
| 256f:c636 | SpaceMouse Module | combined (assumed) |
| 256f:c63a | SpaceMouse Wireless (BT model, USB or Bluetooth) | combined |
| 256f:c641 | SpaceMouse (no buttons) | separate |
| 256f:c652 | Universal Receiver | combined |

"Assumed" means no source states the format for that model. Separate is assumed for the
Logitech-era models and combined for the newer ones, as for their known relatives.

### Reports

Every input report starts with its report ID. Values are little-endian.

- **Report 1, separate format:** translation X, Y, Z as three i16.
- **Report 2:** rotation RX, RY, RZ as three i16.
- **Report 1, combined format:** all six axes as six i16.
- **Report 3:** a bitmask of the buttons held; bit n is button n + 1.
- **Report 0x1C:** up to six u16 numbers of the buttons held (newer devices and the
  Enterprise); 0 means none. Some devices send both report 3 and report 0x1C, so a button
  counts as held if either says so.

Report 0x1D (long presses) and any other reports are ignored. On Windows every report is
padded to the longest one, so the format is taken from the model table, never from a
report's length.

Raw axes: X+ is the cap moved right, Y+ pulled toward you, Z+ pressed down; rotations are
about the same axes. AnvilCAD's frame is right, up, toward you, so
tx = X, ty = −Z, tz = Y, rx = RX, ry = −RZ, rz = RY. Values are divided by 350 (the
SpaceNavigator's declared logical range) and clamped to ±1.

### spacenavd

AnvilCAD speaks spacenavd's original protocol, which every version of the daemon serves.
The client connects to a Unix stream socket and sends nothing. The daemon sends 32-byte
packets of eight native-endian i32s:

- `[0, x, y, z, rx, ry, rz, period_ms]` for motion.
- `[1, button, …]` for a press and `[2, button, …]` for a release. Buttons count from 0.

spacenavd's frame is right, up, into the screen, so AnvilCAD negates z and rz. The socket is
looked for in libspnav's order: `$SPNAV_SOCKET`, a `socket = PATH` line in `/etc/spnavrc`,
`$XDG_RUNTIME_DIR/spnav.sock`, then `/var/run/spnav.sock`. spacenavd sends only changes,
so AnvilCAD applies no at-rest timeout to it. For devices read directly, half a second with no
report counts as at rest. Devices repeat their report about every 8 ms while the cap is
moved, and send zeros when it comes to rest.

### Sources

All were read on 2026-11-21 from the projects' default branches. They were used for facts
(IDs, layouts, constants) only; no code was copied.

- spacenavd, `src/dev.c`, `src/dev_usb_linux.c`, `src/proto_unix.c`, `src/proto.h`,
  `src/spnavd.h`, `src/cfgfile.c`: <https://github.com/FreeSpacenav/spacenavd>. Gave the
  device list and blacklist, the axis flags (`DF_SWAPYZ | DF_INVYZ`), the Linux button
  codes, the socket protocol and path, and the evdev grab. Issue #108 has the
  SpaceNavigator's descriptor (±350) and an observation of its report rate.
- libspnav, `src/spnav.c` and `src/util.c`: <https://github.com/FreeSpacenav/libspnav>.
  Gave the socket search order, the v1 handshake that AnvilCAD does not use, and the object
  mode conversion that fixes the rotation signs above.
- Blender, `intern/ghost/intern/GHOST_NDOFManager.cc`, `GHOST_SystemWin32.cc`,
  `GHOST_Types.hh`: <https://github.com/blender/blender>. Gave the device table, button
  masks, report 0x1C/0x1D and the button numbers.
- PySpaceMouse, `pyspacemouse/devices.toml`:
  <https://github.com/JakubAndrysek/PySpaceMouse>. Gave the format and button bits per
  model, and the recommended udev rule.
- OpenSCAD, `src/gui/input/HidApiInputDriver.cc`: <https://github.com/openscad/openscad>.
  Gave the Windows report padding and the normalisation by 350.
- systemd, `hwdb.d/70-mouse.hwdb` and `rules.d/70-uaccess.rules.in`:
  <https://github.com/systemd/systemd>. Gave which devices get `ID_INPUT_3D_MOUSE` and
  uaccess.
- hidapi, `mac/hidapi_darwin.h` and `windows/hid.c`: <https://github.com/libusb/hidapi>.
  Gave exclusive opening on macOS by default, and shared opening on Windows.
- 3Dconnexion forum posts and the PySpaceMouse troubleshooting notes, seen only as search
  results, report that 3DxWare blocks direct reads. This is unconfirmed.

### Unverified and open

- **Not tried on real hardware.** No device was available while this was written. The
  decoding is tested against reports built from the layouts above, and the navigation
  against exact camera positions. But the signs of the rotation axes on real devices are
  derived (from libspnav's object mode), not stated by any source. If a device turns the
  wrong way, `anvil.opt.mouse_3d_invert` fixes it, and an issue with the model name helps
  fix the default.
- The report format of the models marked "assumed" above.
- Whether hidraw still receives reports while spacenavd grabs the evdev node. AnvilCAD
  prefers spacenavd when it runs, so this does not arise.
- The number keys of the SpacePilot Pro, and the SpacePilot's buttons beyond the first 16
  (sources disagree), and the names of the SpacePilot's buttons.
- Whether 3DxWare always blocks direct reads on macOS and Windows, and whether reads on
  Windows continue when another window has focus (AnvilCAD ignores motion then anyway).
- The Bluetooth udev line, which matches on the kernel's HID device name.
- Battery and LED reports are not used.

### Not done

- Working alongside 3DxWare on macOS and Windows, through 3Dconnexion's own client
  interface. That interface's licence terms need checking first.
- Rotating about the selection or the point under the cursor; for now the view centre is the
  pivot.
- A settings window; the settings are in `init.lua` for now.
- The device's LED.
