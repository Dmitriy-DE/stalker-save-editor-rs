#!/usr/bin/env bash
# Builds an unsigned Debian repository index for .deb packages (deterministic output).
#
# Usage: tools/apt-repo.sh <deb-dir> <repo-dir>
#
# The signing key is not used here. The release owner signs dists/<suite>/Release afterwards
# (InRelease or Release.gpg) with the owner's own key.
set -euo pipefail

SUITE="${APT_SUITE:-stable}"
COMPONENT="main"
ARCH="amd64"
ORIGIN="Stalker Save Editor"
LABEL="Stalker Save Editor"

die() {
    printf 'apt-repo: %s\n' "$*" >&2
    exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 <deb-dir> <repo-dir>"
DEB_DIR="$1"
REPO_DIR="$2"
[[ -d "${DEB_DIR}" ]] || die "not a directory: ${DEB_DIR}"

EPOCH="${SOURCE_DATE_EPOCH:?SOURCE_DATE_EPOCH must be set for reproducible indexes}"
[[ "${EPOCH}" =~ ^[0-9]+$ ]] || die "SOURCE_DATE_EPOCH must be an integer"

for tool in dpkg-deb gzip sha256sum md5sum date; do
    command -v "${tool}" >/dev/null 2>&1 || die "required tool not found: ${tool}"
done

POOL_REL="pool/${COMPONENT}/s/stalker-save-editor"
BIN_REL="dists/${SUITE}/${COMPONENT}/binary-${ARCH}"
mkdir -p "${REPO_DIR}/${POOL_REL}" "${REPO_DIR}/${BIN_REL}"

shopt -s nullglob
debs=("${DEB_DIR}"/*.deb)
[[ ${#debs[@]} -gt 0 ]] || die "no .deb files in ${DEB_DIR}"

packages_file="${REPO_DIR}/${BIN_REL}/Packages"
: >"${packages_file}"
for deb in "${debs[@]}"; do
    name="$(basename "${deb}")"
    cp "${deb}" "${REPO_DIR}/${POOL_REL}/${name}"
    # Control fields come from the package itself; the index only adds delivery fields.
    dpkg-deb -f "${deb}" >>"${packages_file}"
    {
        printf 'Filename: %s/%s\n' "${POOL_REL}" "${name}"
        printf 'Size: %s\n' "$(stat -c '%s' "${deb}")"
        printf 'MD5sum: %s\n' "$(md5sum "${deb}" | awk '{print $1}')"
        printf 'SHA256: %s\n' "$(sha256sum "${deb}" | awk '{print $1}')"
    } >>"${packages_file}"
    printf '\n' >>"${packages_file}"
done

# gzip -n omits the name and timestamp, so the compressed index is byte-stable.
gzip -n -9 -c "${packages_file}" >"${packages_file}.gz"

release_file="${REPO_DIR}/dists/${SUITE}/Release"
{
    printf 'Origin: %s\n' "${ORIGIN}"
    printf 'Label: %s\n' "${LABEL}"
    printf 'Suite: %s\n' "${SUITE}"
    printf 'Codename: %s\n' "${SUITE}"
    printf 'Date: %s\n' "$(date -u -R -d "@${EPOCH}")"
    printf 'Architectures: %s\n' "${ARCH}"
    printf 'Components: %s\n' "${COMPONENT}"
    printf 'Description: Stalker Save Editor packages\n'
    printf 'MD5Sum:\n'
    for f in "${BIN_REL}/Packages" "${BIN_REL}/Packages.gz"; do
        printf ' %s %s %s\n' "$(md5sum "${REPO_DIR}/${f}" | awk '{print $1}')" "$(stat -c '%s' "${REPO_DIR}/${f}")" "${f#dists/${SUITE}/}"
    done
    printf 'SHA256:\n'
    for f in "${BIN_REL}/Packages" "${BIN_REL}/Packages.gz"; do
        printf ' %s %s %s\n' "$(sha256sum "${REPO_DIR}/${f}" | awk '{print $1}')" "$(stat -c '%s' "${REPO_DIR}/${f}")" "${f#dists/${SUITE}/}"
    done
} >"${release_file}"

printf 'apt-repo: wrote %s packages to %s (unsigned)\n' "${#debs[@]}" "${REPO_DIR}" >&2
printf 'apt-repo: sign dists/%s/Release with the owner key to create InRelease and Release.gpg\n' "${SUITE}" >&2
