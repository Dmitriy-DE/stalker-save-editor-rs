#!/usr/bin/env bash
# Builds a Debian (.deb) package without network downloads.
# Output must satisfy PLAN.md budget: <= 30 MiB.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../common.sh"

ARCH="amd64"
if [[ "$(uname -m)" == "aarch64" ]]; then
    ARCH="arm64"
fi

VERSION="$(get_version)"
PKG_NAME="stalker-save-editor"
OUTPUT_DEB="${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"

BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save"
if [[ ! -f "${BIN_SRC}" ]]; then
    echo "Building release binary..."
    "${CARGO_CMD}" build --release -p sse-cli
fi

WORK_DIR="$(mktemp -d -t sse-deb-build-XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

mkdir -p "${WORK_DIR}/DEBIAN"
mkdir -p "${WORK_DIR}/usr/bin"
mkdir -p "${WORK_DIR}/usr/share/applications"
mkdir -p "${WORK_DIR}/usr/share/pixmaps"

# 1. Binary and symlink
cp "${BIN_SRC}" "${WORK_DIR}/usr/bin/stalker-save-editor"
chmod 755 "${WORK_DIR}/usr/bin/stalker-save-editor"
ln -sf "stalker-save-editor" "${WORK_DIR}/usr/bin/stalker-save"

# 2. Desktop entry
cat << 'EOF' > "${WORK_DIR}/usr/share/applications/stalker-save-editor.desktop"
[Desktop Entry]
Name=S.T.A.L.K.E.R. Save Editor
Comment=Save game editor and fixes manager for S.T.A.L.K.E.R. series
Exec=stalker-save-editor
Icon=stalker-save-editor
Terminal=false
Type=Application
Categories=Game;Utility;
Keywords=stalker;save;editor;xray;
EOF
chmod 644 "${WORK_DIR}/usr/share/applications/stalker-save-editor.desktop"

# 3. Control file
BIN_SIZE_KB=$(du -k "${BIN_SRC}" | cut -f1)
cat << EOF > "${WORK_DIR}/DEBIAN/control"
Package: ${PKG_NAME}
Version: ${VERSION}
Section: games
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${BIN_SIZE_KB}
Maintainer: S.T.A.L.K.E.R. Save Editor Team <dev@stalker-save-editor.org>
Description: Native high-performance S.T.A.L.K.E.R. save editor and game fixes manager.
 High-performance Rust rewrite with lossless editing and zero dependencies.
EOF
chmod 644 "${WORK_DIR}/DEBIAN/control"

# 4. Package deb archive
echo "2.0" > "${WORK_DIR}/debian-binary"

TAR_CMD="tar --owner=root:0 --group=root:0"
if ! tar --owner=root:0 --group=root:0 --help >/dev/null 2>&1; then
    TAR_CMD="tar"
fi

(cd "${WORK_DIR}/DEBIAN" && ${TAR_CMD} -czf "${WORK_DIR}/control.tar.gz" .)
(cd "${WORK_DIR}" && ${TAR_CMD} -czf "${WORK_DIR}/data.tar.gz" usr)

# Use ar or python fallback if ar is unavailable
rm -f "${OUTPUT_DEB}"
if command -v ar >/dev/null 2>&1; then
    (cd "${WORK_DIR}" && ar rcs "${OUTPUT_DEB}" debian-binary control.tar.gz data.tar.gz)
else
    # Minimal pure ar writer using standard tools
    echo "ERROR: 'ar' utility required to construct .deb package" >&2
    exit 1
fi

echo "Created Debian package: ${OUTPUT_DEB}"
check_budget "${OUTPUT_DEB}" "${BUDGET_DEB_BYTES}" "Debian Package (.deb)"
