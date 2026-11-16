#!/bin/bash
# Builds iclear.app (with the privileged helper inside) and an installer .pkg
# that puts it in /Applications.
#
#   scripts/build-pkg.sh            → dist/iclear-<version>.pkg
#
# Signing: ad-hoc by default (no Apple Developer ID). To sign for distribution:
#   APP_SIGN_ID="Developer ID Application: …" PKG_SIGN_ID="Developer ID Installer: …" scripts/build-pkg.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source "$HOME/.cargo/env" 2>/dev/null || true

VERSION="$(python3 -c "import json;print(json.load(open('backend/app/tauri.conf.json'))['version'])")"
IDENT="com.iclear.app"
APP="target/release/bundle/macos/iclear.app"
WORK="target/pkg"
OUT="dist/iclear-${VERSION}.pkg"

echo "▸ Compilando helper (release)"
cargo build --release -p iclear-helper

echo "▸ Compilando app y frontend (cargo tauri build)"
(cd backend/app && cargo tauri build --bundles app)

echo "▸ Empaquetando el helper dentro de la app"
# helper_client::bundled_helper_path() looks next to the app binary.
cp target/release/iclear-helper "$APP/Contents/MacOS/iclear-helper"
chmod 755 "$APP/Contents/MacOS/iclear-helper"

echo "▸ Firmando la app (${APP_SIGN_ID:-ad-hoc})"
if [[ -n "${APP_SIGN_ID:-}" ]]; then
  codesign --force --options runtime --timestamp -s "$APP_SIGN_ID" "$APP/Contents/MacOS/iclear-helper"
  codesign --force --options runtime --timestamp -s "$APP_SIGN_ID" "$APP"
else
  codesign --force -s - "$APP/Contents/MacOS/iclear-helper"
  codesign --force -s - "$APP"
fi
codesign --verify --strict "$APP"

echo "▸ Armando el .pkg"
rm -rf "$WORK" && mkdir -p "$WORK/root/Applications" dist
# No extended attributes / AppleDouble "._*" files in the payload.
ditto --norsrc --noextattr --noqtn "$APP" "$WORK/root/Applications/iclear.app"
# Note: macOS tags files with com.apple.provenance (can't be removed), so the
# payload lists a few "._*" entries. Installer applies them as extended
# attributes; they don't end up as loose files on disk.
export COPYFILE_DISABLE=1
pkgbuild --analyze --root "$WORK/root" "$WORK/component.plist" >/dev/null
# Always install to /Applications, even if another copy of the app exists elsewhere.
plutil -replace 0.BundleIsRelocatable -bool false "$WORK/component.plist"
pkgbuild --root "$WORK/root" --component-plist "$WORK/component.plist" \
  --identifier "$IDENT" --version "$VERSION" --install-location / "$WORK/iclear-component.pkg"

productbuild --synthesize --package "$WORK/iclear-component.pkg" "$WORK/distribution.xml"
# Installer window title, host architecture and minimum macOS version.
python3 - "$WORK/distribution.xml" <<'PY'
import re, sys
p = sys.argv[1]
s = open(p).read()
# productbuild's synthesized file varies (minSpecVersion 1 or 2), so match loosely.
s, n = re.subn(r'(<installer-gui-script[^>]*>)',
  r'\1\n    <title>iclear</title>\n    <allowed-os-versions><os-version min="13.0"/></allowed-os-versions>', s, count=1)
assert n == 1, "installer-gui-script not found"
s = re.sub(r'hostArchitectures="[^"]*"', 'hostArchitectures="arm64"', s)
if 'hostArchitectures' not in s:
    s = s.replace('<title>iclear</title>', '<title>iclear</title>\n    <options hostArchitectures="arm64" customize="never" require-scripts="false"/>', 1)
open(p, 'w').write(s)
PY

if [[ -n "${PKG_SIGN_ID:-}" ]]; then
  productbuild --distribution "$WORK/distribution.xml" --package-path "$WORK" --sign "$PKG_SIGN_ID" "$OUT"
else
  productbuild --distribution "$WORK/distribution.xml" --package-path "$WORK" "$OUT"
fi

echo "✓ $OUT ($(du -h "$OUT" | cut -f1))"
