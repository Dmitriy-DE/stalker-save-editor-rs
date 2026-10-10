#!/usr/bin/env bash
# Build release archives using the shared Rust target cache and emit the G5 manifest.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CARGO_CMD="${CARGO:-${HOME}/.cargo/bin/cargo}"
TARGET_CACHE="${HOME}/.cache/sse-target"
DIST_DIR="${PROJECT_ROOT}/dist"
METADATA_DIR="${DIST_DIR}/.package-metadata"
DOWNLOAD_BASE_URL="${DOWNLOAD_BASE_URL:-https://save-editor-downloads.save-editor.workers.dev}"
SSE_PACKAGE_STAGE=""

export CARGO_TARGET_DIR="${TARGET_CACHE}"

usage() {
    cat <<'EOF'
Usage:
  tools/package.sh [--target RUST_TARGET ...]
  tools/package.sh --manifest-only

With no target arguments, attempts all four release targets and writes a G5
manifest only when every required platform artifact is available. With one or
more --target arguments, builds only those targets; run --manifest-only after
combining their dist/ outputs to finalize the release.

Targets:
  x86_64-unknown-linux-gnu
  x86_64-pc-windows-gnu
  aarch64-apple-darwin
  x86_64-apple-darwin

The Windows portable archive is built on Windows with tools/package-windows-portable.ps1 (MSVC); the GNU target is not shipped.
The optional Windows installer is built on Windows with
  tools/package-windows-installer.ps1

Set DOWNLOAD_BASE_URL to the HTTPS artifact host before manifest generation.
The generated latest.json.sig is empty; the release owner must sign latest.json.
EOF
}

die() {
    printf 'package: %s\n' "$*" >&2
    exit 1
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || die "required tool not found: $1"
}

get_file_size() {
    if stat -c '%s' "$1" >/dev/null 2>&1; then
        stat -c '%s' "$1"
    else
        stat -f '%z' "$1"
    fi
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "sha256sum or shasum is required"
    fi
}

check_clean_source() {
    if [[ -n "$(git -C "${PROJECT_ROOT}" status --porcelain --untracked-files=normal)" ]]; then
        die "release source must be a clean git checkout"
    fi
}

read_release_identity() {
    VERSION="$(awk -F '"' '/^version = "/ { print $2; exit }' "${PROJECT_ROOT}/Cargo.toml")"
    [[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?(\+[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$ ]] || die "workspace version is not supported by the G5 parser: ${VERSION}"
    SOURCE_COMMIT="$(git -C "${PROJECT_ROOT}" rev-parse --verify HEAD)"
    [[ "${SOURCE_COMMIT}" =~ ^[0-9a-f]{40,64}$ ]] || die "could not read a lowercase source commit SHA"
    SOURCE_DATE_EPOCH="$(git -C "${PROJECT_ROOT}" show -s --format=%ct HEAD)"
    [[ "${SOURCE_DATE_EPOCH}" =~ ^[0-9]+$ ]] || die "could not read the source commit timestamp"
    [[ "${DOWNLOAD_BASE_URL}" =~ ^https://[A-Za-z0-9.-]+(:[0-9]+)?(/[A-Za-z0-9._/-]*)?$ ]] || die "DOWNLOAD_BASE_URL must be a plain HTTPS base URL"
    DOWNLOAD_BASE_URL="${DOWNLOAD_BASE_URL%/}"
    [[ -n "${DOWNLOAD_BASE_URL}" ]] || die "DOWNLOAD_BASE_URL must include a host"
}

check_release_checkout() {
    [[ -x "${CARGO_CMD}" ]] || command -v "${CARGO_CMD}" >/dev/null 2>&1 || die "cargo not found at ${CARGO_CMD}"
    [[ -f "${PROJECT_ROOT}/Cargo.lock" ]] || die "Cargo.lock is required for locked release builds"
    check_clean_source
    read_release_identity
    mkdir -p "${DIST_DIR}" "${METADATA_DIR}" "${TARGET_CACHE}"
}

write_build_manifest() {
    local path="$1"
    local target="$2"
    local arch="$3"
    local kind="$4"
    cat >"${path}" <<EOF
{
  "target": "${target}",
  "architecture": "${arch}",
  "kind": "${kind}",
  "version": "${VERSION}",
  "source_commit": "${SOURCE_COMMIT}"
}
EOF
}

record_artifact() {
    local key="$1"
    local target="$2"
    local arch="$3"
    local kind="$4"
    local artifact="$5"
    local size digest maximum_size
    size="$(get_file_size "${artifact}")"
    digest="$(sha256_file "${artifact}")"
    [[ "${size}" =~ ^[0-9]+$ ]] && (( size > 0 && size <= 2147483648 )) || die "artifact size is outside G5 limits: ${artifact}"
    case "${key}" in
        linux-x86_64|windows-x86_64|windows-installer-x86_64|linux-deb-amd64) maximum_size=31457280 ;;
        macos-arm64|macos-x86_64) maximum_size=36700160 ;;
        *) die "no packaging size budget is defined for ${key}" ;;
    esac
    (( size <= maximum_size )) || die "${key} exceeds the repository packaging size budget (${maximum_size} bytes)"
    [[ "${digest}" =~ ^[0-9a-f]{64}$ ]] || die "invalid SHA-256 result for ${artifact}"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${SOURCE_COMMIT}" "${VERSION}" "${target}" "${arch}" "${kind}" \
        "$(basename "${artifact}")" "${size}" "${digest}" >"${METADATA_DIR}/${key}.tsv"
    printf '%s  %s\n' "${digest}" "$(basename "${artifact}")" >"${artifact}.sha256"
    printf 'Created %s (%s bytes, sha256 %s)\n' "${artifact}" "${size}" "${digest}"
}

require_linux_builder() {
    [[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || die "${1} requires an x86_64 Linux build host"
}

require_windows_linker() {
    local linker="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-${CC_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-gcc}}"
    command -v "${linker}" >/dev/null 2>&1 || die "Windows GNU target needs its linker (${linker}); no Windows archive was fabricated"
}

require_macos_builder() {
    [[ "$(uname -s)" == Darwin ]] || die "${1} requires macOS with its SDK and hdiutil; no disk image was fabricated"
    command -v hdiutil >/dev/null 2>&1 || die "${1} requires hdiutil; no disk image was fabricated"
}

build_rust_binaries() {
    local target="$1"
    "${CARGO_CMD}" build --locked --release --target "${target}" -p sse-cli -p sse-ui
}

build_linux_binaries() {
    "${CARGO_CMD}" build --locked --release -p sse-cli -p sse-ui
}

build_linux() {
    require_linux_builder "Linux release"
    local cli="${TARGET_CACHE}/release/stalker-save"
    local shell_bin="${TARGET_CACHE}/release/sse-shell"
    local stage
    need_command tar
    need_command gzip
    need_command ar
    build_linux_binaries
    [[ -x "${cli}" && -x "${shell_bin}" ]] || die "Linux release binaries were not produced"
    local cli_size
    cli_size="$(get_file_size "${cli}")"
    (( cli_size <= 3145728 )) || die "stalker-save exceeds the 3 MiB CLI size budget"

    stage="$(mktemp -d "${TMPDIR:-/tmp}/sse-package-linux.XXXXXX")"
    SSE_PACKAGE_STAGE="${stage}"
    trap 'rm -rf -- "$SSE_PACKAGE_STAGE"' EXIT
    cp "${cli}" "${stage}/stalker-save"
    cp "${shell_bin}" "${stage}/sse-shell"
    cp "${PROJECT_ROOT}/packaging/icons/stalker-save-editor.svg" "${stage}/stalker-save-editor.svg"
    cp "${PROJECT_ROOT}/packaging/linux/stalker-save-editor.desktop" "${stage}/stalker-save-editor.desktop"
    write_build_manifest "${stage}/BUILD_MANIFEST.json" linux x86_64 portable
    tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner \
        -C "${stage}" -cf - stalker-save sse-shell stalker-save-editor.svg stalker-save-editor.desktop BUILD_MANIFEST.json \
        | gzip -n >"${DIST_DIR}/SaveEditor-linux-x86_64.tar.gz"
    record_artifact linux-x86_64 linux-x86_64 x86_64 portable "${DIST_DIR}/SaveEditor-linux-x86_64.tar.gz"

    local deb_stage="${stage}/deb"
    mkdir -p "${deb_stage}/DEBIAN" "${deb_stage}/usr/bin" "${deb_stage}/usr/share/stalker-save-editor" \
        "${deb_stage}/usr/share/applications" "${deb_stage}/usr/share/metainfo" "${deb_stage}/usr/share/icons/hicolor/scalable/apps"
    cp "${cli}" "${deb_stage}/usr/bin/stalker-save"
    cp "${shell_bin}" "${deb_stage}/usr/bin/sse-shell"
    cp "${PROJECT_ROOT}/packaging/linux/stalker-save-editor.desktop" "${deb_stage}/usr/share/applications/"
    cp "${PROJECT_ROOT}/packaging/linux/org.stalker_save_editor.SaveEditor.metainfo.xml" "${deb_stage}/usr/share/metainfo/"
    cp "${PROJECT_ROOT}/packaging/icons/stalker-save-editor.svg" "${deb_stage}/usr/share/icons/hicolor/scalable/apps/"
    write_build_manifest "${deb_stage}/usr/share/stalker-save-editor/BUILD_MANIFEST.json" linux x86_64 package
    local installed_size_kib
    installed_size_kib="$(du -sk "${deb_stage}/usr" | awk '{ print $1 }')"
    cat >"${deb_stage}/DEBIAN/control" <<EOF
Package: stalker-save-editor
Version: ${VERSION}
Section: games
Priority: optional
Architecture: amd64
Installed-Size: ${installed_size_kib}
Maintainer: S.T.A.L.K.E.R. Save Editor Team <dev@stalker-save-editor.org>
Depends: libcurl4 | libcurl4t64, libx11-6
Recommends: zenity
Description: S.T.A.L.K.E.R. save editor and shell
EOF
    chmod 0755 "${deb_stage}/usr/bin/stalker-save" "${deb_stage}/usr/bin/sse-shell"
    printf '2.0\n' >"${stage}/debian-binary"
    (cd "${deb_stage}/DEBIAN" && tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner -cf - control | gzip -n >"${stage}/control.tar.gz")
    (cd "${deb_stage}" && tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner -cf - usr | gzip -n >"${stage}/data.tar.gz")
    local deb_name="stalker-save-editor_${VERSION}_amd64.deb"
    (cd "${stage}" && ar rcsD "${DIST_DIR}/${deb_name}" debian-binary control.tar.gz data.tar.gz)
    record_artifact linux-deb-amd64 linux-deb-amd64 x86_64 package "${DIST_DIR}/${deb_name}"
}

build_windows() {
    local target="x86_64-pc-windows-gnu"
    require_windows_linker
    local cli="${TARGET_CACHE}/${target}/release/stalker-save.exe"
    local shell_bin="${TARGET_CACHE}/${target}/release/sse-shell.exe"
    need_command zip
    build_rust_binaries "${target}"
    [[ -f "${cli}" && -f "${shell_bin}" ]] || die "Windows release binaries were not produced"
    local cli_size
    cli_size="$(get_file_size "${cli}")"
    (( cli_size <= 3145728 )) || die "stalker-save.exe exceeds the 3 MiB CLI size budget"
    local stage
    stage="$(mktemp -d "${TMPDIR:-/tmp}/sse-package-windows.XXXXXX")"
    SSE_PACKAGE_STAGE="${stage}"
    trap 'rm -rf -- "$SSE_PACKAGE_STAGE"' EXIT
    cp "${cli}" "${stage}/stalker-save.exe"
    cp "${shell_bin}" "${stage}/sse-shell.exe"
    cp "${PROJECT_ROOT}/packaging/icons/stalker-save-editor.svg" "${stage}/stalker-save-editor.svg"
    write_build_manifest "${stage}/BUILD_MANIFEST.json" windows x86_64 portable
    (cd "${stage}" && zip -q "${DIST_DIR}/SaveEditor-windows-x86_64.zip" stalker-save.exe sse-shell.exe stalker-save-editor.svg BUILD_MANIFEST.json)
    record_artifact windows-x86_64 windows-x86_64 x86_64 portable "${DIST_DIR}/SaveEditor-windows-x86_64.zip"
}

build_macos() {
    local target="$1"
    local arch="$2"
    local key="$3"
    local bundle_arch="$4"
    require_macos_builder "macOS ${arch} release"
    local cli="${TARGET_CACHE}/${target}/release/stalker-save"
    local shell_bin="${TARGET_CACHE}/${target}/release/sse-shell"
    build_rust_binaries "${target}"
    [[ -x "${cli}" && -x "${shell_bin}" ]] || die "macOS ${arch} release binaries were not produced"
    local stage
    stage="$(mktemp -d "${TMPDIR:-/tmp}/sse-package-macos.XXXXXX")"
    SSE_PACKAGE_STAGE="${stage}"
    trap 'rm -rf -- "$SSE_PACKAGE_STAGE"' EXIT
    mkdir -p "${stage}/SaveEditor.app/Contents/MacOS" "${stage}/SaveEditor.app/Contents/Resources"
    cp "${cli}" "${stage}/SaveEditor.app/Contents/MacOS/stalker-save"
    cp "${shell_bin}" "${stage}/SaveEditor.app/Contents/MacOS/sse-shell"
    chmod 0755 "${stage}/SaveEditor.app/Contents/MacOS/stalker-save" "${stage}/SaveEditor.app/Contents/MacOS/sse-shell"
    write_build_manifest "${stage}/SaveEditor.app/Contents/Resources/BUILD_MANIFEST.json" macos "${arch}" disk-image
    sed -e "s/@ARCH@/${bundle_arch}/g" -e "s/@VERSION@/${VERSION}/g" \
        "${PROJECT_ROOT}/packaging/macos/Info.plist.in" >"${stage}/SaveEditor.app/Contents/Info.plist"
    cp "${PROJECT_ROOT}/packaging/icons/stalker-save-editor.svg" "${stage}/SaveEditor.app/Contents/Resources/stalker-save-editor.svg"
    local dmg_name="SaveEditor-macos-${arch}.dmg"
    hdiutil create -volname "SaveEditor-${arch}" -srcfolder "${stage}" -ov -format UDZO "${DIST_DIR}/${dmg_name}" >/dev/null
    record_artifact "${key}" "${key}" "${arch}" disk-image "${DIST_DIR}/${dmg_name}"
}

load_artifact_metadata() {
    local key="$1"
    local metadata="${METADATA_DIR}/${key}.tsv"
    [[ -f "${metadata}" ]] || return 1
    IFS=$'\t' read -r artifact_commit artifact_version artifact_target artifact_arch artifact_kind artifact_file artifact_size artifact_sha <"${metadata}"
    [[ "${artifact_commit}" == "${SOURCE_COMMIT}" ]] || die "${key} artifact was built from a different source commit"
    [[ "${artifact_version}" == "${VERSION}" ]] || die "${key} artifact has a different version"
    [[ "${artifact_size}" =~ ^[0-9]+$ && "${artifact_size}" -gt 0 && "${artifact_size}" -le 2147483648 ]] || die "${key} artifact has invalid size metadata"
    [[ "${artifact_sha}" =~ ^[0-9a-f]{64}$ ]] || die "${key} artifact has invalid SHA-256 metadata"
    local artifact_path="${DIST_DIR}/${artifact_file}"
    [[ -f "${artifact_path}" ]] || die "${key} artifact file is missing: ${artifact_path}"
    [[ "$(get_file_size "${artifact_path}")" == "${artifact_size}" ]] || die "${key} artifact size changed since packaging"
    [[ "$(sha256_file "${artifact_path}")" == "${artifact_sha}" ]] || die "${key} artifact SHA-256 changed since packaging"
    [[ -f "${artifact_path}.sha256" ]] || die "${key} SHA-256 sidecar is missing"
    [[ "$(cat "${artifact_path}.sha256")" == "${artifact_sha}  ${artifact_file}" ]] || die "${key} SHA-256 sidecar is invalid"
}

emit_artifact() {
    local key="$1"
    local comma="$2"
    load_artifact_metadata "${key}"
    [[ "${artifact_target}" == "${key}" ]] || die "${key} metadata contains the wrong target"
    printf '%s\n    "%s": {\n' "${comma}" "${key}"
    printf '      "architecture": "%s",\n' "${artifact_arch}"
    printf '      "file": "%s",\n' "${artifact_file}"
    printf '      "kind": "%s",\n' "${artifact_kind}"
    printf '      "sha256": "%s",\n' "${artifact_sha}"
    printf '      "size": %s,\n' "${artifact_size}"
    printf '      "target": "%s",\n' "${artifact_target}"
    printf '      "url": "%s/%s"\n    }' "${DOWNLOAD_BASE_URL}" "${artifact_file}"
}

write_artifact_group() {
    local kind="$1"
    shift
    local first=1 key
    for key in "$@"; do
        if [[ -f "${METADATA_DIR}/${key}.tsv" ]]; then
            local comma=,
            (( first == 1 )) && comma=""
            emit_artifact "${key}" "${comma}"
            first=0
        elif [[ "${kind}" == required ]]; then
            die "cannot write G5 manifest: missing ${key}; run package.sh for that target first"
        fi
    done
    printf '\n'
}

write_manifest() {
    local published_at temporary
    load_artifact_metadata windows-x86_64
    load_artifact_metadata linux-x86_64
    load_artifact_metadata linux-deb-amd64
    published_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    temporary="${DIST_DIR}/latest.json.tmp"
    {
        printf '{\n  "schema": 1,\n  "channel": "stable",\n  "version": "%s",\n' "${VERSION}"
        printf '  "source_commit": "%s",\n  "published_at": "%s",\n' "${SOURCE_COMMIT}" "${published_at}"
        printf '  "artifacts": {\n'
        write_artifact_group required windows-x86_64 linux-x86_64 linux-deb-amd64
        printf '  },\n  "optional_artifacts": {\n'
        write_artifact_group optional windows-installer-x86_64 macos-arm64 macos-x86_64
        printf '  }\n}\n'
    } >"${temporary}"
    mv "${temporary}" "${DIST_DIR}/latest.json"
    printf '%s  latest.json\n' "$(sha256_file "${DIST_DIR}/latest.json")" >"${DIST_DIR}/latest.json.sha256"
    : >"${DIST_DIR}/latest.json.sig"
    printf 'Created G5 manifest: %s/latest.json\n' "${DIST_DIR}"
    printf 'Signature placeholder is empty; the release owner must sign latest.json before publishing.\n'
}

main() {
    local manifest_only=0 selected=0 build_one=0 build_one_target=""
    local -a targets=()
    while (($#)); do
        case "$1" in
            --help|-h)
                usage
                return 0
                ;;
            --target)
                (($# >= 2)) || die "--target requires a Rust target triple"
                targets+=("$2")
                selected=1
                shift 2
                ;;
            --manifest-only)
                manifest_only=1
                shift
                ;;
            --build-one)
                (($# >= 2)) || die "--build-one requires a Rust target triple"
                build_one=1
                build_one_target="$2"
                shift 2
                ;;
            *) die "unknown argument: $1" ;;
        esac
    done
    (( manifest_only == 0 || selected == 0 )) || die "--manifest-only cannot be combined with --target"
    check_release_checkout
    if (( manifest_only == 1 )); then
        write_manifest
        return 0
    fi

    if (( build_one == 1 )); then
        case "${build_one_target}" in
            x86_64-unknown-linux-gnu) build_linux ;;
            x86_64-pc-windows-gnu) build_windows ;;
            aarch64-apple-darwin) build_macos "${build_one_target}" arm64 macos-arm64 arm64 ;;
            x86_64-apple-darwin) build_macos "${build_one_target}" x86_64 macos-x86_64 x86_64 ;;
            *) die "unsupported target: ${build_one_target}" ;;
        esac
        return 0
    fi

    if (( selected == 0 )); then
        targets=(x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin)
    fi

    local failures=0 target
    for target in "${targets[@]}"; do
        printf '\n=== Packaging %s ===\n' "${target}"
        if "${SCRIPT_DIR}/package.sh" --build-one "${target}"; then
            :
        else
            failures=$((failures + 1))
        fi
    done

    if (( selected == 0 && failures == 0 )); then
        write_manifest || failures=$((failures + 1))
    elif (( selected == 0 )); then
        printf '\nG5 manifest not written because one or more platform builds failed.\n' >&2
    fi
    (( failures == 0 )) || return 1
}

main "$@"
