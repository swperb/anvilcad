# Testing on the Windows VM

AnvilCAD has a Windows 11 Pro virtual machine for building, testing and looking at the app
on Windows before a change reaches CI or users. `scripts/winvm/winvm` drives it from macOS
or Linux. Coding agents use it the same way a person would: it needs no window on the host
and never touches the host's mouse or keyboard.

## The machine

| | |
|---|---|
| Host | TrueNAS SCALE 24.10 at `truenas-scale-1` (on the Tailnet), VM `anvilwin11` |
| Hardware | 4 vCPUs (host passthrough, Intel i5-7500), 8 GB RAM of which 6 GB is guaranteed (the VirtIO balloon driver returns the rest when the NAS needs it; alongside Home Assistant the NAS can promise only about 8 GB, so a fixed 8 GB sometimes failed to start), 160 GB VirtIO disk on the zvol `nas1/vms/anvil-win11`, UEFI (OVMF) and a TPM 2.0; no Secure Boot, because TrueNAS 24.10's libvirt setup cannot start the Secure Boot firmware, so the install bypasses that check |
| Network | VirtIO NIC bridged to the LAN (`br0`), so it gets its own DHCP address on 192.168.1.0/24 |
| Access | OpenSSH (key only, PowerShell shell) and Remote Desktop, local account `anvil` |
| Licence | Windows 11 Pro, activated with the owner's licence |
| Toolchain | Rust stable (x86_64-pc-windows-msvc, as in CI) with rustfmt and clippy, Visual Studio Build Tools (C++), Git |

The VM's settings live outside the repository in `~/.config/anvilcad/winvm.env`
(`TRUENAS_HOST`, `TRUENAS_USER`, `WINVM_NAME`, `WINVM_ZVOL`, `WINVM_USER`, `WINVM_MAC`,
optionally `WINVM_HOST` to skip the IP lookup and `WINVM_JUMP` to change the SSH relay, and `WIN_PASS` for `provision`). Keep that
file at mode 600. The harness reaches TrueNAS as `truenas_admin` over SSH and calls its
middleware (`midclt`) for power and snapshots; it reaches Windows over SSH with your
`~/.ssh/id_ed25519` key, relayed through `nc` on the NAS (TrueNAS turns off SSH port
forwarding, so `ProxyJump` is refused). That works from anywhere on the Tailnet; set
`WINVM_JUMP=` (empty) to connect straight to the VM when you are on its LAN.

## Everyday use

```bash
scripts/winvm/winvm status
```

```bash
scripts/winvm/winvm check
```

`check` copies the working tree, uncommitted edits included, into the VM and runs
`cargo clippy --workspace --all-targets` and `cargo test --workspace` there. Each checkout
or worktree gets its own source tree (`C:\anvil\src\<name>`) and target dir
(`C:\anvil\target\<name>`), so builds stay incremental and several agents can share the VM.

Any other cargo command works the same way:

```bash
scripts/winvm/winvm cargo test -p anvil-io --release
```

### Headless screenshots

`winvm agent` runs [`anvilcad-agent`](AGENT_HARNESS.md) in the VM. Paths are relative to
the synced tree; anything written under `target/winvm-out/` is copied back to
`target/winvm/` on the host.

```bash
scripts/winvm/winvm agent --sample --screenshot target/winvm-out/sample.png
```

The VM has no GPU, so wgpu uses Microsoft's WARP software adapter, the same path a Windows
machine without a working driver takes.

### The real windowed app

The `anvil` account signs in to the VM's desktop automatically, so the real `anvilcad.exe`
can be run, clicked through and screenshotted there: native file dialogs, DPI scaling,
the taskbar and window chrome included. This is the VM's own desktop, not the host's.

```bash
scripts/winvm/winvm gui launch
```

```bash
scripts/winvm/winvm gui shot target/winvm/desktop.png
```

`gui windows` lists windows with their rectangles in desktop pixels, `gui click X Y`
clicks, `gui type TEXT` types, `gui key '^s'` sends keys in SendKeys syntax and
`gui close` quits the app. `gui resolution 1920x1080` changes the desktop size (it
starts at 1024x768, smaller than the app's default window). Each action runs as a scheduled task in the signed-in session.
If someone left that session through Remote Desktop, the harness moves it back to the
console first.

### Snapshots

The disk is a ZFS zvol, so snapshots are instant and cheap:

```bash
scripts/winvm/winvm snapshot before-installer-test
```

```bash
scripts/winvm/winvm rollback before-installer-test
```

`rollback` stops the VM, rolls the disk back (destroying later snapshots) and starts it
again; `winvm wait` blocks until SSH answers. Roll back to `activated`: it has the
toolchain and the activated licence. The older `fresh-install` and `provisioned` snapshots
predate activation, so rolling back to them (which also destroys every later snapshot)
means activating Windows again.

### By hand

`scripts/winvm/winvm ssh` opens PowerShell in the VM. `scripts/winvm/winvm ip` prints its
address for Remote Desktop. Sign in as `.\anvil` (the `.\` marks a local account) with the
password in `WIN_PASS`. If the client hangs at "Configuring remote PC", it is waiting on
you: look for a hidden prompt to trust the VM's self-signed certificate, and save the
credentials with the PC so it has nothing left to ask. Remote Desktop takes over the
auto-logon desktop session; after you disconnect, the next `winvm gui` action moves that
session back to the console.

## How it was built

The VM was installed unattended from Microsoft's Windows 11 multi-edition ISO:

- The ISO was remastered on the host with `wimlib` and `xorriso`: only the Pro image, split
  into `install.swm` parts under 4 GB; the VirtIO storage and network drivers from the
  Fedora `virtio-win` ISO in `$WinPEDriver$`; an `autounattend.xml` answer file; and the
  `efisys_noprompt.bin` boot image, so it boots with no "press any key".
- The answer file partitions the disk, installs Pro with Microsoft's generic Pro setup key
  (which selects the edition but does not activate), creates the `anvil` account and runs a
  first-logon script that installs the VirtIO guest tools and enables OpenSSH and Remote
  Desktop.
- `winvm provision` then installs the Visual Studio Build Tools, Rust and Git, turns on
  persistent auto-logon, and excludes `C:\anvil` and the Rust directories from Defender.

Windows was then activated by the owner with their own licence, and the `activated`
snapshot taken.

## Moving the VM to NVMe

The VM's disk is on the HDD mirror (`nas1`) for now because the NAS's only NVMe is its
boot drive, and TrueNAS does not support VM disks on `boot-pool`. Once TrueNAS boots from
another device and the NVMe is a pool of its own (say `fast`), move the disk with ZFS and
point the VM and the harness at it. As `truenas_admin`, with the VM stopped:

```bash
midclt call zfs.snapshot.create '{"dataset":"nas1/vms/anvil-win11","name":"move"}'
```

```bash
sudo zfs send -R nas1/vms/anvil-win11@move | sudo zfs recv fast/vms/anvil-win11
```

Then change the VM's DISK device path to `/dev/zvol/fast/vms/anvil-win11` (in the UI or
with `midclt call vm.device.update`), set `WINVM_ZVOL=fast/vms/anvil-win11` in
`~/.config/anvilcad/winvm.env`, start the VM and check it before you destroy the old zvol.
The Home Assistant VM's `haos.img` moves the same way, as a file copy.

## Limits

- The VM shares a 4-core, 16 GB NAS with other services, so a cold `cargo test --workspace`
  is slow (expect tens of minutes); incremental runs are much quicker.
- There is no GPU, so rendering is WARP only (DX12 on Microsoft's software rasteriser).
  Hardware-specific driver bugs still need a real machine. The NAS has an RTX 3060, but it
  cannot be passed through as things stand: it is the host's only GPU (the i5-7500's HD 630
  is off in the firmware) and TrueNAS keeps one for the host, and its IOMMU group includes
  the CPU's PCIe root port (`00:01.0`), which TrueNAS 24.10 refuses to isolate. Passing it
  through would also take it from the host while the VM runs.
- It is x86-64 only; Windows on Arm is not covered.
