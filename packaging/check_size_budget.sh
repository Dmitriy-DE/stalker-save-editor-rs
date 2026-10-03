#!/usr/bin/env bash
# Verification of all built artifacts against PLAN.md size budget gates.
# Exits with 1 if any package or binary exceeds its budget.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

echo "=== Verifying Packaging Size Gates (PLAN.md) ==="
FAILED=0

# 1. CLI binary gate: <= 3 MiB
CLI_BIN="${PROJECT_ROOT}/target/release/stalker-save"
if [[ -f "${CLI_BIN}" ]]; then
    check_budget "${CLI_BIN}" "${BUDGET_CLI_BYTES}" "CLI Binary (target/release/stalker-save)" || FAILED=1
fi

# 2. Check artifacts in dist/
for f in "${DIST_DIR}"/*.deb; do
    if [[ -f "${f}" ]]; then
        check_budget "${f}" "${BUDGET_DEB_BYTES}" "Debian Package (${f##*/})" || FAILED=1
    fi
done

for f in "${DIST_DIR}"/*.AppImage; do
    if [[ -f "${f}" ]]; then
        check_budget "${f}" "${BUDGET_APPIMAGE_BYTES}" "Linux AppImage (${f##*/})" || FAILED=1
    fi
done

for f in "${DIST_DIR}"/*-windows-installer.exe "${DIST_DIR}"/*-setup.exe; do
    if [[ -f "${f}" ]]; then
        check_budget "${f}" "${BUDGET_WIN_INSTALLER_BYTES}" "Windows Installer (${f##*/})" || FAILED=1
    fi
done

for f in "${DIST_DIR}"/*-windows*.zip; do
    if [[ -f "${f}" ]]; then
        check_budget "${f}" "${BUDGET_WIN_PORTABLE_BYTES}" "Windows Portable Zip (${f##*/})" || FAILED=1
    fi
done

for f in "${DIST_DIR}"/*.dmg; do
    if [[ -f "${f}" ]]; then
        check_budget "${f}" "${BUDGET_MACOS_DMG_BYTES}" "macOS Disk Image (${f##*/})" || FAILED=1
    fi
done

if (( FAILED != 0 )); then
    echo "ERROR: One or more package size gates FAILED!" >&2
    exit 1
fi

echo "SUCCESS: All present packages and binaries satisfy PLAN.md size gates."
exit 0
