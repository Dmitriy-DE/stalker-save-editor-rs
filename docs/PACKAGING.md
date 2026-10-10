# Packaging

Packages are built by `tools/package.sh` on Linux and macOS, and by two PowerShell scripts on Windows. The CI
workflow is `.github/workflows/ci.yml`. CI uploads its packages as workflow artifacts. Nothing in CI publishes a
release, signs a package, or holds the signing key.

## Which jobs run when

| Job | Runs | Output |
|---|---|---|
| `test-build` | push to `main` | unsigned test binaries for Linux, Windows and macOS |
| `packages-linux` | every pull request | `.AppImage`, `.tar.gz`, `.deb` |
| `windows-portable` | every pull request | `SaveEditor-windows-x86_64.zip` |
| `windows-installer` | every pull request | `SaveEditor-windows-x86_64-setup.exe` |
| `packages` | by hand, or on a `v*` tag | macOS `.dmg` for `arm64` and `x86_64`, plus the Linux packages |
| `release-manifest` | by hand, or on a `v*` tag | `latest.json`, and its signature when the owner has signed it |

## Linux

`tools/package.sh` builds the Linux packages from `stalker-save` (`sse-cli`) and `sse-shell` (`sse-ui`).

- **Portable archive**: `SaveEditor-linux-x86_64.tar.gz`, written with sorted names, owner 0, a fixed timestamp
  (`SOURCE_DATE_EPOCH`, the commit time) and `gzip -n`.
- **Debian package**: `stalker-save-editor_<version>_amd64.deb`. It installs `/usr/bin/stalker-save` and
  `/usr/bin/sse-shell`, a desktop entry, metainfo, the icon and a build manifest. It is built as an `ar` archive with
  deterministic members.
- **AppImage**: `SaveEditor-linux-x86_64.AppImage`. The script downloads `appimagetool` 1.9.1 and the AppImage runtime
  over HTTPS only. Each download must match a SHA-256 pinned in the script; a mismatch is refused. CI extracts the
  AppImage and checks that `AppRun`, `usr/bin/stalker-save`, `usr/bin/sse-shell`, the desktop file and the icon are
  present. CI does not start the application.

### APT repository

`tools/apt-repo.sh <deb-dir> <repo-dir>` builds an unsigned Debian index for the `stable` suite, `main` component and
`amd64`. The index is deterministic. The signing key is not used by the script. The release owner signs
`dists/<suite>/Release` (`InRelease` or `Release.gpg`) afterwards with their own key.

## Windows

- **Portable archive**: `tools/package-windows-portable.ps1` builds the MSVC target and writes
  `SaveEditor-windows-x86_64.zip` with `stalker-save.exe`, `sse-shell.exe`, the icon and the build manifest. The GNU
  target is not shipped.
- **Installer**: `tools/package-windows-installer.ps1` builds `SaveEditor-windows-x86_64-setup.exe` with Inno Setup
  (`packaging/installer.iss`). CI installs Inno Setup with Chocolatey.

## macOS

`tools/package.sh` builds each macOS architecture separately (`arm64` and `x86_64`). It creates `SaveEditor.app` with
the two binaries, `Info.plist` and the icon, then writes `SaveEditor-macos-<arch>.dmg` with `hdiutil`. The script does
not sign or notarize the application bundle, so the images are unsigned.

## Release manifest

`tools/package.sh --manifest-only` runs after the packages are downloaded into `dist/`. It writes `latest.json`, with the
`artifacts` and `optional_artifacts` sections, and `latest.json.sha256`. The download host comes from
`DOWNLOAD_BASE_URL`, which must be HTTPS. The script writes `latest.json.sig` empty.

The owner signs `latest.json` on a local machine. The signature is ECDSA P-256 over SHA-256, and the public key is in
`crates/sse-update/src/signature.rs`. The application refuses a manifest whose signature does not verify. CI checks
the signature file. When the signature is not present, CI uploads the manifest under the name
`release-manifest-UNSIGNED-DO-NOT-PUBLISH` instead of `release-manifest`.

The build settings above (sorted archives, fixed timestamps, deterministic gzip) are meant to make the archives
reproducible. Nothing has rebuilt the packages on a second machine to confirm it.

## What is not verified

- CI builds the packages. It checks the contents of the AppImage only. It does not install or start any package.
- The macOS images are built only by hand or on a tag. CI does not test them.
- The signature step is the owner's and is not part of CI.
