#!/usr/bin/env bash
# Builds a self-contained Linux AppImage without external network downloads.
# Output must satisfy PLAN.md budget: <= 35 MiB.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../common.sh"

ARCH="$(uname -m)"
VERSION="$(get_version)"
OUTPUT_APPIMAGE="${DIST_DIR}/S.T.A.L.K.E.R.-Save-Editor-${VERSION}-${ARCH}.AppImage"

BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save"
if [[ ! -f "${BIN_SRC}" ]]; then
    echo "Building release binary..."
    "${CARGO_CMD}" build --release -p sse-cli
fi

WORK_DIR="$(mktemp -d -t sse-appimage-build-XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

APPDIR="${WORK_DIR}/AppDir"
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"

# 1. Binary
cp "${BIN_SRC}" "${APPDIR}/usr/bin/stalker-save"
chmod 755 "${APPDIR}/usr/bin/stalker-save"

# 2. Desktop file
cat << 'EOF' > "${APPDIR}/stalker-save-editor.desktop"
[Desktop Entry]
Name=S.T.A.L.K.E.R. Save Editor
Comment=Save game editor and fixes manager for S.T.A.L.K.E.R. series
Exec=stalker-save
Icon=stalker-save-editor
Terminal=false
Type=Application
Categories=Game;Utility;
EOF
chmod 644 "${APPDIR}/stalker-save-editor.desktop"

# 3. AppRun script
cat << 'EOF' > "${APPDIR}/AppRun"
#!/bin/sh
SELF=$(readlink -f "$0")
APPDIR=$(dirname "$SELF")
export PATH="${APPDIR}/usr/bin:${PATH}"
exec "${APPDIR}/usr/bin/stalker-save" "$@"
EOF
chmod 755 "${APPDIR}/AppRun"

# 4. Construct self-extracting runtime wrapper without network downloads
RUNTIME_WRAPPER="${WORK_DIR}/runtime.sh"
cat << 'EOF' > "${RUNTIME_WRAPPER}"
#!/usr/bin/env bash
# Standalone AppImage runtime self-extractor
set -euo pipefail
APPIMAGE=$(readlink -f "$0")
TARGET_DIR="${TMPDIR:-/tmp}/.mount_sse_${USER:-user}_$$"
mkdir -p "${TARGET_DIR}"
trap 'rm -rf "${TARGET_DIR}"' EXIT INT TERM

# Extract payload located after the exit line
ARCHIVE_START=$(awk '/^__ARCHIVE_BELOW__/ {print NR + 1; exit 0; }' "$0")
tail -n +"${ARCHIVE_START}" "$0" | tar -xz -C "${TARGET_DIR}"

exec "${TARGET_DIR}/AppRun" "$@"
exit 0
__ARCHIVE_BELOW__
EOF

(cd "${APPDIR}" && tar -czf "${WORK_DIR}/payload.tar.gz" .)

cat "${RUNTIME_WRAPPER}" "${WORK_DIR}/payload.tar.gz" > "${OUTPUT_APPIMAGE}"
chmod 755 "${OUTPUT_APPIMAGE}"

echo "Created AppImage: ${OUTPUT_APPIMAGE}"
check_budget "${OUTPUT_APPIMAGE}" "${BUDGET_APPIMAGE_BYTES}" "Linux AppImage"
