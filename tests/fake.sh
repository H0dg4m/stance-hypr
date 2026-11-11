#!/usr/bin/env bash
# Integration tests against the built-in fake backend: no compositor, no
# display session needed (runs in CI).   tests/fake.sh [path/to/stance]
set -u
BIN=$(realpath "${1:-target/debug/stance}")
WORK=$(mktemp -d "${TMPDIR:-/tmp}/stance-fake.XXXXXX")
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT
export XDG_CONFIG_HOME=$WORK/cfg XDG_STATE_HOME=$WORK/state XDG_DATA_HOME=$WORK/data
export STANCE_STATE_DIR=$WORK/state/stance STANCE_BACKEND=fake
unset STANCE_FAKE_STATE
mkdir -p -m 700 "$XDG_CONFIG_HOME/stance/modes" "$STANCE_STATE_DIR"

PASS=0; FAIL=0
ok() { PASS=$((PASS+1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $1"; [ -n "${2:-}" ] && echo "       $2"; }
s() { "$BIN" "$@"; }
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
check() { # name, expected, actual
  [ "$3" = "$2" ] && ok "$1" || bad "$1" "expected '$2', got '$3'"
}
x_of() { s monitors --json | j "[m['x'] for m in d['data'] if m['name']=='$1'][0]"; }

check "two default screens" 2 "$(s monitors --json | j "len(d['data'])")"
check "backend flag beats env" 2 "$(STANCE_BACKEND=nonsense s --backend fake monitors --json | j "len(d['data'])")"
STANCE_BACKEND=nonsense s monitors >/dev/null 2>&1; check "unknown backend refused" 2 $?

# set -> pending -> confirm
s set B-1 --pos 2000x0 --timeout 30 >/dev/null
check "set moves the screen" 2000 "$(x_of B-1)"
check "pending after set" True "$(s status --json | j "d['data']['pending'] is not None")"
s confirm >/dev/null
check "confirm clears pending" True "$(s status --json | j "d['data']['pending'] is None")"
check "confirm keeps the change" 2000 "$(x_of B-1)"

# set -> revert
s set B-1 --pos 3000x0 --timeout 30 >/dev/null
s revert >/dev/null
check "revert restores" 2000 "$(x_of B-1)"
check "revert clears pending" True "$(s status --json | j "d['data']['pending'] is None")"

# set -> timeout (guardian)
s set B-1 --pos 4000x0 --timeout 1 >/dev/null
sleep 3
check "timeout reverts" 2000 "$(x_of B-1)"

# mode save / apply
s set B-1 --pos 1920x0 --no-confirm >/dev/null
s mode save desk >/dev/null 2>&1 && ok "mode save" || bad "mode save"
s set B-1 --pos 5000x0 --no-confirm >/dev/null
s mode apply desk --no-confirm >/dev/null 2>&1
check "mode apply restores layout" 1920 "$(x_of B-1)"
check "active mode recorded" desk "$(s status --json | j "d['data']['active_mode']")"

# daemon: applies the matching rule at start and when a screen appears
s set B-1 --pos 5000x0 --no-confirm >/dev/null
printf '[[rule]]\nmonitors = ["A-1", "B-1"]\nmode = "desk"\n' > "$XDG_CONFIG_HOME/stance/rules.toml"
chmod 600 "$XDG_CONFIG_HOME/stance/rules.toml"
"$BIN" daemon >"$WORK/daemon.log" 2>&1 &
DPID=$!
trap 'kill $DPID 2>/dev/null; rm -rf "$WORK"' EXIT
for _ in $(seq 1 40); do [ "$(x_of B-1)" = 1920 ] && break; sleep 0.25; done
check "daemon applies the rule at start" 1920 "$(x_of B-1)"
s set B-1 --pos 5000x0 --no-confirm >/dev/null
python3 - "$STANCE_STATE_DIR/fake.json" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p)); d["monitors"].append(dict(d["monitors"][0], name="C-1", x=9000)); json.dump(d,open(p,"w"))
PY
sleep 0.5
python3 - "$STANCE_STATE_DIR/fake.json" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p)); d["monitors"]=[m for m in d["monitors"] if m["name"]!="C-1"]; json.dump(d,open(p,"w"))
PY
# C-1 came and went: the rule only matches the exact set A-1,B-1 again
for _ in $(seq 1 40); do [ "$(x_of B-1)" = 1920 ] && break; sleep 0.25; done
check "daemon reacts to monitor events" 1920 "$(x_of B-1)"
kill $DPID 2>/dev/null; wait $DPID 2>/dev/null
rm -f "$XDG_CONFIG_HOME/stance/rules.toml"

# events: one JSON line per change; ends when the reader goes away
rm -rf "$XDG_CONFIG_HOME/stance/themes"
"$BIN" events > "$WORK/events.out" 2>/dev/null &
EPID=$!
sleep 0.5
s set B-1 --pos 2500x0 --timeout 30 >/dev/null     # pending.json (+ the fake's file)
s confirm >/dev/null
python3 - "$STANCE_STATE_DIR/fake.json" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p)); d["monitors"].append(dict(d["monitors"][0], name="D-1", x=9000)); json.dump(d,open(p,"w"))
PY
printf '{"show":{"map":true}}' > "$XDG_CONFIG_HOME/stance/panel.json"; chmod 600 "$XDG_CONFIG_HOME/stance/panel.json"
s feature off quick >/dev/null
mkdir -p -m 700 "$XDG_CONFIG_HOME/stance/themes"; sleep 0.3
s theme export stance-dark --out "$WORK/t.json" >/dev/null && sed -i 's/"id": *"stance-dark"/"id": "evtest"/' "$WORK/t.json"
s theme import "$WORK/t.json" >/dev/null
sleep 0.7
kill $EPID 2>/dev/null; wait $EPID 2>/dev/null
ev() { grep -c "$1" "$WORK/events.out"; }
check "events: hello first" hello "$(head -1 "$WORK/events.out" | j "d['event']")"
check "events: backend and caps" "fake True" "$(head -1 "$WORK/events.out" | j "d['backend'], d['monitor_events']")"
[ "$(ev '"event":"pending"')" -ge 1 ] && ok "events: pending" || bad "events: pending" "$(cat "$WORK/events.out")"
[ "$(ev '"change":"added","event":"monitors","name":"D-1"')" -ge 1 ] && ok "events: monitor added" || bad "events: monitor added" "$(cat "$WORK/events.out")"
[ "$(ev '"event":"panel"')" -ge 1 ] && ok "events: panel.json" || bad "events: panel.json"
[ "$(ev '"event":"theme"')" -ge 1 ] && ok "events: themes/ (created after start)" || bad "events: themes/" "$(cat "$WORK/events.out")"
[ "$(ev '"event":"features"')" -ge 1 ] && ok "events: features.json" || bad "events: features.json"
python3 - "$STANCE_STATE_DIR/fake.json" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p)); d["monitors"]=[m for m in d["monitors"] if m["name"]!="D-1"]; json.dump(d,open(p,"w"))
PY
t0=$(date +%s)
timeout 10 bash -c '"$0" events | head -1 >/dev/null' "$BIN"; rc=$?
[ "$rc" = 0 ] && [ $(( $(date +%s) - t0 )) -lt 5 ] && ok "events: ends when the reader closes (idle)" || bad "events: ends when the reader closes (idle)" "rc=$rc"

# capabilities the fake has not got
out=$(s night status --json 2>&1); rc=$?
check "night unsupported exit code" 2 "$rc"
check "night unsupported code" unsupported "$(echo "$out" | j "d['error']['code']")"

check "caps json" fake "$(s caps --json | j "d['data']['backend']")"
check "persist unsupported on fake" unsupported "$(s persist --json 2>&1 | j "d['error']['code']")"

check "quick keeps going without night light" True "$(s quick --json | j "d['data']['night'] is None and 'night' in d['data']['errors']")"

# a mode with compositor options on a backend without them
printf '[[monitor]]\nmatch = "A-1"\nscale = 1.0\n\n[hyprland]\nblur = false\n' > "$XDG_CONFIG_HOME/stance/modes/opts.toml"
chmod 600 "$XDG_CONFIG_HOME/stance/modes/opts.toml"
s mode apply opts --no-confirm >/dev/null 2>&1; check "options apply by default" 0 $?
python3 - "$STANCE_STATE_DIR/fake.json" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p)); d["caps"]={"compositor_options":False}; json.dump(d,open(p,"w"))
PY
check "caps seeded" False "$(s caps --json | j "d['data']['caps']['compositor_options']")"
s mode apply opts --no-confirm --json >/dev/null 2>&1; check "mode with options unsupported (exit)" 2 $?
check "mode with options unsupported (code)" unsupported "$(s mode apply opts --no-confirm --json 2>&1 | j "d['error']['code']")"

# files from a newer stance are never overwritten
printf 'schema = 2\n' > "$XDG_CONFIG_HOME/stance/rules.toml"
chmod 600 "$XDG_CONFIG_HOME/stance/rules.toml"
check "rules put refuses newer file" newer_schema "$(s rules put --data '{"rule":[]}' --json 2>&1 | j "d['error']['code']")"
check "newer rules file untouched" "schema = 2" "$(cat "$XDG_CONFIG_HOME/stance/rules.toml")"
case "$(s theme system)" in dark|light|unknown) ok "theme system answers";; *) bad "theme system answers";; esac

# phone deck, protocol v2 (loopback, own control socket dir)
export STANCE_DECK_DIR=$WORK/deck XDG_RUNTIME_DIR=$WORK/run
mkdir -p -m 700 "$STANCE_DECK_DIR" "$XDG_RUNTIME_DIR"
"$BIN" deck serve --loopback --port 0 2>"$WORK/deck.log" & DECK=$!
trap 'kill $DECK 2>/dev/null; rm -rf "$WORK"' EXIT
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -S "$STANCE_DECK_DIR/control.sock" ] || [ -n "$(ls "$STANCE_DECK_DIR" 2>/dev/null)" ] && break; sleep 0.2; done
sleep 0.3
PAY=$(s deck pair --json 2>/dev/null | j "d['data']['payload']" 2>/dev/null)
s deck test pair "$PAY" --name ci >/dev/null 2>&1; check "deck pairs" 0 $?
check "deck v2 client negotiates and pings" pong "$(s deck test ping 2>&1)"
case "$(s deck test offer clip.text 2>&1)" in
  *"offer declined: unsupported"*) ok "deck declines offers (no caps in 0.6)";;
  *) bad "deck declines offers (no caps in 0.6)" "$(s deck test offer clip.text 2>&1)";;
esac
case "$(s deck test stray-data 2>&1)" in
  *"session ended"*) ok "deck ends the session on a stray data frame";;
  *) bad "deck ends the session on a stray data frame";;
esac
check "deck serves the next session" pong "$(s deck test ping 2>&1)"
grep -q "data for unknown stream" "$WORK/deck.log" && ok "deck logs the protocol error" || bad "deck logs the protocol error" "$(cat "$WORK/deck.log")"
kill $DECK; wait $DECK 2>/dev/null

echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
