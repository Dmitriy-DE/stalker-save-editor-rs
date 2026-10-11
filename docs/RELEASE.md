# Releasing

How a version goes out, step by step, and who does each step. Background: [PACKAGING.md](PACKAGING.md) (what the
jobs build) and `.github/workflows/ci.yml`.

Two roles:

- **Integrator** — whoever merges pull requests. Works only with the repository and CI. Has no keys.
- **Owner** — the only one who holds the update signing key, the Steam account, the APT key and the download host.
  Everything that publishes something or signs something is the owner's.

Nothing in CI publishes, signs or tags. A `v*` tag starts the release jobs, so the tag itself is an owner step.

## State of 2.0.0

`Cargo.toml` says `2.0.0`. There is no tag and nothing is published. Before the owner tags, the open points of
[ACCEPTANCE.md](../ACCEPTANCE.md), part V, are theirs to walk through: sections 1–10 (the check in the games),
section 12 (edits enabled without a game check), section 13 (S.T.A.L.K.E.R. 2 control saves).

## Before the tag — integrator

1. `main` is green: `check` on Linux, Windows and macOS, `packages-linux`, `windows-portable`,
   `windows-installer`, `release-rehearsal`.
2. The version is the same everywhere: `Cargo.toml` (`[workspace.package] version`), `Cargo.lock`, the README
   line, the title of `docs/RELEASE_NOTES_<version>.md`. `cargo metadata --locked` must pass.
3. The release notes say what is not verified, in the words of ACCEPTANCE part V, section 14 and `TASKS.md`.
4. Download the artifacts of the last `main` run and look at them: `packages-Linux` (AppImage, `.tar.gz`,
   `.deb`), `packages-Windows-portable`, `windows-installer`, and `web` once the web job is merged.
   `release-rehearsal` must show that the shipped public key refuses a foreign signature.
5. Hand over to the owner: the commit to tag, the run with its artifacts, the release notes, and the list of what
   was not verified. The integrator does not tag.

## The release — owner only

6. **Check in the games** (ACCEPTANCE part V). Anything that fails goes back to the integrator as an issue; the
   tag waits.
7. **Tag.** `git tag -s v2.0.0 <commit>` and `git push origin v2.0.0`. The tag starts `packages` (macOS disk
   images, plus Linux) and `release-manifest`. The same jobs can be started by hand (`workflow_dispatch`) for a
   dry run without a tag.
8. **Collect the packages.** From the tag's run download `packages-*`, `windows-installer` and
   `release-manifest-UNSIGNED-DO-NOT-PUBLISH` into one `dist/` folder. That artifact holds `latest.json`,
   `latest.json.sha256` and an empty `latest.json.sig`.
   The manifest's links point at `DOWNLOAD_BASE_URL` (default `https://save-editor-downloads.save-editor.workers.dev`,
   set in `tools/package.sh`). To rebuild the manifest for another host: put the packages into `dist/` of a clean
   checkout of the tag and run `DOWNLOAD_BASE_URL=https://<host> tools/package.sh --manifest-only`.
9. **Sign `latest.json`** on the owner's machine, with the owner's key (never in CI, never by an agent). The
   signature is ECDSA P-256 over SHA-256, DER, base64 on one line — the same shape the rehearsal job produces:

   ```sh
   openssl dgst -sha256 -sign <owner key.pem> -out latest.json.der dist/latest.json
   base64 -w0 latest.json.der > dist/latest.json.sig
   ```

10. **Verify the signature with the shipped public key** before anything is uploaded:

    ```sh
    cargo run --locked --release -p sse-cli -- update verify-manifest dist/latest.json dist/latest.json.sig
    ```

    It must accept. Change one byte of a copy of `latest.json` and it must refuse.
11. **Upload the packages first, the manifest last.** Every file named in `latest.json` goes to the download
    host under exactly that name; check each one's SHA-256 against the manifest after the upload. Only then
    upload `latest.json` and `latest.json.sig`: installed editors see the release from that moment.
12. **GitHub release** for the tag: the same packages, `latest.json`, `latest.json.sig`, and the text of
    `docs/RELEASE_NOTES_2.0.0.md`.
13. **APT.** `tools/apt-repo.sh <deb-dir> <repo-dir>` builds the unsigned index. Sign
    `dists/stable/Release` with the owner's OpenPGP key (`InRelease`, or `Release.gpg`), then upload the
    repository. The key does not exist yet (TASKS.md, A23); without it there is no APT channel in 2.0.0.
14. **Web edition.** Publish the `web` artifact (the whole folder: `index.html`, `app.js`, `sse_web.wasm`,
    `item-icons.bin`) to the static host.
15. **Steam Workshop.** Stage the three Enhanced Edition packages from the tagged commit, check them in the
    games, upload them from the owner's Steam account and write the three item numbers into
    `crates/sse-companion/src/workshop.rs` — [WORKSHOP.md](WORKSHOP.md). The numbers land in a later patch
    release: 2.0.0 itself ships with "not published".
16. **Check the update from outside:** a 2.0.0 build asks for updates and reports "up to date"; a build with a
    lower version is offered 2.0.0, downloads it and refuses a tampered file.

## After the release — integrator

17. Bump `Cargo.toml` to the next development version (`2.0.1-dev` or `2.1.0-dev`) and `Cargo.lock` with it.
18. Update `TASKS.md` (the "Выпуск 2.0" row) and the README line with what was actually published.
19. When the owner sends the Workshop numbers: put them into `workshop.rs`, change the guarding test in the
    same pull request.

## If something goes wrong

- A wrong package is already uploaded but the manifest is not: replace the package; nobody has seen it.
- The manifest is already uploaded: do not edit packages in place. Upload a corrected version under a new
  version number and a new signed manifest. An installed editor refuses a package whose hash differs from the
  signed manifest, so a silent swap only breaks updates.
- A leaked or lost signing key cannot be fixed by a new manifest: the public key is compiled into every
  released editor (`crates/sse-update/src/signature.rs`). It needs a new key and a release that users install
  by hand.

## Not verified

- This procedure has never been run to the end. The signing shape is copied from the `release-rehearsal` job,
  which uses a throwaway key; the owner's key and their own signing commands were not looked at.
- Whether the download host already serves a manifest for the C# 1.3.x line, and what a 1.3.x editor does when
  it meets a 2.0.0 manifest there, was not checked. Decide before step 11 whether 2.0.0 uses the same address.
- The macOS disk images are built only by the tag's run, unsigned and not notarized; nobody has opened one.
- No package was installed or started on a clean system by CI (PACKAGING.md, "What is not verified").
- The upload commands for the download host and the static host are the owner's and are not written down here.
