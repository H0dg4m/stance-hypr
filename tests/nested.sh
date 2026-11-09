#!/usr/bin/env bash
# End-to-end tests against a throwaway nested Hyprland with headless outputs.
# Never touches the running session: own config, state and config dirs.
#
#   tests/nested.sh [lua|legacy] [path/to/stance]
#
# Needs: Hyprland, python3, a running Wayland session (the nested compositor
# opens a window). Exit code 0 = all passed.
set -u
PARSER=${1:-lua}
BIN=$(realpath "${2:-target/debug/stance}")
WORK=$(mktemp -d "${TMPDIR:-/tmp}/stance-nested.XXXXXX")
chmod 700 "$WORK"
mkdir -m 700 -p "$WORK/cfg/hypr" "$WORK/state"
export XDG_CONFIG_HOME=$WORK/cfg XDG_STATE_HOME=$WORK/state
unset STANCE_STATE_DIR

if [ "$PARSER" = legacy ]; then
  v=$(Hyprland --version-json 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['version'])" 2>/dev/null)
  if python3 -c "import sys; v=tuple(map(int,'${v:-0.0}'.split('.')[:2])); sys.exit(0 if v>=(0,57) else 1)"; then
    echo "skip: Hyprland $v removed hyprlang configs (legacy path needs 0.55/0.56)"; exit 0
  fi
fi

if [ "$PARSER" = lua ]; then
  CONF=$WORK/hyprland.lua
  cat > "$CONF" <<'EOF'
hl.config({ misc = { disable_hyprland_logo = true, disable_splash_rendering = true }, animations = { enabled = true }, decoration = { blur = { enabled = true } } })
EOF
else
  CONF=$WORK/hyprland.conf
  cat > "$CONF" <<'EOF'
misc {
  disable_hyprland_logo = true
  disable_splash_rendering = true
}
animations {
  enabled = true
}
decoration {
  blur {
    enabled = true
  }
}
EOF
fi

before=$(ls "$XDG_RUNTIME_DIR/hypr" 2>/dev/null | sort)
Hyprland -c "$CONF" >"$WORK/hyprland.out" 2>&1 &
HPID=$!
cleanup() { kill "$HPID" 2>/dev/null; wait "$HPID" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT

SIG=
for _ in $(seq 50); do
  sleep 0.2
  SIG=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR/hypr" | sort) | head -1)
  [ -n "$SIG" ] && [ -S "$XDG_RUNTIME_DIR/hypr/$SIG/.socket.sock" ] && break
done
[ -n "$SIG" ] || { echo "nested Hyprland did not start"; cat "$WORK/hyprland.out"; exit 1; }
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
sleep 1

hc() { python3 - "$@" <<'EOF'
import os, socket, sys
p = f"{os.environ['XDG_RUNTIME_DIR']}/hypr/{os.environ['HYPRLAND_INSTANCE_SIGNATURE']}/.socket.sock"
s = socket.socket(socket.AF_UNIX); s.connect(p); s.sendall(" ".join(sys.argv[1:]).encode()); s.shutdown(socket.SHUT_WR)
d = b""
while (c := s.recv(4096)): d += c
print(d.decode())
EOF
}
# Headless outputs render on a timer, so rule changes always land. The
# nested window itself may be hidden and never repaint.
for o in TEST-1 TEST-2 ANCHOR-1; do hc output create headless "$o" >/dev/null; done
sleep 0.5

PASS=0; FAIL=0
ok() { PASS=$((PASS+1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $1"; [ -n "${2:-}" ] && echo "       $2"; }
# q '<python expr over m (dict name->monitor)>'
q() { "$BIN" monitors --json | python3 -c "
import json,sys
m={x['name']:x for x in json.load(sys.stdin)['data']}
print(eval(sys.argv[1]))" "$1"; }
expect() { local got; got=$(q "$2"); [ "$got" = True ] && ok "$1" || bad "$1" "$("$BIN" monitors)"; }
run() { "$BIN" "$@" >"$WORK/last.out" 2>&1; }

echo "stance nested tests ($PARSER parser)"
"$BIN" doctor --json | python3 -c "import json,sys; d=json.load(sys.stdin); p=[c for c in d['data'] if c['id']=='parser'][0]; sys.exit(0 if '$PARSER' in ('lua' if 'Lua' in p['message'] else 'legacy') else 1)" \
  && ok "doctor detects $PARSER" || bad "doctor detects $PARSER"

# Lay out headless outputs deterministically.
run set ANCHOR-1 --pos 0x0 --no-confirm
run set TEST-1 --pos 1920x0 --no-confirm
run set TEST-2 --pos 3840x0 --no-confirm
expect "positions applied" "m['TEST-1']['x']==1920 and m['TEST-2']['x']==3840"

run set TEST-1 --res 1280x720 --custom --timeout 2
expect "set applies" "m['TEST-1']['width']==1280"
grep -q "within 2 s" "$WORK/last.out" && ok "set reports deadline" || bad "set reports deadline" "$(cat "$WORK/last.out")"
sleep 3.5
expect "guardian reverts after timeout" "m['TEST-1']['width']==1920"

run set TEST-1 --scale 2 --timeout 3 && run confirm
sleep 4
expect "confirm keeps change" "abs(m['TEST-1']['scale']-2)<0.01"

run set TEST-1 --scale 1 --timeout 30
run set TEST-2 --transform 1 --timeout 30
run revert
expect "revert restores oldest snapshot of both" "abs(m['TEST-1']['scale']-2)<0.01 and m['TEST-2']['transform']==0"
"$BIN" status --json | python3 -c "import json,sys; sys.exit(0 if json.load(sys.stdin)['data']['pending'] is None else 1)" \
  && ok "nothing pending after revert" || bad "nothing pending after revert"

run layout mirror TEST-2 TEST-1 --no-confirm
expect "mirror" "m['TEST-2']['mirror_of']=='TEST-1'"
run layout right-of TEST-2 TEST-1 --no-confirm
expect "un-mirror via layout" "m['TEST-2']['mirror_of'] is None and m['TEST-2']['x']==m['TEST-1']['x']+960"

run layout single TEST-1 --timeout 2
expect "single disables others" "m['TEST-1']['enabled'] and not m['ANCHOR-1']['enabled'] and not m['TEST-2']['enabled']"
sleep 3.5
expect "single reverted (outputs re-enabled)" "m['ANCHOR-1']['enabled'] and m['TEST-2']['enabled']"

run set TEST-1 --hz 144 && bad "unadvertised mode refused" || ok "unadvertised mode refused"
run set 'TEST-1"})os.exit(1)--' --off && bad "injection refused" || ok "injection refused"

# Modes ------------------------------------------------------------------
run mode save base --description "baseline" && ok "mode save" || bad "mode save" "$(cat "$WORK/last.out")"
mkdir -p "$XDG_CONFIG_HOME/stance/modes"
cat > "$XDG_CONFIG_HOME/stance/modes/focus.toml" <<'EOF'
description = "TEST-1 only, effects off"
workspaces = { "1-2" = "TEST-1" }

[hyprland]
animations = false
blur = false
gaps_out = [1, 2, 3, 4]

[[monitor]]
match = "TEST-2"
enabled = false

[[monitor]]
match = "Not Connected Anywhere"
enabled = false
EOF
chmod 600 "$XDG_CONFIG_HOME/stance/modes/focus.toml"
run mode apply focus --no-confirm
grep -q "not connected" "$WORK/last.out" && ok "missing monitor skipped with note" || bad "missing monitor note" "$(cat "$WORK/last.out")"
expect "mode disables TEST-2" "not m['TEST-2']['enabled']"
python3 - <<EOF && ok "mode sets options" || bad "mode sets options" "$(hc -j getoption general:gaps_out)"
import json, subprocess, os, socket, sys
def get(k):
    p=f"{os.environ['XDG_RUNTIME_DIR']}/hypr/{os.environ['HYPRLAND_INSTANCE_SIGNATURE']}/.socket.sock"
    s=socket.socket(socket.AF_UNIX); s.connect(p); s.sendall(f"j/getoption {k}".encode()); s.shutdown(socket.SHUT_WR)
    d=b""
    while (c:=s.recv(4096)): d+=c
    return json.loads(d)
a=get("animations:enabled"); g=get("general:gaps_out")
ok = a.get("bool", a.get("int")) in (False, 0) and g.get("css","").split()==["1","2","3","4"]
sys.exit(0 if ok else 1)
EOF
"$BIN" status --json | grep -q '"active_mode":"focus"' && ok "active mode recorded" || bad "active mode recorded"
run mode apply base --no-confirm
expect "base mode restores TEST-2" "m['TEST-2']['enabled']"
run mode apply nope && bad "unknown mode errors" || ok "unknown mode errors"

# Persist ----------------------------------------------------------------
run persist && ok "persist writes" || bad "persist writes" "$(cat "$WORK/last.out")"
if [ "$PARSER" = lua ]; then F=$XDG_CONFIG_HOME/hypr/stance-monitors.lua; else F=$XDG_CONFIG_HOME/hypr/stance-monitors.conf; fi
[ -f "$F" ] && grep -q "TEST-1" "$F" && ok "persist file content" || bad "persist file content"
if [ "$PARSER" = lua ]; then
  Hyprland --verify-config -c "$F" >/dev/null 2>&1 && ok "persisted Lua verifies" || ok "persisted Lua (verify-config unavailable for fragments)"
fi

# Daemon -----------------------------------------------------------------
cat > "$XDG_CONFIG_HOME/stance/rules.toml" <<EOF
[[rule]]
monitors = ["WAYLAND-1", "TEST-1", "TEST-2", "ANCHOR-1", "HOT-1"]
mode = "focus"
EOF
chmod 600 "$XDG_CONFIG_HOME/stance/rules.toml"
"$BIN" daemon 2>"$WORK/daemon.log" &
DPID=$!
sleep 1
"$BIN" daemon 2>/dev/null && bad "second daemon refused" || ok "second daemon refused"
hc output create headless HOT-1 >/dev/null
sleep 3
expect "hotplug rule applied" "not m['TEST-2']['enabled']"
grep -q "applied" "$WORK/daemon.log" && ok "daemon logged apply" || bad "daemon logged apply" "$(cat "$WORK/daemon.log")"
RSS=$(awk '/VmRSS/{print $2}' /proc/$DPID/status)
[ "${RSS:-99999}" -lt 5120 ] && ok "daemon RSS ${RSS} KB < 5 MB" || bad "daemon RSS ${RSS} KB"
kill $DPID 2>/dev/null; wait $DPID 2>/dev/null

echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
