# Translating the panel

Panel text lives in `quickshell/stance/i18n/<code>.json`: a plain JSON object
whose keys are the English source strings and whose values are the translation.

## Add a language

1. Copy `quickshell/stance/i18n/tr.json` to `i18n/<code>.json`, using a code
   such as `de`, `fr` or `pt_BR` (`^[a-z]{2,3}(_[A-Z]{2})?$`).
2. Set `"_name"` to the language's own name (`"Deutsch"`); it is shown in the
   Settings language picker. Translate every value; keep the keys unchanged.
3. Keep the `%1`, `%2` placeholders (order may change, the set must match).
   Strings you leave equal to the English text just show English.
4. List the code in `codes` at the top of `quickshell/stance/StanceI18n.qml`.
5. Run `python3 scripts/i18n-check.py`; it reports missing, unused and
   placeholder-mismatched entries per language.
6. Try it: pick the language in Settings > General, or start the panel with
   `LANG=de_DE.UTF-8` while the language is on Automatic.

If a file is missing or broken the panel falls back to English.
