#!/usr/bin/env bash
# Master packaging entrypoint.
# Builds release binaries and packages without network downloads.
# Enforces PLAN.md package size limits and clean-checkout time bounds (<= 5 min).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

START_TIME=$(date +%s)
echo "=== Packaging S.T.A.L.K.E.R. Save Editor ==="

# 1. Build release binaries
echo "Building sse-cli in release mode..."
"${CARGO_CMD}" build --release -p sse-cli

# Verify CLI size budget
check_budget "${PROJECT_ROOT}/target/release/stalker-save" "${BUDGET_CLI_BYTES}" "CLI Binary"

# 2. Package based on OS
case "$OSTYPE" in
    linux*)
        echo "Building Linux packages (.deb and AppImage)..."
        bash "${SCRIPT_DIR}/linux/build_deb.sh"
        bash "${SCRIPT_DIR}/linux/build_appimage.sh"
        # Also build cross-platform packages
        bash "${SCRIPT_DIR}/windows/build_windows.sh"
        bash "${SCRIPT_DIR}/macos/build_macos.sh"
        ;;
    darwin*)
        echo "Building macOS packages (.dmg)..."
        bash "${SCRIPT_DIR}/macos/build_macos.sh"
        ;;
    msys*|cygwin*|win32*)
        echo "Building Windows packages..."
        bash "${SCRIPT_DIR}/windows/build_windows.sh"
        ;;
    *)
        echo "Unknown OS: $OSTYPE, assembling generic packages..."
        bash "${SCRIPT_DIR}/linux/build_deb.sh"
        bash "${SCRIPT_DIR}/windows/build_windows.sh"
        ;;
esac

# 3. Optional sounds package
bash "${SCRIPT_DIR}/sounds/package_sounds.sh"

# 4. Enforce size gates
bash "${SCRIPT_DIR}/check_size_budget.sh"

END_TIME=$(date +%s)
DURATION=$(( END_TIME - START_TIME ))

echo "=== Packaging completed successfully in ${DURATION}s (Budget: <= 300s / 5 minutes) ==="
if (( DURATION > 300 )); then
    echo "WARNING: Packaging exceeded 5 minute budget!" >&2
fi
