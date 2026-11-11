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

echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
