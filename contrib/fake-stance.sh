#!/usr/bin/env bash
# Wrapper for developing the panel without touching real systemd units:
#   STANCE_BIN=$PWD/contrib/fake-stance.sh XDG_CONFIG_HOME=/tmp/x/cfg qs -p quickshell/stance
# Everything goes to the real binary (target/release/stance, or STANCE_REAL)
# except `feature on|off NAME`, which only edits $XDG_CONFIG_HOME/stance/features.json
# (and clip.enabled for `clip`) and never calls systemctl.
here=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
real=${STANCE_REAL:-$here/target/release/stance}
if [ "${1:-}" = feature ] && { [ "${2:-}" = on ] || [ "${2:-}" = off ]; }; then
  name=${3:-}
  case "$name" in clip|deck|rules|quick) ;; *) echo '{"ok":false,"schema":1,"error":{"code":"usage","message":"unknown feature"}}'; exit 2;; esac
  # Never the real config: refuse without an explicit XDG_CONFIG_HOME.
  [ -n "${XDG_CONFIG_HOME:-}" ] || { echo '{"ok":false,"schema":1,"error":{"code":"usage","message":"fake-stance: set XDG_CONFIG_HOME"}}'; exit 2; }
  cfg=$XDG_CONFIG_HOME/stance
  mkdir -p "$cfg"
  f=$cfg/features.json
  [ -s "$f" ] || echo '{}' > "$f"
  v=true; [ "$2" = off ] && v=false
  tmp=$(jq --arg n "$name" --argjson v "$v" '.[$n]=$v' "$f") && printf '%s\n' "$tmp" > "$f"
  [ "$name" = clip ] && "$real" clip config put --data "{\"enabled\":$v}" >/dev/null 2>&1
  [ -n "${STANCE_FAKE_LOG:-}" ] && echo "feature $2 $name" >> "$STANCE_FAKE_LOG"
  echo '{"ok":true,"schema":1,"data":{"warnings":[]}}'
  exit 0
fi
exec "$real" "$@"
