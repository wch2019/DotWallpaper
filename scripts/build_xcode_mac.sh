#!/bin/bash
# macOS 26+ 标准 Xcode 构建：Vue dist + Rust staticlib + Swift/AppKit .app
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${PROJECT:-xcode/WallpaperEngine.xcodeproj}"
SCHEME="${SCHEME:-WallpaperEngine}"
CONFIGURATION="${CONFIGURATION:-Release}"
DERIVED_DATA="${DERIVED_DATA:-build/xcode-derived}"

if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "error: xcodebuild not found; install Xcode 27 or newer" >&2
  exit 1
fi
if ! command -v npm >/dev/null 2>&1; then
  echo "error: npm not found; install Node.js/npm before building" >&2
  exit 1
fi

# Keep the minimum OS explicit at the command line as well as in the project,
# so CI or a local override cannot accidentally produce a macOS 12-compatible app.
xcodebuild \
  -project "$PROJECT" \
  -scheme "$SCHEME" \
  -configuration "$CONFIGURATION" \
  -sdk macosx \
  -derivedDataPath "$DERIVED_DATA" \
  MACOSX_DEPLOYMENT_TARGET=26.0 \
  CODE_SIGNING_ALLOWED=NO \
  build

APP="$DERIVED_DATA/Build/Products/$CONFIGURATION/WallpaperEngine.app"
if [ ! -d "$APP" ]; then
  echo "error: Xcode completed without producing $APP" >&2
  exit 1
fi

INFO_PLIST="$APP/Contents/Info.plist"
MIN_VERSION=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$INFO_PLIST" 2>/dev/null || true)
if [ "$MIN_VERSION" != "26.0" ]; then
  echo "error: expected LSMinimumSystemVersion=26.0, got '${MIN_VERSION:-missing}'" >&2
  exit 1
fi

ARCHS=$(lipo -archs "$APP/Contents/MacOS/WallpaperEngine")
case " $ARCHS " in
  *" arm64 "*) ;;
  *) echo "error: expected arm64 executable, got '$ARCHS'" >&2; exit 1 ;;
esac

# Xcode builds with CODE_SIGNING_ALLOWED=NO, leaving the linker-signed executable
# but not a valid sealed app bundle. Sign the entire bundle so the documented
# build:mac:app -> open path is directly usable. The release script strips and
# re-signs the final bundle (optionally with Developer ID) after this step.
codesign --force -s - "$APP"
codesign --verify --deep --strict --verbose=1 "$APP"

echo
echo "Xcode build succeeded"
echo "app:  $APP"
echo "arch: $ARCHS"
echo "min:  $MIN_VERSION"
echo "sign: ad-hoc (local build only)"
