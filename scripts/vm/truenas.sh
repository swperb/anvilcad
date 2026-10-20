# Shared by scripts/winvm/winvm and scripts/linuxvm/linuxvm: power, addresses and ZFS
# snapshots of an AnvilCAD test VM on the maintainer's TrueNAS box, through its middleware.
#
# Before sourcing, the tool sets TOOL (its name, for messages), CONFIG (its env file) and
# VM_NAME, VM_ZVOL, VM_LOGIN, VM_MAC, VM_HOST and VM_JUMP from its own settings.
# shellcheck shell=bash

# Settings every VM tool shares: TRUENAS_HOST, TRUENAS_USER, TEST_VMS and ANVIL_VM_MODE.
VMS_CONFIG="${VMS_CONFIG:-$HOME/.config/anvilcad/vms.env}"
# shellcheck source=/dev/null
[ -f "$VMS_CONFIG" ] && source "$VMS_CONFIG"
TRUENAS_HOST="${TRUENAS_HOST:-truenas-scale-1}"
TRUENAS_USER="${TRUENAS_USER:-truenas_admin}"
# The test VMs. They share the NAS's memory with each other and with services that must stay
# up; only these are ever stopped to make room (docs/LINUX_VM.md, "Memory").
TEST_VMS="${TEST_VMS:-anvilwin11 anvillinux}"
# How test VMs share memory:
#   auto       start; if TrueNAS cannot fit this VM, shut down the other test VMs once they
#              are idle (no harness command using them), then start (the default)
#   parallel   never stop another VM; every test VM runs side by side, and a VM that does not
#              fit is an error (use once the NAS has the memory for all of them)
#   exclusive  always shut down the other test VMs (once idle) before using this one
ANVIL_VM_MODE="${ANVIL_VM_MODE:-auto}"
# How long auto and exclusive wait for another agent to finish with a VM before giving up.
ANVIL_VM_WAIT="${ANVIL_VM_WAIT:-1800}"

SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30 -o StrictHostKeyChecking=accept-new)
VM_SSH_OPTS=("${SSH_OPTS[@]}")
# TrueNAS turns off sshd's TCP forwarding, so ProxyJump is refused; nc on the NAS relays instead.
# Both hops keep alive, so a long build is not dropped by an idle relay.
[ -n "$VM_JUMP" ] && VM_SSH_OPTS+=(-o "ProxyCommand=ssh -o BatchMode=yes -o ServerAliveInterval=30 $VM_JUMP nc %h %p")

die() { echo "$TOOL: $*" >&2; exit 1; }

nas() { ssh "${SSH_OPTS[@]}" -l "$TRUENAS_USER" "$TRUENAS_HOST" "$@"; }

# Calls a TrueNAS middleware method. Arguments after the method are JSON values.
midclt() {
  local q="" a
  for a in "$@"; do q+=" $(printf '%q' "$a")"; done
  nas "midclt call$q"
}

# Prints one field of a VM's record: vm_field NAME PYTHON-EXPRESSION-ON-v.
vm_field() {
  midclt vm.query "[[\"name\",\"=\",\"$1\"]]" '{"get":true}' |
    python3 -c "import json,sys; v=json.load(sys.stdin); print($2)"
}

vm_id() { vm_field "$VM_NAME" 'v["id"]'; }
vm_state() { vm_field "${1:-$VM_NAME}" 'v["status"]["state"]'; }

# The VM's MAC address, from the config or from its NIC device.
vm_mac() {
  if [ -n "$VM_MAC" ]; then echo "$VM_MAC"; return; fi
  vm_field "$VM_NAME" 'next(d["attributes"]["mac"] for d in v["devices"] if d["dtype"] == "NIC")'
}

# The VM's LAN address: VM_HOST if set, otherwise found in the NAS's neighbour table.
vm_ip() {
  if [ -n "$VM_HOST" ]; then echo "$VM_HOST"; return; fi
  local mac ip find
  mac="$(vm_mac | tr 'A-F' 'a-f')"
  [ -n "$mac" ] || die "the VM has no NIC with a MAC address"
  find="ip -4 neigh show dev br0 | awk '\$2==\"lladdr\" && tolower(\$3)==\"$mac\" {print \$1; exit}'"
  ip="$(nas "$find")"
  if [ -z "$ip" ]; then
    # The neighbour entry appears once the NAS has talked to the VM; ping the subnet once.
    nas 'n=$(ip -4 -o addr show br0 | awk "{print \$4}" | cut -d. -f1-3); for i in $(seq 1 254); do ping -c1 -W1 "$n.$i" >/dev/null 2>&1 & done; wait' || true
    ip="$(nas "$find")"
  fi
  [ -n "$ip" ] || die "could not find the VM's IP (MAC $mac). Is it running? Set its HOST in $CONFIG to skip the lookup."
  echo "$ip"
}

HOST_CACHE=""
host() { [ -n "$HOST_CACHE" ] || HOST_CACHE="$(vm_ip)"; echo "$HOST_CACHE"; }

vm_stop_by_name() {
  local id; id="$(vm_field "$1" 'v["id"]')"
  midclt -j 1 vm.stop "$id" '{"force_after_timeout":true}' >/dev/null 2>&1
}

# Leases: every harness command that uses a VM holds a lease on it (a file on the NAS,
# refreshed each minute while the command runs), so no other agent shuts that VM down to
# make room for another. Leases older than five minutes are from commands that died.
LEASE=""
lease_take() {
  [ -n "$LEASE" ] && return 0
  LEASE=".anvil-leases/$VM_NAME.$(hostname -s 2>/dev/null || hostname).$$"
  nas "mkdir -p .anvil-leases && touch $LEASE" || { LEASE=""; return 0; }
  ( while sleep 60; do nas "touch $LEASE" >/dev/null 2>&1 || true; done ) &
  LEASE_PID=$!
  trap 'kill "$LEASE_PID" 2>/dev/null; nas "rm -f $LEASE" >/dev/null 2>&1 || true' EXIT
}

# Whether another harness command holds a live lease on VM $1.
vm_busy() {
  local mine="${LEASE:-none}"
  nas "find .anvil-leases -name '$1.*' -mmin -5 2>/dev/null | grep -v -F '$mine' | grep -q ." 2>/dev/null
}

# Shuts down the other running test VMs once no one is using them.
stop_other_vms() {
  local other waited
  for other in $TEST_VMS; do
    [ "$other" = "$VM_NAME" ] && continue
    [ "$(vm_state "$other" 2>/dev/null)" = RUNNING ] || continue
    waited=0
    while vm_busy "$other"; do
      [ "$waited" -ge "$ANVIL_VM_WAIT" ] && die "$other is still in use by another agent after ${ANVIL_VM_WAIT}s; try again later, or use --parallel once the NAS has the memory for both"
      [ "$waited" -eq 0 ] && echo "$TOOL: waiting for another agent to finish with $other (ANVIL_VM_WAIT=${ANVIL_VM_WAIT}s)"
      sleep 30; waited=$((waited + 30))
    done
    echo "$TOOL: shutting down $other to make room for $VM_NAME (ANVIL_VM_MODE=$ANVIL_VM_MODE)"
    vm_stop_by_name "$other"
  done
}

# Starts the VM following ANVIL_VM_MODE (see the top of this file). When TrueNAS still cannot
# guarantee the memory, starting with overcommit lets ZFS give back cache as the VM needs it.
vm_start() {
  case "$ANVIL_VM_MODE" in auto|parallel|exclusive) ;; *) die "ANVIL_VM_MODE is auto, parallel or exclusive, not '$ANVIL_VM_MODE'" ;; esac
  [ "$ANVIL_VM_MODE" = exclusive ] && stop_other_vms
  [ "$(vm_state)" = RUNNING ] && return 0
  local id err
  id="$(vm_id)"
  err="$(midclt vm.start "$id" 2>&1 >/dev/null)" && { echo "$TOOL: started $VM_NAME"; return 0; }
  case "$err" in *ENOMEM*) ;; *) die "could not start $VM_NAME: $(echo "$err" | head -1)" ;; esac
  if [ "$ANVIL_VM_MODE" = auto ]; then
    stop_other_vms
    err="$(midclt vm.start "$id" 2>&1 >/dev/null)" && { echo "$TOOL: started $VM_NAME"; return 0; }
    case "$err" in *ENOMEM*) ;; *) die "could not start $VM_NAME: $(echo "$err" | head -1)" ;; esac
  fi
  if ! midclt vm.start "$id" '{"overcommit":true}' >/dev/null 2>&1; then
    [ "$ANVIL_VM_MODE" = parallel ] && die "not enough memory to run $VM_NAME beside the other VMs (ANVIL_VM_MODE=parallel); add RAM, or use --auto to take turns"
    die "could not start $VM_NAME, even with overcommit"
  fi
  echo "$TOOL: started $VM_NAME (with overcommit; ZFS gives back cache as the VM needs it)"
}

vm_stop() { midclt -j 1 vm.stop "$(vm_id)" '{"force_after_timeout":true}' >/dev/null 2>&1; echo "$TOOL: stopped $VM_NAME"; }

# Waits until SSH answers in the VM.
vm_wait() {
  local deadline=$(( $(date +%s) + ${1:-900} ))
  until ssh "${VM_SSH_OPTS[@]}" -o ConnectTimeout=5 "$VM_LOGIN@$(host)" 'exit 0' >/dev/null 2>&1; do
    [ "$(date +%s)" -lt "$deadline" ] || die "timed out waiting for SSH"
    HOST_CACHE=""; sleep 10
  done
  echo "$TOOL: $(host) is up"
}

# Takes a lease on the VM and makes sure it is running and answering SSH, starting it (and
# making room, as ANVIL_VM_MODE says) if needed.
ensure_up() {
  lease_take
  if [ "$(vm_state)" != RUNNING ] || [ "$ANVIL_VM_MODE" = exclusive ]; then vm_start; vm_wait; fi
}

# Takes --auto, --parallel or --exclusive from before the command and leaves the remaining
# arguments in ARGS.
ARGS=()
parse_mode_flags() {
  ARGS=()
  local a
  for a in "$@"; do
    case "$a" in
      --auto|--parallel|--exclusive) [ ${#ARGS[@]} -eq 0 ] && { ANVIL_VM_MODE="${a#--}"; continue; } ;;
    esac
    ARGS+=("$a")
  done
}

vm_snapshot() {
  if [ "${1:-}" = delete ]; then
    shift
    vm_snapshot_delete "$@"
    return
  fi
  local name="${1:-$(date -u +%Y%m%d-%H%M%S)}"
  midclt zfs.snapshot.create "{\"dataset\":\"$VM_ZVOL\",\"name\":\"$name\"}" >/dev/null
  echo "$TOOL: snapshot $VM_ZVOL@$name"
}

vm_snapshots() {
  midclt zfs.snapshot.query "[[\"dataset\",\"=\",\"$VM_ZVOL\"]]" '{"select":["name"]}' |
    python3 -c 'import json,sys; [print(s["name"]) for s in json.load(sys.stdin)]'
}

# The snapshots the VM docs tell people to roll back to. Deleting one needs --force.
BASELINE_SNAPSHOTS="fresh-install provisioned activated"

# Deletes snapshots of the VM's disk by name. Every name is checked before any is deleted,
# so a typo deletes nothing.
vm_snapshot_delete() {
  local force="" names=() n existing
  for n in "$@"; do
    case "$n" in
      --force) force=1 ;;
      -*) die "unknown option $n" ;;
      *) names+=("$n") ;;
    esac
  done
  [ ${#names[@]} -gt 0 ] || die "usage: $TOOL snapshot delete [--force] <name>..."
  existing=" $(vm_snapshots | sed 's/.*@//' | tr '\n' ' ') "
  for n in "${names[@]}"; do
    case "$existing" in *" $n "*) ;; *) die "no snapshot $VM_ZVOL@$n" ;; esac
    case " $BASELINE_SNAPSHOTS " in
      *" $n "*) [ -n "$force" ] || die "$n is a baseline snapshot; add --force to delete it" ;;
    esac
  done
  for n in "${names[@]}"; do
    midclt zfs.snapshot.delete "$VM_ZVOL@$n" >/dev/null
    echo "$TOOL: deleted snapshot $VM_ZVOL@$n"
  done
}

# Rolls the VM's disk back to a snapshot and starts it again. Later snapshots are destroyed,
# as with zfs rollback -r.
vm_rollback() {
  [ $# -eq 1 ] || die "usage: $TOOL rollback <snapshot name>"
  [ "$(vm_state)" = RUNNING ] && vm_stop >/dev/null
  midclt zfs.snapshot.rollback "$VM_ZVOL@$1" '{"recursive":true,"force":true}' >/dev/null
  vm_start
  vm_wait
  echo "$TOOL: rolled back to $VM_ZVOL@$1"
}

# Commands every VM tool shares. Returns 1 if the command is not one of them.
vm_common_command() {
  local c="$1"; shift
  case "$c" in
    ip) vm_ip ;;
    start) lease_take; vm_start; vm_wait ;;
    stop) vm_stop ;;
    restart) midclt -j 1 vm.restart "$(vm_id)" >/dev/null 2>&1; HOST_CACHE=""; sleep 15; vm_wait ;;
    wait) vm_wait "$@" ;;
    snapshot) vm_snapshot "$@" ;;
    snapshots) vm_snapshots ;;
    rollback) vm_rollback "$@" ;;
    *) return 1 ;;
  esac
}
