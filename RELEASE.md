# Releasing stance

Maintainer checklist. Everything up to step 4 is prepared in the repo; the
tag and the GitHub release are made by the maintainer.

## 1. Checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build --release
tests/nested.sh lua target/release/stance        # needs a running Wayland session
# phone app: in stance-deck-android, ./gradlew :core:test -Dstance.binary=<this>/target/release/stance
# (and scripts/e2e.sh there for the emulator run)
```

All green, and `stance doctor` on a real machine shows no FAIL.

## 2. Version and changelog

- `Cargo.toml` `version`, `packaging/aur/stance-hypr/PKGBUILD` `pkgver`.
  The phone app is released from stance-deck-android (its own `versionName` /
  `versionCode`); its release notes say which stance versions it speaks to
- `CHANGELOG.md`: replace "unreleased" with the date

Commit: `git commit -am "stance X.Y.Z"` and push.

## 3. Tag

```sh
git tag -a vX.Y.Z -m "stance X.Y.Z"
git push origin vX.Y.Z
```

## 4. Artifacts

```sh
# CLI tarball
mkdir -p dist/stance-X.Y.Z
cp target/release/stance LICENSE README.md CHANGELOG.md dist/stance-X.Y.Z/
cp -r quickshell examples contrib docs dist/stance-X.Y.Z/
tar -C dist -czf dist/stance-X.Y.Z-x86_64-linux.tar.gz stance-X.Y.Z

sha256sum dist/*.tar.gz > dist/SHA256SUMS
# The phone APK is built and signed in stance-deck-android
```

## 5. GitHub release

```sh
awk '/^## X.Y.Z/{f=1;next} /^## /{f=0} f' CHANGELOG.md > dist/notes.md
gh release create vX.Y.Z --title "stance X.Y.Z" --notes-file dist/notes.md \
  dist/stance-X.Y.Z-x86_64-linux.tar.gz dist/stance-deck-X.Y.Z.apk dist/SHA256SUMS
```

## 6. AUR

`stance-hypr-git`'s `pkgver()` needs at least one tag in the repo (it has
`v0.1.0`). After the first upload, drop "AUR upload pending" from both
READMEs.

```sh
cd packaging/aur/stance-hypr
updpkgsums && makepkg --printsrcinfo > .SRCINFO && makepkg -f   # build check
# then push PKGBUILD + .SRCINFO to ssh://aur@aur.archlinux.org/stance-hypr.git
cd ../stance-hypr-git && makepkg --printsrcinfo > .SRCINFO
```

Commit the updated PKGBUILD / .SRCINFO back to this repo.
- At the 0.3.0 tag: add the two `stance-clip-*.service` install lines to `packaging/aur/stance-hypr/PKGBUILD` (already in `-git`).
- At the 0.3.0 tag: also `stance-panel.service`; optdepends add `wl-clipboard: clipboard shelf`.
