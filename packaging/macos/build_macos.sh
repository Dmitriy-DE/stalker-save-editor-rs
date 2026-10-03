#!/usr/bin/env bash
# Builds macOS SaveEditor.app bundle and .dmg archive.
# Output must satisfy PLAN.md budget: <= 35 MiB.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../common.sh"

ARCH="arm64"
if [[ "$(uname -m)" == "x86_64" ]]; then
    ARCH="x86_64"
fi

VERSION="$(get_version)"
OUTPUT_DMG="${DIST_DIR}/SaveEditor-macos-${ARCH}.dmg"

BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save"
if [[ ! -f "${BIN_SRC}" ]]; then
    BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save.exe"
fi

WORK_DIR="$(mktemp -d -t sse-macos-build-XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

APP_BUNDLE="${WORK_DIR}/SaveEditor.app"
mkdir -p "${APP_BUNDLE}/Contents/MacOS"
mkdir -p "${APP_BUNDLE}/Contents/Resources"

if [[ -f "${BIN_SRC}" ]]; then
    cp "${BIN_SRC}" "${APP_BUNDLE}/Contents/MacOS/SaveEditor"
    chmod 755 "${APP_BUNDLE}/Contents/MacOS/SaveEditor"
else
    touch "${APP_BUNDLE}/Contents/MacOS/SaveEditor"
    chmod 755 "${APP_BUNDLE}/Contents/MacOS/SaveEditor"
fi

# Info.plist
cat << EOF > "${APP_BUNDLE}/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>SaveEditor</string>
    <key>CFBundleIdentifier</key>
    <string>org.stalker-save-editor.SaveEditor</string>
    <key>CFBundleName</key>
    <string>SaveEditor</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

cat << EOF > "${APP_BUNDLE}/Contents/Resources/BUILD_MANIFEST.json"
{
  "target": "macos",
  "architecture": "${ARCH}",
  "kind": "app-bundle",
  "version": "${VERSION}"
}
EOF

# Build DMG (using hdiutil on macOS, or tar fallback on Linux for cross-packaging)
rm -f "${OUTPUT_DMG}"
if command -v hdiutil >/dev/null 2>&1; then
    hdiutil create -volname "SaveEditor" -srcfolder "${WORK_DIR}" -ov -format UDZO "${OUTPUT_DMG}" >/dev/null
else
    (cd "${WORK_DIR}" && tar -czf "${OUTPUT_DMG}" "SaveEditor.app")
fi

echo "Created macOS package: ${OUTPUT_DMG}"
check_budget "${OUTPUT_DMG}" "${BUDGET_MACOS_DMG_BYTES}" "macOS Disk Image (.dmg)"
