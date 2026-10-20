#!/usr/bin/env bash
# Installs or updates what the Ubuntu test VM needs to build, test and run AnvilCAD.
# Run through scripts/linuxvm/linuxvm provision; safe to run again.
set -euo pipefail
mkdir -p ~/anvil

# Rust, through rustup, with the components CI uses.
if [ ! -x ~/.cargo/bin/rustup ]; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable -c rustfmt -c clippy
else
  ~/.cargo/bin/rustup update stable --no-self-update
fi

# Record the desktop session's environment at each login, so commands run over SSH can
# reach the same Wayland or X display and D-Bus session (scripts/linuxvm/linuxvm gui ...).
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/anvil-session-env.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=AnvilCAD test session environment
Exec=sh -c 'mkdir -p "$HOME/anvil" && env | grep -E "^(DISPLAY|WAYLAND_DISPLAY|XAUTHORITY|DBUS_SESSION_BUS_ADDRESS|XDG_SESSION_TYPE|XDG_RUNTIME_DIR|XDG_CURRENT_DESKTOP)=" | sed "s/^/export /" > "$HOME/anvil/session.env"'
X-GNOME-Autostart-Phase=Applications
NoDisplay=true
EOF

# A fixed 1920x1080 desktop and no first-run tour, so screenshots are comparable.
mkdir -p ~/.config
cat > ~/.config/monitors.xml <<'EOF'
<monitors version="2">
  <configuration>
    <logicalmonitor>
      <x>0</x><y>0</y><scale>1</scale><primary>yes</primary>
      <monitor>
        <monitorspec><connector>Virtual-1</connector><vendor>unknown</vendor><product>unknown</product><serial>unknown</serial></monitorspec>
        <mode><width>1920</width><height>1080</height><rate>60.000</rate></mode>
      </monitor>
    </logicalmonitor>
  </configuration>
</monitors>
EOF
sudo apt-get remove -y gnome-initial-setup >/dev/null 2>&1 || true

# ydotool injects input through /dev/uinput, which works under Wayland and X11 alike. Its
# pointer moves are relative, so make them 1:1: no acceleration, default speed. Kept in the
# dconf system database so it holds in every session.
sudo modprobe uinput
echo uinput | sudo tee /etc/modules-load.d/uinput.conf >/dev/null
printf "[org/gnome/desktop/peripherals/mouse]\naccel-profile='flat'\nspeed=0.0\n\n[org/gnome/desktop/peripherals/touchpad]\naccel-profile='flat'\n" |
  sudo tee /etc/dconf/db/local.d/10-anvil-pointer >/dev/null
sudo dconf update

# Restart the desktop once so the autostart entry and monitor layout take effect.
if [ ! -f ~/anvil/session.env ]; then sudo systemctl restart gdm3; sleep 15; fi
~/.cargo/bin/rustc --version
~/.cargo/bin/cargo --version
echo "provision: done"
