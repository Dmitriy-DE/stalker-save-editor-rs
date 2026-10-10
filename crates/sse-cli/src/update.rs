//! CLI subcommand handler for `update`.

use sse_core::ExitCode;
use sse_update::{DefaultFetch, UpdateInstallation, UpdateInstallationDetector, UpdateService, UpdateState};
use std::path::{Path, PathBuf};

const UPDATE_USAGE: &str = "Usage: stalker-save update check [OPTIONS] | \
stalker-save update download [DESTINATION] [OPTIONS]\n\
\n\
Options:\n\
  --manifest-url URL      Custom update manifest URL\n\
  --current-version VER   Override current version string (defaults to build version)\n\
  --target TARGET         Target platform override (windows, linux, macos)\n\
  --arch ARCH             Target architecture override (x86_64, arm64)\n\
  --kind KIND             Installation kind override (portable, package, installer, disk-image)\n\
  --root DIR              Installation root directory override";

/// Executes the `update` subcommand.
pub fn run_update(args: &[String]) -> ExitCode {
    let Some(first) = args.first() else {
        eprintln!("{UPDATE_USAGE}");
        return ExitCode::Usage;
    };

    let rest = args.get(1..).unwrap_or_default();
    match first.as_str() {
        "check" => run_check(rest),
        "download" => run_download(rest),
        "verify-manifest" => run_verify_manifest(rest),
        _ => {
            eprintln!("{UPDATE_USAGE}");
            ExitCode::Usage
        }
    }
}

struct ParsedOptions {
    manifest_url: Option<String>,
    current_version: Option<String>,
    target: Option<String>,
    arch: Option<String>,
    kind: Option<String>,
    root: Option<PathBuf>,
    destination: Option<PathBuf>,
}

fn parse_options(args: &[String]) -> Result<ParsedOptions, ExitCode> {
    let mut opts = ParsedOptions {
        manifest_url: None,
        current_version: None,
        target: None,
        arch: None,
        kind: None,
        root: None,
        destination: None,
    };

    let mut i = 0usize;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        match arg.as_str() {
            "--manifest-url" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.manifest_url = Some(val.clone());
                i = i.saturating_add(2);
            }
            "--current-version" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.current_version = Some(val.clone());
                i = i.saturating_add(2);
            }
            "--target" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.target = Some(val.clone());
                i = i.saturating_add(2);
            }
            "--arch" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.arch = Some(val.clone());
                i = i.saturating_add(2);
            }
            "--kind" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.kind = Some(val.clone());
                i = i.saturating_add(2);
            }
            "--root" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.root = Some(PathBuf::from(val));
                i = i.saturating_add(2);
            }
            "--output" => {
                let val = args.get(i.saturating_add(1)).ok_or(ExitCode::Usage)?;
                opts.destination = Some(PathBuf::from(val));
                i = i.saturating_add(2);
            }
            "-h" | "--help" => {
                eprintln!("{UPDATE_USAGE}");
                return Err(ExitCode::Done);
            }
            other => {
                if !other.starts_with("--") && opts.destination.is_none() {
                    opts.destination = Some(PathBuf::from(other));
                    i = i.saturating_add(1);
                } else {
                    eprintln!("Unknown option: {other}");
                    return Err(ExitCode::Usage);
                }
            }
        }
    }

    Ok(opts)
}

fn build_service(opts: &ParsedOptions) -> Result<UpdateService, ExitCode> {
    let current_ver = opts
        .current_version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    let installation = if let (Some(target), Some(arch), Some(kind)) = (&opts.target, &opts.arch, &opts.kind) {
        UpdateInstallation {
            target: target.clone(),
            architecture: arch.clone(),
            kind: kind.clone(),
            root: opts.root.clone().unwrap_or_else(|| PathBuf::from(".")),
            executable: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("stalker-save")),
        }
    } else {
        match UpdateInstallationDetector::detect(None, opts.target.as_deref(), None) {
            Ok(mut inst) => {
                if let Some(ref a) = opts.arch {
                    inst.architecture = a.clone();
                }
                if let Some(ref k) = opts.kind {
                    inst.kind = k.clone();
                }
                if let Some(ref r) = opts.root {
                    inst.root = r.clone();
                }
                inst
            }
            Err(e) => {
                eprintln!("Error detecting installation: {e}");
                return Err(ExitCode::Refused);
            }
        }
    };

    let mut service = UpdateService::new(current_ver, installation);
    if let Some(ref url) = opts.manifest_url {
        service = service.with_manifest_url(url.clone());
    }

    Ok(service)
}

fn run_check(args: &[String]) -> ExitCode {
    let opts = match parse_options(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let service = match build_service(&opts) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let mut fetch = DefaultFetch;
    let result = service.check(&mut fetch);

    match result.state {
        UpdateState::Current => {
            println!("Up to date: {}", service.current_version());
            ExitCode::Done
        }
        UpdateState::Available => {
            if let (Some(ref manifest), Some(ref artifact)) = (&result.manifest, &result.artifact) {
                println!(
                    "Available: {} ({}, {} bytes)",
                    manifest.version, artifact.file, artifact.size
                );
            } else {
                println!("Update available");
            }
            ExitCode::Done
        }
        UpdateState::DowngradeRefused => {
            let remote_version = result.manifest.as_ref().map_or("unknown", |m| m.version.as_str());
            eprintln!(
                "Refused: release manifest version ({remote_version}) is older than current version ({}) (downgrade protection)",
                service.current_version()
            );
            ExitCode::Refused
        }
        UpdateState::Invalid => {
            let err = result.error.unwrap_or_else(|| "Invalid release manifest".to_string());
            eprintln!("Error: {err}");
            ExitCode::Damaged
        }
        UpdateState::Unavailable => {
            let err = result.error.unwrap_or_else(|| "Update service unavailable".to_string());
            eprintln!("Error: {err}");
            ExitCode::System
        }
    }
}

/// Checks `latest.json` against its detached signature with the same verifier the updater uses.
///
/// Exit code 0 only for a valid signature; an empty or invalid signature is refused.
fn run_verify_manifest(args: &[String]) -> ExitCode {
    let [manifest, signature] = args else {
        eprintln!("Usage: stalker-save update verify-manifest MANIFEST SIGNATURE");
        return ExitCode::Usage;
    };
    let (Ok(manifest_bytes), Ok(signature_bytes)) = (std::fs::read(manifest), std::fs::read(signature)) else {
        eprintln!("Error: cannot read the manifest or its signature");
        return ExitCode::System;
    };
    if signature_bytes.iter().all(u8::is_ascii_whitespace) {
        eprintln!("Refused: the manifest signature is empty; the manifest is not signed");
        return ExitCode::Refused;
    }
    match sse_update::verify_signature(&manifest_bytes, &signature_bytes) {
        Ok(()) => {
            println!("Signature valid");
            ExitCode::Done
        }
        Err(error) => {
            eprintln!("Refused: manifest signature is invalid: {error}");
            ExitCode::Refused
        }
    }
}

fn run_download(args: &[String]) -> ExitCode {
    let opts = match parse_options(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let service = match build_service(&opts) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let mut fetch = DefaultFetch;
    let check_result = service.check(&mut fetch);

    let artifact = match check_result.artifact {
        Some(a) => a,
        None => {
            if let Some(err) = check_result.error {
                eprintln!("Error: {err}");
            } else {
                eprintln!("No update artifact available for the current installation.");
            }
            return match check_result.state {
                UpdateState::DowngradeRefused => ExitCode::Refused,
                UpdateState::Invalid => ExitCode::Damaged,
                _ => ExitCode::Usage,
            };
        }
    };

    let destination = if let Some(ref dest) = opts.destination {
        if dest.is_dir() {
            dest.join(&artifact.file)
        } else {
            dest.clone()
        }
    } else {
        Path::new(&artifact.file).to_path_buf()
    };

    println!("Downloading {} ({} bytes)...", artifact.file, artifact.size);

    match service.download(&mut fetch, &artifact, &destination, None) {
        Ok(saved_path) => {
            println!("Downloaded and verified: {}", saved_path.display());
            ExitCode::Done
        }
        Err(e) => {
            eprintln!("Download failed: {e}");
            ExitCode::System
        }
    }
}

#[cfg(test)]
mod verify_manifest_tests {
    use super::{run_verify_manifest, ExitCode};

    fn write_pair(name: &str, manifest: &[u8], signature: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!("sse-verify-manifest-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let manifest_path = directory.join("latest.json");
        let signature_path = directory.join("latest.json.sig");
        assert!(std::fs::write(&manifest_path, manifest).is_ok());
        assert!(std::fs::write(&signature_path, signature).is_ok());
        (manifest_path, signature_path)
    }

    #[test]
    fn empty_signature_is_refused_and_never_reported_valid() {
        let (manifest, signature) = write_pair("empty", b"{}\n", b"");
        let args = [manifest.display().to_string(), signature.display().to_string()];
        assert_eq!(run_verify_manifest(&args), ExitCode::Refused);
    }

    #[test]
    fn garbage_signature_is_refused() {
        let (manifest, signature) = write_pair("garbage", b"{}\n", b"not a signature");
        let args = [manifest.display().to_string(), signature.display().to_string()];
        assert_eq!(run_verify_manifest(&args), ExitCode::Refused);
    }

    #[test]
    fn wrong_argument_count_is_a_usage_error() {
        assert_eq!(run_verify_manifest(&["only-one".to_owned()]), ExitCode::Usage);
    }
}
