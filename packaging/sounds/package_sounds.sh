#!/usr/bin/env bash
# Packages optional standalone game sounds into an archive without network downloads.
# In S.T.A.L.K.E.R. Save Editor 2.0, sounds are decoded directly from game archives
# and sounds package is completely optional.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../common.sh"

OUTPUT_SOUNDS="${DIST_DIR}/stalker-save-editor-sounds.tar.gz"

WORK_DIR="$(mktemp -d -t sse-sounds-XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

mkdir -p "${WORK_DIR}/sounds"

# Sound placeholder manifest
cat << 'EOF' > "${WORK_DIR}/sounds/SOUNDS_MANIFEST.json"
{
  "description": "Optional standalone UI sound pack for S.T.A.L.K.E.R. Save Editor",
  "note": "Standard installation reads Ogg Vorbis sounds straight from the game's installed archives"
}
EOF

(cd "${WORK_DIR}" && tar -czf "${OUTPUT_SOUNDS}" sounds)

echo "Created optional sounds package: ${OUTPUT_SOUNDS}"
