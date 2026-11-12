#!/usr/bin/env bash
# A 0.2.0 config/state layout read by this build: nothing is lost, new parts
# default to what 0.2.0 users had (features on, clipboard shelf off, built-in
# themes only). Needs no Hyprland.   tests/upgrade.sh [path/to/stance]
set -u
BIN=$(realpath "${1:-target/debug/stance}")
WORK=$(mktemp -d "${TMPDIR:-/tmp}/stance-upgrade.XXXXXX")
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/home"; export HOME=$WORK/home
export XDG_CONFIG_HOME=$WORK/cfg XDG_STATE_HOME=$WORK/state XDG_DATA_HOME=$WORK/data
unset STANCE_STATE_DIR STANCE_CLIP_DIR STANCE_CLIP_CONFIG
C=$XDG_CONFIG_HOME/stance
mkdir -p -m 700 "$C/modes" "$XDG_STATE_HOME/stance"
# What 0.2.0 wrote.
cat > "$C/panel.json" <<'J'
{"show":{"map":true,"screen":true,"night":true,"brightness":true,"audio":true,"modes":true},"confirmTimeout":5,"compact":false}
J
printf 'description = "desk"\n\n[[monitor]]\nmatch = "DP-1"\nscale = 1.0\n' > "$C/modes/desk.toml"
printf '[[rule]]\nmonitors = ["DP-1", "eDP-1"]\nmode = "desk"\n' > "$C/rules.toml"
chmod 600 "$C"/panel.json "$C"/modes/desk.toml "$C"/rules.toml
before=$(cd "$C" && find . -type f -exec sha256sum {} + | sort)

PASS=0; FAIL=0
ok() { PASS=$((PASS+1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $1"; [ -n "${2:-}" ] && echo "       $2"; }
j() { python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }

[ "$("$BIN" feature list --json | j "all(f['on'] for f in d['features'])")" = True ] \
  && ok "features default on" || bad "features default on"
[ "$("$BIN" clip config show --json | j "d['enabled']")" = False ] \
  && ok "clipboard shelf stays off" || bad "clipboard shelf stays off"
[ "$("$BIN" clip list --json | j "len(d['items'])")" = 0 ] && ok "clip list on a fresh store" || bad "clip list on a fresh store"
[ "$("$BIN" theme list --json | j "len(d['themes'])")" = 17 ] && ok "built-in themes only" || bad "built-in themes only"
"$BIN" rules --json >/dev/null 2>&1 && ok "0.2.0 rules still read" || bad "0.2.0 rules still read" "$("$BIN" rules 2>&1)"
after=$(cd "$C" && find . -type f -exec sha256sum {} + | sort)
[ "$before" = "$after" ] && ok "0.2.0 files untouched by reads" || bad "0.2.0 files untouched by reads"
"$BIN" feature off quick >/dev/null 2>&1 && [ "$("$BIN" feature list --json | j "[f['on'] for f in d['features'] if f['name']=='quick'][0]")" = False ] \
  && ok "feature switch writes features.json" || bad "feature switch writes features.json"
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
