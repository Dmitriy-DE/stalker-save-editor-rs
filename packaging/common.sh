#!/usr/bin/env bash
# Common configuration and size budget enforcement for packaging scripts.
# All sizes and rules correspond strictly to PLAN.md CI gates.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST_DIR="${PROJECT_ROOT}/dist"
mkdir -p "${DIST_DIR}"

CARGO_CMD="${CARGO:-$(command -v cargo 2>/dev/null || echo "${HOME}/.cargo/bin/cargo")}"

# Budget limits in bytes from PLAN.md
BUDGET_CLI_BYTES=3145728          # <= 3 MiB
BUDGET_DEB_BYTES=31457280         # <= 30 MiB
BUDGET_APPIMAGE_BYTES=36700160    # <= 35 MiB
BUDGET_WIN_INSTALLER_BYTES=31457280 # <= 30 MiB
BUDGET_WIN_PORTABLE_BYTES=31457280  # <= 30 MiB
BUDGET_MACOS_DMG_BYTES=36700160   # <= 35 MiB

get_version() {
    local toml="${PROJECT_ROOT}/Cargo.toml"
    if [[ -f "${toml}" ]]; then
        grep -m1 '^version' "${toml}" | cut -d '"' -f2
    else
        echo "2.0.0-dev"
    fi
}

check_budget() {
    local file_path="$1"
    local max_bytes="$2"
    local artifact_name="$3"

    if [[ ! -f "${file_path}" ]]; then
        echo "ERROR: Artifact not found: ${file_path}" >&2
        return 1
    fi

    local actual_bytes
    if [[ "$OSTYPE" == "darwin"* ]]; then
        actual_bytes=$(stat -f%z "${file_path}")
    else
        actual_bytes=$(stat -c%s "${file_path}")
    fi

    local actual_mib
    actual_mib=$(awk "BEGIN {printf \"%.2f\", ${actual_bytes}/1048576}")
    local budget_mib
    budget_mib=$(awk "BEGIN {printf \"%.2f\", ${max_bytes}/1048576}")

    if (( actual_bytes > max_bytes )); then
        echo "FAIL [Size Gate]: ${artifact_name} size ${actual_mib} MiB (${actual_bytes} bytes) exceeds budget of ${budget_mib} MiB (${max_bytes} bytes)!" >&2
        return 1
    else
        echo "PASS [Size Gate]: ${artifact_name} size ${actual_mib} MiB is within budget (${budget_mib} MiB limit)."
        return 0
    fi
}
