#!/usr/bin/env bash
# Offscreen QML smoke test: no compositor, fake backend, throwaway config.
#   tests/qml-smoke.sh [path/to/stance] [out-dir]
# Builds the panel and every settings page, writes PNGs to out-dir (default:
# a temp dir that is removed) and fails on any QML error line.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
BIN=$(realpath "${1:-$here/target/debug/stance}")
WORK=$(mktemp -d "${TMPDIR:-/tmp}/stance-qml.XXXXXX")
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT
OUT=${2:-$WORK/out}
mkdir -p "$OUT"
export XDG_CONFIG_HOME=$WORK/cfg XDG_STATE_HOME=$WORK/state XDG_DATA_HOME=$WORK/data XDG_RUNTIME_DIR=$WORK/run
export STANCE_STATE_DIR=$WORK/state/stance STANCE_BACKEND=fake STANCE_BIN=$BIN STANCE_SMOKE_OUT=$OUT
export QT_QPA_PLATFORM=offscreen
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE STANCE_FAKE_STATE
mkdir -p -m 700 "$XDG_CONFIG_HOME/stance" "$STANCE_STATE_DIR" "$XDG_RUNTIME_DIR"

cp -r "$here/quickshell/stance" "$WORK/shell"
cp "$here/tests/qml/smoke.qml" "$WORK/shell/shell.qml"
cp "$here"/tests/qml/stubs/*.qml "$WORK/shell/"

log=$WORK/log
timeout 120 qs -p "$WORK/shell" >"$log" 2>&1
rc=$?
# QML problems Quickshell prints as warnings.
if grep -E "TypeError|ReferenceError|is not defined|Unable to assign|Cannot read property|Cannot assign|is not a type|failed to load|Type .* unavailable|Binding loop" "$log"; then
  echo "FAIL qml errors (see above)"; exit 1
fi
grep -q "smoke: done" "$log" || { tail -30 "$log"; echo "FAIL smoke did not finish (rc=$rc)"; exit 1; }
n=$(grep -c "smoke: built" "$log")
echo "ok   qml smoke: $n views built${2:+, PNGs in $OUT}"
prefs_of() { grep -o 'smoke: prefs .*' "$1" | head -1 | cut -d' ' -f3-; }
want() { # name, expected substring, actual
  case "$3" in *"$2"*) echo "ok   $1";; *) echo "FAIL $1: expected $2 in $3"; exit 1;; esac
}
want "no panel.json = first-run setup" '"state":"missing","onboarded":false' "$(prefs_of "$log")"
want "first run gets the standard board" 'map.L,screen.L,night.M,brightness.M,audio.M,modes.L' "$(prefs_of "$log")"

# panel.json migration and fail-closed reads: one view each.
mig() { # name, panel.json content ("-" = unreadable file)
  rm -rf "$XDG_CONFIG_HOME/stance" && mkdir -p -m 700 "$XDG_CONFIG_HOME/stance"
  if [ "$2" = - ]; then echo '{}' > "$XDG_CONFIG_HOME/stance/panel.json"; chmod 000 "$XDG_CONFIG_HOME/stance/panel.json"
  else printf '%s\n' "$2" > "$XDG_CONFIG_HOME/stance/panel.json"; fi
  STANCE_SMOKE_STEPS=panel STANCE_SMOKE_OUT=$WORK/mig timeout 60 qs -p "$WORK/shell" >"$WORK/mig.log" 2>&1
  prefs_of "$WORK/mig.log"
}
mkdir -p "$WORK/mig"
r=$(mig "schema 1" '{"show":{"night":false,"modes":false},"confirmTimeout":20,"schema":1}')
want "schema 1: show.* off drops those widgets" '"board":"map.L,screen.L,brightness.M,audio.M"' "$r"
want "schema 1: existing user skips setup" '"onboarded":true' "$r"
r=$(mig "schema 2" '{"board":[{"id":"clip","size":"S"},{"id":"nope","size":"S"},{"id":"night","size":"L"},{"id":"clip","size":"S"}],"known":["map","screen","night","brightness","audio","modes","clip"],"onboarded":true,"schema":2}')
want "schema 2: unknown ids, bad sizes, duplicates cleaned; new widget (deck) joins once" '"board":"clip.S,night.M,deck.S"' "$r"
if [ "$(id -u)" != 0 ]; then
  r=$(mig "unreadable" -)
  want "unreadable panel.json is not a first run" '"state":"present","onboarded":true' "$r"
  chmod 600 "$XDG_CONFIG_HOME/stance/panel.json"
  want "unreadable panel.json is left alone" '{}' "$(cat "$XDG_CONFIG_HOME/stance/panel.json")"
fi
