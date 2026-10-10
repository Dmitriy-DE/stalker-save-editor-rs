//! `latest.json` release manifest parser, semver comparison, and downgrade protection.

use crate::platform;
use core::cmp::Ordering;
use sse_core::{Error, Result};
use std::collections::BTreeMap;

/// Maximum manifest size in bytes (2 MiB).
pub const MAXIMUM_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

/// Maximum artifact size in bytes (2 GiB).
pub const MAXIMUM_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Update state comparing the current build version to the release manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Currently installed build matches or exceeds release (up to date).
    Current,
    /// A strictly newer release is available.
    Available,
    /// The release manifest carries an older version than current (downgrade refused).
    DowngradeRefused,
    /// Update service is unreachable or check is unavailable.
    Unavailable,
    /// Release manifest is malformed, untrusted, or invalid.
    Invalid,
}

/// A parsed pre-release identifier in semantic versioning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrereleasePart {
    /// Numeric identifier (no leading zeros permitted if length > 1).
    Numeric(u64),
    /// Alphanumeric identifier.
    Alpha(String),
}

/// A parsed Semantic Version 2.0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemVer {
    /// Major version number.
    pub major: u64,
    /// Minor version number.
    pub minor: u64,
    /// Patch version number.
    pub patch: u64,
    /// Pre-release identifier sequence.
    pub prerelease: Vec<PrereleasePart>,
}

impl SemVer {
    /// Parses a semver string with optional prerelease and build metadata.
    ///
    /// # Errors
    /// Returns an error if the version does not conform to strict semver.
    pub fn parse(text: &str) -> Result<Self> {
        let (version_and_pre, build) = match text.split_once('+') {
            Some((version, build)) => (version, Some(build)),
            None => (text, None),
        };
        if let Some(build) = build {
            if build.is_empty()
                || build.split('.').any(|part| {
                    part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            {
                return Err(Error::damaged("invalid build metadata"));
            }
        }
        // Build metadata is validated but deliberately omitted because SemVer ignores it in precedence.

        let (core, pre) = match version_and_pre.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (version_and_pre, None),
        };

        let mut parts = core.split('.');
        let major_str = parts.next().ok_or_else(|| Error::damaged("missing major version"))?;
        let minor_str = parts.next().ok_or_else(|| Error::damaged("missing minor version"))?;
        let patch_str = parts.next().ok_or_else(|| Error::damaged("missing patch version"))?;
        if parts.next().is_some() {
            return Err(Error::damaged("too many version components"));
        }

        let major = parse_version_number(major_str)?;
        let minor = parse_version_number(minor_str)?;
        let patch = parse_version_number(patch_str)?;

        let mut prerelease = Vec::new();
        if let Some(pre_text) = pre {
            if pre_text.is_empty() {
                return Err(Error::damaged("empty prerelease identifier"));
            }
            for part in pre_text.split('.') {
                if part.is_empty() {
                    return Err(Error::damaged("empty prerelease segment"));
                }
                if part.bytes().all(|b| b.is_ascii_digit()) {
                    if part.len() > 1 && part.starts_with('0') {
                        return Err(Error::damaged("numeric prerelease identifier has leading zero"));
                    }
                    let num = part
                        .parse::<u64>()
                        .map_err(|_| Error::damaged("invalid prerelease number"))?;
                    prerelease.push(PrereleasePart::Numeric(num));
                } else {
                    if !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                        return Err(Error::damaged("invalid characters in prerelease identifier"));
                    }
                    prerelease.push(PrereleasePart::Alpha(part.to_string()));
                }
            }
        }

        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    /// Compares two semantic versions.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // When major, minor, patch match:
        // A version with pre-release is LESS than a normal version with none.
        if self.prerelease.is_empty() && other.prerelease.is_empty() {
            return Ordering::Equal;
        }
        if self.prerelease.is_empty() {
            return Ordering::Greater;
        }
        if other.prerelease.is_empty() {
            return Ordering::Less;
        }

        let min_len = self.prerelease.len().min(other.prerelease.len());
        let mut i = 0_usize;
        while i < min_len {
            let left_part = self.prerelease.get(i);
            let right_part = other.prerelease.get(i);
            if let (Some(a), Some(b)) = (left_part, right_part) {
                let ord = match (a, b) {
                    (PrereleasePart::Numeric(n1), PrereleasePart::Numeric(n2)) => n1.cmp(n2),
                    (PrereleasePart::Numeric(_), PrereleasePart::Alpha(_)) => Ordering::Less,
                    (PrereleasePart::Alpha(_), PrereleasePart::Numeric(_)) => Ordering::Greater,
                    (PrereleasePart::Alpha(s1), PrereleasePart::Alpha(s2)) => s1.cmp(s2),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            i = i.saturating_add(1);
        }

        self.prerelease.len().cmp(&other.prerelease.len())
    }
}

fn parse_version_number(s: &str) -> Result<u64> {
    if s.is_empty() {
        return Err(Error::damaged("empty version number segment"));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(Error::damaged("version component contains leading zero"));
    }
    s.parse::<u64>().map_err(|_| Error::damaged("invalid version number"))
}

/// An update artifact entry describing a downloadable release package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateArtifact {
    /// Target manifest key / platform identifier (e.g. `linux-deb-amd64`).
    pub target: String,
    /// Architecture (e.g. `x86_64` or `arm64`).
    pub architecture: String,
    /// Package kind (e.g. `package`, `portable`, `installer`, `disk-image`).
    pub kind: String,
    /// Published filename without directories.
    pub file: String,
    /// Size in bytes.
    pub size: u64,
    /// Hex-encoded SHA-256 digest in lowercase.
    pub sha256: String,
    /// Download URL.
    pub url: String,
    /// Version of the release manifest this artifact came from; set when the artifact is
    /// selected from a verified manifest. Empty for artifacts that were not selected from one.
    pub release_version: String,
}

impl UpdateArtifact {
    /// Validates all metadata fields according to security and naming rules.
    ///
    /// # Errors
    /// Returns an error if any field is invalid or out of specification.
    pub fn validate(&self) -> Result<()> {
        if self.size == 0 || self.size > MAXIMUM_ARTIFACT_BYTES {
            return Err(Error::damaged("artifact.size must be > 0 and <= 2 GiB"));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::damaged("artifact.sha256 must be a lowercase 64-hex string"));
        }
        if self.file.is_empty()
            || self.file == "."
            || self.file == ".."
            || self.file.contains('/')
            || self.file.contains('\\')
            || self.file.contains(':')
        {
            return Err(Error::damaged("artifact.file contains invalid filename characters"));
        }
        if !platform::describes(&self.target, &self.architecture, &self.kind) {
            return Err(Error::damaged("artifact does not match registered platform matrix"));
        }
        let expected_suffix = format!("/{}", self.file);
        if !self.url.ends_with(&expected_suffix) {
            return Err(Error::damaged("artifact URL path does not match file name"));
        }
        Ok(())
    }
}

/// The parsed `latest.json` release manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateManifest {
    /// Manifest schema version (must be 1).
    pub schema: u32,
    /// Release channel (must be `stable`).
    pub channel: String,
    /// Release version string.
    pub version: String,
    /// Source git commit SHA.
    pub source_commit: String,
    /// Publication timestamp.
    pub published_at: String,
    /// Release artifacts indexed by target key.
    pub artifacts: BTreeMap<String, UpdateArtifact>,
    /// Optional platform artifacts.
    pub optional_artifacts: BTreeMap<String, UpdateArtifact>,
}

impl UpdateManifest {
    /// Parses and validates a `latest.json` manifest payload.
    ///
    /// # Errors
    /// Returns an error if JSON is malformed, schema != 1, required artifacts missing, or fields invalid.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAXIMUM_MANIFEST_BYTES {
            return Err(Error::Refused(
                "Manifest payload exceeds maximum permitted size".to_string(),
            ));
        }

        let mut reader = sse_codecs::json::Reader::new(bytes);
        match reader.next_event()? {
            Some(sse_codecs::json::Event::ObjectStart) => {}
            _ => return Err(Error::damaged("Expected JSON object at manifest root")),
        }

        let mut schema: Option<u32> = None;
        let mut channel: Option<String> = None;
        let mut version: Option<String> = None;
        let mut source_commit: Option<String> = None;
        let mut published_at: Option<String> = None;
        let mut artifacts: BTreeMap<String, UpdateArtifact> = BTreeMap::new();
        let mut optional_artifacts: BTreeMap<String, UpdateArtifact> = BTreeMap::new();

        while let Some(event) = reader.next_event()? {
            match event {
                sse_codecs::json::Event::Key(k) => match k.as_str() {
                    "schema" => {
                        if let Some(sse_codecs::json::Event::Number(n)) = reader.next_event()? {
                            schema = Some(n.parse::<u32>().map_err(|_| Error::damaged("invalid schema"))?);
                        } else {
                            return Err(Error::damaged("schema must be a number"));
                        }
                    }
                    "channel" => {
                        if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                            channel = Some(s.into_owned());
                        } else {
                            return Err(Error::damaged("channel must be a string"));
                        }
                    }
                    "version" => {
                        if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                            version = Some(s.into_owned());
                        } else {
                            return Err(Error::damaged("version must be a string"));
                        }
                    }
                    "source_commit" => {
                        if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                            source_commit = Some(s.into_owned());
                        } else {
                            return Err(Error::damaged("source_commit must be a string"));
                        }
                    }
                    "published_at" => {
                        if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                            published_at = Some(s.into_owned());
                        } else {
                            return Err(Error::damaged("published_at must be a string"));
                        }
                    }
                    "artifacts" => {
                        artifacts = parse_artifacts_object(&mut reader)?;
                    }
                    "optional_artifacts" => {
                        optional_artifacts = parse_artifacts_object(&mut reader)?;
                    }
                    _ => {
                        reader.skip_value()?;
                    }
                },
                sse_codecs::json::Event::ObjectEnd => break,
                _ => return Err(Error::damaged("Unexpected token in manifest root")),
            }
        }

        let schema = schema.ok_or_else(|| Error::damaged("missing schema"))?;
        if schema != 1 {
            return Err(Error::Refused("unsupported update manifest schema".to_string()));
        }
        let channel = channel.ok_or_else(|| Error::damaged("missing channel"))?;
        if channel != "stable" {
            return Err(Error::Refused("unsupported update release channel".to_string()));
        }
        let version = version.ok_or_else(|| Error::damaged("missing version"))?;
        let _ = SemVer::parse(&version)?;

        let source_commit = source_commit.ok_or_else(|| Error::damaged("missing source_commit"))?;
        if source_commit.len() < 40
            || source_commit.len() > 64
            || !source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::damaged("source_commit must be a lowercase git hex SHA"));
        }

        let published_at = published_at.ok_or_else(|| Error::damaged("missing published_at"))?;

        // Validate required artifacts
        for required_key in &["windows-x86_64", "linux-x86_64", "linux-deb-amd64"] {
            let art = artifacts
                .get(*required_key)
                .ok_or_else(|| Error::damaged("missing required artifact"))?;
            if art.target != *required_key {
                return Err(Error::damaged("artifact target does not match manifest key"));
            }
        }

        Ok(Self {
            schema,
            channel,
            version,
            source_commit,
            published_at,
            artifacts,
            optional_artifacts,
        })
    }

    /// Selects the appropriate update artifact for a given target, architecture, and kind.
    ///
    /// # Errors
    /// Returns an error if no matching artifact exists in this release.
    pub fn select(&self, target_name: &str, arch: &str, kind_name: &str) -> Result<&UpdateArtifact> {
        let key = platform::artifact_key(target_name, arch, kind_name)
            .ok_or_else(|| Error::Refused("No update artifact registered for platform".to_string()))?;

        if let Some(art) = self.artifacts.get(key) {
            if art.architecture == arch && art.kind == kind_name {
                return Ok(art);
            }
        }

        if let Some(art) = self.optional_artifacts.get(key) {
            if art.architecture == arch && art.kind == kind_name {
                return Ok(art);
            }
        }

        Err(Error::Refused("No matching update artifact in manifest".to_string()))
    }
}

fn parse_artifacts_object(reader: &mut sse_codecs::json::Reader<'_>) -> Result<BTreeMap<String, UpdateArtifact>> {
    match reader.next_event()? {
        Some(sse_codecs::json::Event::ObjectStart) => {}
        _ => return Err(Error::damaged("Expected object start for artifacts")),
    }

    let mut map = BTreeMap::new();
    while let Some(event) = reader.next_event()? {
        match event {
            sse_codecs::json::Event::Key(k) => {
                let key_name = k.into_owned();
                let art = parse_single_artifact(reader)?;
                art.validate()?;
                if art.target != key_name {
                    return Err(Error::damaged("artifact target does not match manifest key"));
                }
                map.insert(key_name, art);
            }
            sse_codecs::json::Event::ObjectEnd => break,
            _ => return Err(Error::damaged("Unexpected token in artifacts object")),
        }
    }
    Ok(map)
}

fn parse_single_artifact(reader: &mut sse_codecs::json::Reader<'_>) -> Result<UpdateArtifact> {
    match reader.next_event()? {
        Some(sse_codecs::json::Event::ObjectStart) => {}
        _ => return Err(Error::damaged("Expected object start for single artifact")),
    }

    let mut target: Option<String> = None;
    let mut architecture: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut file: Option<String> = None;
    let mut size: Option<u64> = None;
    let mut sha256: Option<String> = None;
    let mut url: Option<String> = None;

    while let Some(event) = reader.next_event()? {
        match event {
            sse_codecs::json::Event::Key(k) => match k.as_str() {
                "target" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        target = Some(s.into_owned());
                    }
                }
                "architecture" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        architecture = Some(s.into_owned());
                    }
                }
                "kind" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        kind = Some(s.into_owned());
                    }
                }
                "file" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        file = Some(s.into_owned());
                    }
                }
                "size" => {
                    if let Some(sse_codecs::json::Event::Number(n)) = reader.next_event()? {
                        size = Some(n.parse::<u64>().map_err(|_| Error::damaged("invalid size"))?);
                    }
                }
                "sha256" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        sha256 = Some(s.into_owned());
                    }
                }
                "url" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        url = Some(s.into_owned());
                    }
                }
                _ => {
                    reader.skip_value()?;
                }
            },
            sse_codecs::json::Event::ObjectEnd => break,
            _ => return Err(Error::damaged("Unexpected token in artifact object")),
        }
    }

    Ok(UpdateArtifact {
        target: target.ok_or_else(|| Error::damaged("missing target in artifact"))?,
        architecture: architecture.ok_or_else(|| Error::damaged("missing architecture in artifact"))?,
        kind: kind.ok_or_else(|| Error::damaged("missing kind in artifact"))?,
        file: file.ok_or_else(|| Error::damaged("missing file in artifact"))?,
        size: size.ok_or_else(|| Error::damaged("missing size in artifact"))?,
        sha256: sha256.ok_or_else(|| Error::damaged("missing sha256 in artifact"))?,
        url: url.ok_or_else(|| Error::damaged("missing url in artifact"))?,
        release_version: String::new(),
    })
}

/// Compares the installed version with the release manifest version, enforcing downgrade protection.
///
/// # Errors
/// Returns an error if either version string cannot be parsed as semver.
pub fn compare_versions(current_version: &str, manifest_version: &str) -> Result<UpdateState> {
    let current = SemVer::parse(current_version)?;
    let released = SemVer::parse(manifest_version)?;

    match released.compare(&current) {
        Ordering::Greater => Ok(UpdateState::Available),
        Ordering::Equal => Ok(UpdateState::Current),
        Ordering::Less => Ok(UpdateState::DowngradeRefused),
    }
}

#[cfg(test)]
mod tests {
    use super::UpdateManifest;

    const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn artifact(target: &str, kind: &str) -> String {
        format!(
            r#"{{"target":"{target}","architecture":"x86_64","kind":"{kind}","file":"SaveEditor.zip","size":10,"sha256":"{SHA256}","url":"https://updates.test/SaveEditor.zip"}}"#
        )
    }

    fn manifest_with_optional(key: &str, target: &str, kind: &str) -> String {
        format!(
            r#"{{"schema":1,"channel":"stable","version":"1.0.0","source_commit":"{COMMIT}","published_at":"2026-01-01T00:00:00Z","artifacts":{{"windows-x86_64":{w},"linux-x86_64":{l},"linux-deb-amd64":{d}}},"optional_artifacts":{{"{key}":{o}}}}}"#,
            w = artifact("windows-x86_64", "portable"),
            l = artifact("linux-x86_64", "portable"),
            d = artifact("linux-deb-amd64", "package"),
            o = artifact(target, kind),
        )
    }

    #[test]
    fn optional_artifact_whose_target_matches_its_key_parses() {
        let text = manifest_with_optional("windows-installer-x86_64", "windows-installer-x86_64", "installer");
        assert!(UpdateManifest::parse(text.as_bytes()).is_ok());
    }

    #[test]
    fn optional_artifact_whose_target_differs_from_its_key_is_rejected() {
        let text = manifest_with_optional("windows-installer-x86_64", "linux-x86_64", "portable");
        assert!(UpdateManifest::parse(text.as_bytes()).is_err());
    }
}
