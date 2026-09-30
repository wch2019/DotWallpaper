#!/bin/bash
# macOS 26+ 发布流程：Xcode .app -> strip -> 签名 -> 校验 -> UDZO DMG
# 默认 ad-hoc 签名仅适合本机使用；对外分发请提供 Developer ID + 公证配置。
set -euo pipefail
cd "$(dirname "$0")/.."

APP="build/xcode-derived/Build/Products/Release/WallpaperEngine.app"
BIN="$APP/Contents/MacOS/WallpaperEngine"
VERSION="$(node -p "require('./package.json').version")"
DMG="build/WallpaperEngine_${VERSION}_arm64.dmg"

npm run build:mac:app
rm -f "$DMG"

# Swift/Rust release binaries are linked with debug symbols in this project;
# strip only the final executable, then sign the final bundle.
strip -x "$BIN"
if [ -n "${DW_SIGN_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp -s "$DW_SIGN_IDENTITY" "$BIN"
  codesign --force --options runtime --timestamp -s "$DW_SIGN_IDENTITY" "$APP"
  echo "signed with Developer ID identity"
else
  codesign --force -s - "$BIN"
  codesign --force -s - "$APP"
  echo "signed ad-hoc for local use"
fi
codesign --verify --deep --strict --verbose=1 "$APP"

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
mkdir -p "$(dirname "$DMG")"
diskutil image create from \
  --volumeName WallpaperEngine \
  --format UDZO \
  "$STAGE" \
  "$DMG" >/dev/null

if [ -n "${DW_SIGN_IDENTITY:-}" ] && [ -n "${DW_NOTARY_PROFILE:-}" ]; then
  xcrun notarytool submit "$DMG" --keychain-profile "$DW_NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
  xcrun stapler staple "$APP"
  spctl --assess --type execute -vv "$APP"
elif [ -n "${DW_SIGN_IDENTITY:-}" ]; then
  echo "warning: Developer ID signed but not notarized (DW_NOTARY_PROFILE is unset)"
fi

echo
echo "== size report =="
ls -l "$BIN" | awk '{printf "executable (stripped): %s B\n", $5}'
du -sh ui/dist | awk '{printf "frontend dist:         %s\n", $1}'
du -sh "$APP" | awk '{printf ".app total:            %s\n", $1}'
ls -l "$DMG" | awk '{printf "dmg (UDZO):            %s B\n", $5}'
echo "app: $APP"
echo "dmg: $DMG"
