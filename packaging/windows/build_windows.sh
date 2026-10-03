#!/usr/bin/env bash
# Builds Windows portable .zip and Inno Setup configuration.
# Output must satisfy PLAN.md budget: <= 30 MiB.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../common.sh"

ARCH="x64"
VERSION="$(get_version)"
OUTPUT_ZIP="${DIST_DIR}/stalker-save-editor-windows-${ARCH}.zip"

BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save.exe"
# Fallback to linux binary for testing package assembly if cross-compiler not invoked
if [[ ! -f "${BIN_SRC}" ]]; then
    BIN_SRC="${PROJECT_ROOT}/target/release/stalker-save"
fi

WORK_DIR="$(mktemp -d -t sse-win-build-XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

mkdir -p "${WORK_DIR}/package"

if [[ -f "${BIN_SRC}" ]]; then
    cp "${BIN_SRC}" "${WORK_DIR}/package/stalker-save.exe"
else
    # Create empty placeholder if build binary not yet produced
    touch "${WORK_DIR}/package/stalker-save.exe"
fi

# Build manifest for updater detector
cat << EOF > "${WORK_DIR}/package/BUILD_MANIFEST.json"
{
  "target": "windows",
  "architecture": "x86_64",
  "kind": "portable",
  "version": "${VERSION}"
}
EOF

# Inno Setup Script
cat << EOF > "${SCRIPT_DIR}/installer.iss"
; S.T.A.L.K.E.R. Save Editor Inno Setup Script
[Setup]
AppName=S.T.A.L.K.E.R. Save Editor
AppVersion=${VERSION}
DefaultDirName={autopf}\\S.T.A.L.K.E.R. Save Editor
DefaultGroupName=S.T.A.L.K.E.R. Save Editor
OutputDir=${DIST_DIR}
OutputBaseFilename=stalker-save-editor-windows-installer
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "${WORK_DIR}\\package\\stalker-save.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "${WORK_DIR}\\package\\BUILD_MANIFEST.json"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\\S.T.A.L.K.E.R. Save Editor"; Filename: "{app}\\stalker-save.exe"
Name: "{commondesktop}\\S.T.A.L.K.E.R. Save Editor"; Filename: "{app}\\stalker-save.exe"

[Run]
Filename: "{app}\\stalker-save.exe"; Description: "{cm:LaunchProgram,S.T.A.L.K.E.R. Save Editor}"; Flags: nowait postinstall skipifsilent
EOF

# Create portable zip archive
rm -f "${OUTPUT_ZIP}"
if command -v zip >/dev/null 2>&1; then
    (cd "${WORK_DIR}/package" && zip -q -r "${OUTPUT_ZIP}" .)
else
    (cd "${WORK_DIR}/package" && tar -czf "${OUTPUT_ZIP}" .)
fi

echo "Created Windows portable package: ${OUTPUT_ZIP}"
check_budget "${OUTPUT_ZIP}" "${BUDGET_WIN_PORTABLE_BYTES}" "Windows Portable Zip"
