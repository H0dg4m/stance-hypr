#!/usr/bin/env bash
# Stand-in for `stance` while developing the clipboard shelf panel:
#   STANCE_BIN=$PWD/contrib/fake-clip.sh STANCE_CLIP_DIR=/tmp/stance-clip-fixture \
#     qs -p quickshell/stance
# Prints the golden list for `clip list`, {"rev":43} for mutations and a
# config for `clip config`. Anything that is not `clip ...` fails.
here=$(cd "$(dirname "$0")/.." && pwd)
[ "${1:-}" = clip ] || { echo '{"ok":false,"schema":1,"error":{"code":"fake","message":"fake-clip.sh only knows clip"}}'; exit 2; }
shift
[ -n "${STANCE_FAKE_LOG:-}" ] && echo "clip $*" >> "$STANCE_FAKE_LOG"
case "${1:-}" in
  list)
    if [ -n "${STANCE_FAKE_OFF:-}" ]; then echo '{"ok":true,"schema":1,"data":{"rev":1,"enabled":false,"items":[]}}'
    else cat "$here/tests/fixtures/clip-list.json"; fi ;;
  get)  printf 'cargo test --release && tests/nested.sh lua target/release/stance\n\nLonger text for the preview.\n<b>plain</b>\n' ;;
  config)
    if [ "${2:-}" = put ]; then echo '{"ok":true,"schema":1,"data":{"enabled":true,"text":true,"image":true,"retention_days":30}}'
    else echo '{"ok":true,"schema":1,"data":{"enabled":true,"text":true,"image":true,"retention_days":30}}'; fi ;;
  copy|pin|unpin|rm|undo|prune|clear) echo '{"ok":true,"schema":1,"data":{"rev":43}}' ;;
  *) echo '{"ok":false,"schema":1,"error":{"code":"usage","message":"unknown clip command"}}'; exit 2 ;;
esac
