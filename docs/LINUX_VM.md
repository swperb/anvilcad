# Testing on the Linux VM

AnvilCAD has an Ubuntu 24.04 LTS virtual machine, beside the [Windows one](WINDOWS_VM.md), for
building, testing and looking at the app on a real Linux desktop before a change reaches CI or
users. `scripts/linuxvm/linuxvm` drives it from macOS or Linux. Like `winvm`, it needs no window
on the host and never touches the host's mouse or keyboard.

## The machine

| | |
|---|---|
| Host | TrueNAS SCALE 24.10 at `truenas-scale-1` (on the Tailnet), VM `anvillinux` |
| Hardware | 4 vCPUs (host passthrough, Intel i5-7500), 8 GB RAM, 120 GB VirtIO disk on the zvol `nas1/vms/anvil-linux`, UEFI |
| Network | VirtIO NIC bridged to the LAN (`br0`), DHCP |
| Desktop | GNOME 46, signed in automatically as `anvil`, on Wayland (switchable to X11), 1920x1080, no screen lock |
| Access | OpenSSH, key only; `anvil` has passwordless sudo (a disposable test VM) |
| Toolchain | Rust stable with rustfmt and clippy, build-essential, and CI's Linux packages (`libxkbcommon-dev`, `libwayland-dev`, `libgtk-3-dev`, `mesa-vulkan-drivers`) |
| Test tools | `gnome-screenshot`, `ydotool` (input on Wayland and X11), `xdotool` and `wmctrl` (X11), headless `sway` with `grim`, and `Xvfb` |

Settings live outside the repository in `~/.config/anvilcad/linuxvm.env` (`LINUXVM_NAME`,
`LINUXVM_ZVOL`, `LINUXVM_USER`, `LINUXVM_MAC`, optionally `LINUXVM_HOST` to skip the IP lookup
and `LINUXVM_JUMP` to change the SSH relay), at mode 600. Settings shared by both VMs go in
`~/.config/anvilcad/vms.env` (see Memory below).

## Everyday use

```bash
scripts/linuxvm/linuxvm status
```

```bash
scripts/linuxvm/linuxvm check
```

`check` copies the working tree, uncommitted edits included, into the VM with rsync and runs
the same three commands as CI's Linux job: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets` and `cargo test --workspace`. Each checkout or worktree has its own source tree
and target dir in the VM (`~/anvil/src/<name>`, `~/anvil/target/<name>`), so builds stay
incremental and several agents can share the VM. `linuxvm cargo ...` runs any other cargo
command.

### Headless screenshots

```bash
scripts/linuxvm/linuxvm agent --sample --screenshot target/linuxvm-out/sample.png
```

`anvilcad-agent` needs no display; it renders with Mesa's lavapipe. Anything written under
`target/linuxvm-out/` is copied back to `target/linuxvm/`.

### The real windowed app, three ways

| | Command | What it tests |
|---|---|---|
| GNOME desktop | `linuxvm gui launch` | What users see: mutter's window management and client-side decorations, on Wayland or (after `linuxvm gui session x11`) X11 |
| Headless Wayland | `linuxvm gui launch --headless-wayland` | The Wayland back end alone, in a fresh sway compositor with nothing else on screen |
| Headless X11 | `linuxvm gui launch --headless-x11` | The X11 back end alone, under Xvfb |

Then `gui shot [out.png]` screenshots whichever is running, `gui click X Y`, `gui move X Y`,
`gui type TEXT` and `gui key ctrl+s` drive it through ydotool (which works under both Wayland
and X11), `gui windows` lists windows (X11 and Xwayland), and `gui close` quits the app and any
headless compositor. `gui session` prints the GNOME session type and `gui session wayland|x11`
switches it (the desktop restarts).

Ubuntu 24.04's ydotool moves the pointer only relatively, so the harness pushes it into the top
left corner before moving to X,Y, and provisioning turns pointer acceleration off so that moves
are 1:1.

### Snapshots

As with Windows: `linuxvm snapshot NAME`, `linuxvm snapshots`, `linuxvm rollback NAME` and
`linuxvm snapshot delete NAME`.
`fresh-install` is the system as installed; `provisioned` adds Rust and the session helpers.

## Memory: one VM at a time, or both at once

The NAS has 16 GB, shared by Home Assistant (4 GB) and the two test VMs (8 GB each), so today
they cannot all run at once. `ANVIL_VM_MODE` says what to do, set in
`~/.config/anvilcad/vms.env` or per command with `--auto`, `--parallel` or `--exclusive`
before the command (`linuxvm --parallel check`):

| Mode | Behaviour |
|---|---|
| `auto` (default) | Start the VM. If TrueNAS cannot fit it, shut down the other test VM, once no other agent is using it, and start again. With enough memory both simply run. |
| `parallel` | Never stop another VM. Use once the NAS has the memory for every test VM (24 GB is enough, 32 GB comfortable); a VM that does not fit is then an error, not a shutdown. |
| `exclusive` | Always shut down the other test VM (once idle) first, for timing-sensitive runs. |

Every command that uses a VM holds a lease on it: a file in `~/.anvil-leases` on the NAS,
refreshed each minute while the command runs. `auto` and `exclusive` never shut down a VM with
a live lease; they wait for it (up to `ANVIL_VM_WAIT` seconds, 1800 by default) and then give
up with a message. So parallel agents can share the NAS without pulling a VM out from under
each other's builds. Only the VMs in `TEST_VMS` (default `anvilwin11 anvillinux`) are ever
stopped; Home Assistant never is.

Once the RAM is upgraded:

```bash
echo 'ANVIL_VM_MODE=parallel' >> ~/.config/anvilcad/vms.env
```

## CI

The VM runs the same commands as CI's `ubuntu-latest` job, so a change that passes
`linuxvm check` passes there too. CI also covers the parts the VM exercises by hand: the
headless harness's tests drive the Lua configuration, key bindings, command palette and
scripting on Linux, macOS and Windows, and a shellcheck step lints these scripts. The VMs are
not registered as GitHub runners: on a public repository, a pull request from a fork could
then run code on the NAS and its LAN.

## How it was built

From Ubuntu's 24.04.5 live server ISO, installed unattended:

- The ISO's GRUB menu was patched in place, at the same byte length, to boot straight into
  `autoinstall` with a 1 second timeout. The autoinstall config came from a small `CIDATA` seed
  ISO (cloud-init's NoCloud source).
- It installs `ubuntu-desktop` and the packages above, creates `anvil` with the maintainer's SSH
  key, switches networking to NetworkManager, turns on GDM auto-login and turns off the screen
  lock.
- `linuxvm provision` then installs Rust, records the desktop session's environment at each
  login (so SSH commands reach its Wayland or X display and D-Bus), fixes the display at
  1920x1080 and makes pointer motion 1:1 for ydotool.
