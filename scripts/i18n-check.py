#!/usr/bin/env python3
"""Every literal string passed to StanceI18n.t()/f() in the QML has a Turkish
entry with the same %N placeholders (exit 1 otherwise). Entries that look
unused are only listed: some keys are looked up through variables."""
import json, pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent / "quickshell"
i18n = (root / "stance" / "StanceI18n.qml").read_text()
start = i18n.index("readonly property var tr: ({")
body = i18n[start + len("readonly property var tr: (") :]
body = body[: body.rindex("})") + 1]
# The dictionary is a JS object literal of "string": "string" pairs.
pair = re.compile(r'"((?:[^"\\]|\\.)*)"\s*:\s*"((?:[^"\\]|\\.)*)"')
tr = {json.loads(f'"{k}"'): json.loads(f'"{v}"') for k, v in pair.findall(body)}

call = re.compile(r'StanceI18n\.[tf]\(\s*"((?:[^"\\]|\\.)*)"')
used = {}
for f in sorted(root.rglob("*.qml")):
    if f.name == "StanceI18n.qml":
        continue
    for n, line in enumerate(f.read_text().splitlines(), 1):
        for m in call.finditer(line):
            used.setdefault(json.loads(f'"{m.group(1)}"'), f"{f.relative_to(root)}:{n}")

missing = sorted(set(used) - set(tr))
unused = sorted(set(tr) - set(used))
bad_args = [k for k in used if k in tr and set(re.findall(r"%\d", k)) != set(re.findall(r"%\d", tr[k]))]
for k in missing:
    print(f"missing tr: {k!r}  ({used[k]})")
for k in bad_args:
    print(f"placeholder mismatch: {k!r} -> {tr[k]!r}")
for k in unused:
    print(f"unused tr: {k!r}")
print(f"{len(used)} strings, {len(tr)} translations, {len(missing)} missing, {len(unused)} unused, {len(bad_args)} bad placeholders")
sys.exit(1 if missing or bad_args else 0)
