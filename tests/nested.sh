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
wl_before=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort)
Hyprland -c "$CONF" >"$WORK/hyprland.out" 2>&1 &
HPID=$!
# Only hyprsunsets whose environment carries the NESTED instance signature
# (the user's own one has the live signature and is never matched).
nested_sunsets() {
  local p
  for p in /proc/[0-9]*; do
    [ "$(cat "$p/comm" 2>/dev/null)" = hyprsunset ] || continue
    tr '\0' '\n' < "$p/environ" 2>/dev/null | grep -qx "HYPRLAND_INSTANCE_SIGNATURE=${SIG:-none}" && echo "${p#/proc/}"
  done
}
kill_sunsets() { local p; for p in $(nested_sunsets); do kill "$p" 2>/dev/null; done; sleep 0.3; }
cleanup() { kill_sunsets; kill "$HPID" 2>/dev/null; wait "$HPID" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT

SIG=
for _ in $(seq 50); do
  sleep 0.2
  SIG=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR/hypr" | sort) | head -1)
  [ -n "$SIG" ] && [ -S "$XDG_RUNTIME_DIR/hypr/$SIG/.socket.sock" ] && break
done
[ -n "$SIG" ] || { echo "nested Hyprland did not start"; cat "$WORK/hyprland.out"; exit 1; }
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
NWL=$(comm -13 <(echo "$wl_before") <(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort) | head -1)
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

# Arrange (panel drag) and position memory -------------------------------
X2=$(q "m['TEST-2']['x']")
run arrange "TEST-2=${X2}x300" --no-confirm && ok "arrange moves" || bad "arrange moves" "$(cat "$WORK/last.out")"
expect "arrange position" "m['TEST-2']['y']==300"
run arrange TEST-2=0x0 --no-confirm && bad "arrange overlap refused" || ok "arrange overlap refused"
run set TEST-2 --off --no-confirm
expect "off for memory test" "not m['TEST-2']['enabled']"
run set TEST-2 --on --no-confirm
expect "on restores last position" "m['TEST-2']['enabled'] and m['TEST-2']['x']==$X2 and m['TEST-2']['y']==300"
run arrange "TEST-2=${X2}x0" --no-confirm

# Mode / rules JSON (panel editor) -----------------------------------------
run mode put edited --data '{"description":"from json","night_light":false,"monitor":[{"match":"TEST-1","scale":1.0}]}' \
  && ok "mode put" || bad "mode put" "$(cat "$WORK/last.out")"
"$BIN" mode show edited --json | grep -q '"from json"' && ok "mode put round-trip" || bad "mode put round-trip"
run mode put edited --data '{"description":"x"}' && bad "mode put needs --force" || ok "mode put needs --force"
run mode put evil --data '{"exec":"rm -rf ~"}' && bad "mode put rejects unknown keys" || ok "mode put rejects unknown keys"
run rules put --data '{"rule":[{"monitors":["TEST-1"],"mode":"nope"}]}' && bad "rules put needs existing mode" || ok "rules put needs existing mode"
run rules put --data '{"rule":[{"monitors":["TEST-1","TEST-2","ANCHOR-1"],"mode":"edited"}]}' \
  && ok "rules put" || bad "rules put" "$(cat "$WORK/last.out")"
"$BIN" rules --json | grep -q '"edited"' && ok "rules show" || bad "rules show"
run rules put --data '{"rule":[]}'
run mode rm edited

# Modes ------------------------------------------------------------------
run mode save base --description "baseline" && ok "mode save" || bad "mode save" "$(cat "$WORK/last.out")"
# `mode save` records the REAL default audio sink; never let a nested test
# switch real audio on a later `mode apply`.
sed -i '/^audio_sink/d' "$XDG_CONFIG_HOME/stance/modes/base.toml"
grep -q audio_sink "$XDG_CONFIG_HOME/stance/modes/base.toml" && bad "audio_sink stripped from test mode" || ok "audio_sink stripped from test mode"
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

# Night light (hyprsunset, nested instance only) --------------------------
# Never touches brightness/audio: those would hit real devices.
if [ -z "$NWL" ] || ! command -v hyprsunset >/dev/null; then
  echo "  skip night light (no nested WAYLAND_DISPLAY or no hyprsunset)"
else
  nt() { WAYLAND_DISPLAY=$NWL "$BIN" "$@"; }
  # nq '<expr over d>' <night args...>: evaluates against data of --json output
  nq() { local e=$1; shift; nt "$@" --json | python3 -c "
import json,sys
d=json.load(sys.stdin)['data']
print(eval(sys.argv[1]))" "$e"; }
  nexpect() { local n=$1 e=$2; shift 2; [ "$(nq "$e" "$@" 2>&1)" = True ] && ok "$n" || bad "$n" "$(nt "$@" 2>&1)"; }

  kill_sunsets
  nexpect "night status without hyprsunset: running=false" "d['running'] is False and d['temperature'] is None" night status
  nexpect "night on 4000 starts hyprsunset" "d['running'] and d['on'] and d['temperature']==4000" night on 4000
  [ "$(nested_sunsets | wc -l)" = 1 ] && ok "exactly one nested hyprsunset" || bad "exactly one nested hyprsunset" "$(nested_sunsets)"
  nexpect "night toggle turns it off (6000)" "d['running'] and not d['on'] and d['temperature']==6000" night toggle
  nexpect "night toggle again restores last warmth" "d['on'] and d['temperature']==4000" night toggle
  nexpect "night set 5000" "d['temperature']==5000" night set 5000
  nt night set 999 >/dev/null 2>&1 && bad "night set 999 refused" || ok "night set 999 refused"
  nt night set 20001 >/dev/null 2>&1 && bad "night set 20001 refused" || ok "night set 20001 refused"
  nt night set 999 --json 2>/dev/null | grep -q '"ok":false' && ok "night set 999 --json error" || bad "night set 999 --json error"
  kill_sunsets
  nexpect "stale socket after kill: running=false" "d['running'] is False" night status
  # two simultaneous clicks must not spawn two daemons
  nt night on 4200 >/dev/null 2>&1 & A=$!
  nt night on 4200 >/dev/null 2>&1 & B=$!
  wait $A $B
  [ "$(nested_sunsets | wc -l)" = 1 ] && ok "concurrent night on: one hyprsunset" || bad "concurrent night on: one hyprsunset" "$(nested_sunsets | wc -l) running"
  nexpect "night off keeps daemon, 6000" "not d['on'] and d['temperature']==6000" night off
  kill_sunsets
  cat > "$XDG_CONFIG_HOME/stance/modes/evening.toml" <<'EOF'
description = "night light only"
night_light = 3500
EOF
  chmod 600 "$XDG_CONFIG_HOME/stance/modes/evening.toml"
  WAYLAND_DISPLAY=$NWL run mode apply evening --no-confirm && ok "mode with night_light applies" || bad "mode with night_light applies" "$(cat "$WORK/last.out")"
  nexpect "mode set night light to 3500" "d['running'] and d['temperature']==3500" night status
  kill_sunsets
fi

# Lid / power / waybar ---------------------------------------------------
hc output create headless eDP-7 >/dev/null; sleep 0.5
run set eDP-7 --pos 0x1080 --no-confirm
E7=$(q "(m['eDP-7']['x'], m['eDP-7']['y'])")
run lid close && ok "lid close runs" || bad "lid close runs" "$(cat "$WORK/last.out")"
expect "lid close turns the panel off" "not m['eDP-7']['enabled'] and m['TEST-1']['enabled']"
"$BIN" status --json | grep -q '"pending":null' && ok "lid close has no timer" || bad "lid close has no timer"
run lid open && ok "lid open runs" || bad "lid open runs" "$(cat "$WORK/last.out")"
expect "lid open restores place" "m['eDP-7']['enabled'] and (m['eDP-7']['x'], m['eDP-7']['y'])==$E7"
run lid sideways && bad "lid rejects junk" || ok "lid rejects junk"
"$BIN" waybar | python3 -c "import json,sys; d=json.load(sys.stdin); assert {'text','tooltip','class'}<=set(d)" && ok "waybar line is JSON" || bad "waybar line is JSON"
"$BIN" power --json | grep -q '"ok":true' && ok "power reports" || bad "power reports"
r=$(hc eval 'hl.bind("switch:on:Lid Switch", hl.dsp.exec_cmd("stance lid close"), { locked = true })')
[ "$r" = ok ] && ok "documented lid bind parses" || bad "documented lid bind parses" "$r"
hc output remove eDP-7 >/dev/null; sleep 0.3

# Phone deck (loopback only, nested instance) ------------------------------
# Never presses brightness/audio buttons: those would hit real devices.
if [ -z "$NWL" ]; then
  echo "  skip deck (no nested display)"
else
  # Own control socket dir: the user's real deck server may be running.
  export STANCE_DECK_DIR=$WORK/deck
  mkdir -m 700 -p "$STANCE_DECK_DIR"
  cat > "$XDG_CONFIG_HOME/stance/deck.toml" <<'EOF'
[[button]]
id = "night"
label = "Night"
action = "night"

[[button]]
id = "below"
label = "TEST-2 below"
action = "layout"
kind = "below"
monitor = "TEST-2"
reference = "TEST-1"
EOF
  chmod 600 "$XDG_CONFIG_HOME/stance/deck.toml"
  WAYLAND_DISPLAY=$NWL "$BIN" deck serve --loopback --port 0 2>"$WORK/deck.log" & DECK=$!
  sleep 0.8
  PAY=$("$BIN" deck pair --json 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['data']['payload'])" 2>/dev/null)
  [ -n "$PAY" ] && ok "deck pair gives a code" || bad "deck pair gives a code" "$(cat "$WORK/deck.log")"
  run deck test pair "$PAY" --name nested && ok "deck pairs" || bad "deck pairs" "$(cat "$WORK/last.out")"
  run deck test pair "$PAY" --name again && bad "deck code is single use" || ok "deck code is single use"
  "$BIN" deck test buttons --json | grep -q '"below"' && ok "deck lists buttons" || bad "deck lists buttons"
  kill_sunsets
  run deck test press night && ok "deck press night" || bad "deck press night" "$(cat "$WORK/last.out")"
  [ "$(WAYLAND_DISPLAY=$NWL "$BIN" night --json | python3 -c "import json,sys; print(json.load(sys.stdin)['data']['on'])")" = True ] \
    && ok "deck night light is on" || bad "deck night light is on"
  run deck test press below && ok "deck press layout" || bad "deck press layout" "$(cat "$WORK/last.out")"
  "$BIN" status --json | grep -q '"pending":{' && ok "deck change is pending" || bad "deck change is pending" "$("$BIN" status)"
  run deck test confirm && ok "deck confirm" || bad "deck confirm" "$(cat "$WORK/last.out")"
  "$BIN" status --json | grep -q '"pending":null' && ok "deck confirm cleared pending" || bad "deck confirm cleared pending"
  expect "deck layout applied (TEST-2 below TEST-1)" "m['TEST-2']['y']>m['TEST-1']['y']"
  run deck unpair nested
  sleep 1.2
  run deck test buttons && bad "unpaired phone refused" || ok "unpaired phone refused"
  kill $DECK; wait $DECK 2>/dev/null
  run deck put --data '{"button":[{"id":"w","label":"Work","action":"mode","name":"base"}]}' \
    && ok "deck put" || bad "deck put" "$(cat "$WORK/last.out")"
  "$BIN" deck buttons --json | grep -q '"custom":true' && ok "deck buttons from file" || bad "deck buttons from file"
  run deck put --data '{"button":[{"id":"x","label":"X","action":"exec","name":"rm"}]}' \
    && bad "deck put rejects unknown actions" || ok "deck put rejects unknown actions"
  run deck reset && "$BIN" deck buttons --json | grep -q '"custom":false' \
    && ok "deck reset to defaults" || bad "deck reset to defaults"
  unset STANCE_DECK_DIR
  kill_sunsets
  run layout right-of TEST-2 TEST-1 --no-confirm
fi

# Clipboard shelf (nested compositor's own clipboard) ---------------------
if command -v wl-copy >/dev/null && command -v wl-paste >/dev/null && [ -n "$NWL" ]; then
  export STANCE_CLIP_DIR=$WORK/clip
  # Everything that talks to a clipboard goes through W (never the live session).
  W() { WAYLAND_DISPLAY=$NWL "$@"; }
  W wl-paste --type text --watch "$BIN" clip ingest &
  CT=$!
  sleep 0.5
  W wl-copy "not stored while off"; sleep 0.5
  [ "$("$BIN" clip list --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["data"]["items"]))')" = 0 ] \
    && ok "clip stores nothing while off" || bad "clip stores nothing while off"
  run clip config put --data '{"enabled":true}'
  W wl-copy "hello from nested"; sleep 0.5
  W wl-copy "https://example.org/x"; sleep 0.5
  printf 'pw' | W wl-copy --type 'x-kde-passwordManagerHint' 2>/dev/null; sleep 0.3
  W wl-copy --type text/plain --type x-kde-passwordManagerHint "hunter2" 2>/dev/null; sleep 0.5
  "$BIN" clip list --json >"$WORK/clip.json"
  python3 - "$WORK/clip.json" <<'PY' && ok "clip captures text and link" || bad "clip captures text and link" "$(cat "$WORK/clip.json")"
import json, sys
items = json.load(open(sys.argv[1]))["data"]["items"]
kinds = {i["preview"]: i["kind"] for i in items}
assert kinds.get("hello from nested") == "text", kinds
assert kinds.get("https://example.org/x") == "link", kinds
assert not any("hunter2" in p or "not stored" in p for p in kinds), kinds
PY
  ID=$("$BIN" clip list --kind text --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["items"][0]["id"])')
  W wl-copy "something else"; sleep 0.5
  W "$BIN" clip copy "$ID" >"$WORK/last.out" 2>&1 && [ "$(W wl-paste --no-newline)" = "hello from nested" ] \
    && ok "clip copy puts the item back" || bad "clip copy puts the item back" "$(cat "$WORK/last.out")"
  sleep 0.5
  N=$("$BIN" clip list --json | python3 -c 'import json,sys; d=json.load(sys.stdin)["data"]["items"]; print(sum(i["preview"]=="hello from nested" for i in d), d[0]["preview"])')
  [ "$N" = "1 hello from nested" ] && ok "clip copy-back deduped and on top" || bad "clip copy-back deduped and on top" "$N"
  run clip rm "$ID" && run clip undo && grep -q "restored 1" "$WORK/last.out" && ok "clip rm + undo" || bad "clip rm + undo"
  run clip get ../../etc/passwd && bad "clip id path refused" || ok "clip id path refused"
  kill $CT 2>/dev/null; wait $CT 2>/dev/null
  unset STANCE_CLIP_DIR
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
case "$BIN" in */release/*) LIM=5120; KIND=release;; *) LIM=8192; KIND=debug;; esac
[ "${RSS:-99999}" -lt "$LIM" ] && ok "daemon RSS ${RSS} KB < $((LIM/1024)) MB ($KIND build)" || bad "daemon RSS ${RSS} KB (limit $LIM, $KIND)"
kill $DPID 2>/dev/null; wait $DPID 2>/dev/null

echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
