//! Downloads whole-file overlay content that the content store does not hold yet.
//!
//! Only the explicit install of a fix starts a download. The address is built from the content
//! hash, so the catalogue needs no extra field. The body is accepted only when its SHA-256 equals
//! the catalogued hash; then it is written atomically into the content store.

use std::path::Path;

use sse_codecs::sha256::sha256_hex;
use sse_core::{Error, Result};
use sse_sys::fetch::{Fetch, Response, SystemFetch};

use crate::models::GameFixDefinition;
use crate::store::GameFixContentStore;

/// Base address of the fix-asset files on the official download server.
pub const FIX_ASSET_BASE_URL: &str = "https://save-editor-downloads.save-editor.workers.dev/fix-assets/";

/// Largest accepted fix file. The biggest shipped overlay is far smaller.
pub const MAXIMUM_FIX_ASSET_BYTES: u64 = 8 * 1024 * 1024;

/// Official host; the same host as the update service.
const DOWNLOAD_HOST: &str = "save-editor-downloads.save-editor.workers.dev";

/// Address of the fix file with the given content SHA-256.
#[must_use]
pub fn fix_asset_url(content_sha256: &str) -> String {
    format!("{FIX_ASSET_BASE_URL}{content_sha256}")
}

/// Fetch configuration for fix files: HTTPS transfers with a size limit and no whole-transfer
/// timeout, as for update artifacts; connect and idle timeouts still apply.
#[must_use]
pub fn fix_asset_fetch_config() -> SystemFetch {
    SystemFetch {
        max_bytes: MAXIMUM_FIX_ASSET_BYTES,
        total_timeout: None,
        ..SystemFetch::default()
    }
}

fn is_official_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let Some(authority) = rest.get(..authority_end) else {
        return false;
    };
    !authority.contains('@')
        && (authority.eq_ignore_ascii_case(DOWNLOAD_HOST)
            || authority.eq_ignore_ascii_case(&format!("{DOWNLOAD_HOST}:443")))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Downloads one fix file and checks it against `content_sha256` before returning it.
///
/// # Errors
/// Refuses a non-HTTPS or foreign address, a redirect, a failed status, a body over the size limit,
/// and a body whose SHA-256 differs. Network failures are returned as they are.
pub fn download_fix_asset(fetch: &mut dyn Fetch, content_sha256: &str) -> Result<Vec<u8>> {
    if !is_sha256_hex(content_sha256) {
        return Err(Error::damaged("Fix file address is not a SHA-256 hash"));
    }
    let url = fix_asset_url(content_sha256);
    if !is_official_https_url(&url) {
        return Err(Error::Refused(
            "Fix files can only be downloaded over HTTPS from the official server".to_owned(),
        ));
    }

    let mut body = Vec::new();
    let mut too_large = false;
    let mut on_response = |response: &Response| response.status == 200 && response.final_url == url;
    let mut sink = |chunk: &[u8]| {
        let total = u64::try_from(body.len().saturating_add(chunk.len())).unwrap_or(u64::MAX);
        if total > MAXIMUM_FIX_ASSET_BYTES {
            too_large = true;
            return false;
        }
        body.extend_from_slice(chunk);
        true
    };
    let response = fetch.get_with_response(&url, 0, &mut on_response, &mut sink)?;

    if response.final_url != url {
        return Err(Error::Refused(
            "Fix file download was redirected to another address; install cancelled".to_owned(),
        ));
    }
    if too_large {
        return Err(Error::damaged(
            "Fix file is larger than the allowed size; install cancelled",
        ));
    }
    if !sha256_hex(&body).eq_ignore_ascii_case(content_sha256) {
        return Err(Error::damaged(
            "Downloaded fix file does not match the catalogue SHA-256; install cancelled",
        ));
    }
    Ok(body)
}

/// Stores the files of `definition` that the default content store lacks.
///
/// # Errors
/// See [`download_fix_asset`]; nothing is written when a download fails.
pub fn fetch_missing_overlays(definition: &GameFixDefinition, fetch: &mut dyn Fetch) -> Result<()> {
    fetch_missing_overlays_into(&GameFixContentStore::default_directory(), definition, fetch)
}

/// Same as [`fetch_missing_overlays`], into an explicit store directory.
///
/// # Errors
/// See [`download_fix_asset`].
pub fn fetch_missing_overlays_into(
    store_dir: &Path,
    definition: &GameFixDefinition,
    fetch: &mut dyn Fetch,
) -> Result<()> {
    for overlay in &definition.overlays {
        if GameFixContentStore::contains(store_dir, &overlay.content_sha256) {
            continue;
        }
        let bytes = download_fix_asset(fetch, &overlay.content_sha256)?;
        std::fs::create_dir_all(store_dir).map_err(|error| Error::System(error.to_string()))?;
        GameFixContentStore::add(store_dir, &bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Serves canned responses; `final_url` lets a test simulate a redirect.
    #[derive(Default)]
    struct FakeFetch {
        routes: HashMap<String, (u16, String, Vec<u8>)>,
        requests: RefCell<Vec<String>>,
    }

    impl Fetch for FakeFetch {
        fn get(&mut self, url: &str, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
            let mut ignore = |_: &Response| true;
            self.get_with_response(url, range_from, &mut ignore, sink)
        }

        fn get_with_response(
            &mut self,
            url: &str,
            _range_from: u64,
            on_response: &mut dyn FnMut(&Response) -> bool,
            sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> Result<Response> {
            self.requests.borrow_mut().push(url.to_owned());
            let (status, final_url, body) = self
                .routes
                .get(url)
                .cloned()
                .ok_or_else(|| Error::Refused(format!("404 Not Found: {url}")))?;
            let response = Response {
                status,
                content_length: u64::try_from(body.len()).ok(),
                content_range: None,
                final_url,
            };
            if !on_response(&response) {
                return Err(Error::Refused("response refused".to_owned()));
            }
            for chunk in body.chunks(64 * 1024) {
                if !sink(chunk) {
                    break;
                }
            }
            Ok(response)
        }
    }

    fn temp_store(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sse-fix-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn definition_with(content_sha256: &str) -> Result<GameFixDefinition> {
        let mut definition = crate::catalog::GameFixCatalog::try_get("cs.ai.limansk-bridge-model")
            .ok_or_else(|| Error::damaged("fix is not catalogued"))?
            .clone();
        let overlay = definition
            .overlays
            .first_mut()
            .ok_or_else(|| Error::damaged("fix has no overlay"))?;
        overlay.content_sha256 = content_sha256.to_owned();
        Ok(definition)
    }

    #[test]
    fn verified_download_is_stored_under_its_hash() -> Result<()> {
        let body = b"srp bridge model".to_vec();
        let sha = sha256_hex(&body);
        let url = fix_asset_url(&sha);
        let mut fetch = FakeFetch::default();
        fetch.routes.insert(url.clone(), (200, url.clone(), body.clone()));
        let store = temp_store("ok");
        let definition = definition_with(&sha)?;

        fetch_missing_overlays_into(&store, &definition, &mut fetch)?;

        assert_eq!(GameFixContentStore::read(&store, &sha), Some(body));
        assert_eq!(fetch.requests.borrow().as_slice(), std::slice::from_ref(&url));
        // A stored file is not downloaded again.
        fetch_missing_overlays_into(&store, &definition, &mut fetch)?;
        assert_eq!(fetch.requests.borrow().len(), 1);
        Ok(())
    }

    #[test]
    fn wrong_hash_is_refused_and_nothing_is_stored() -> Result<()> {
        let sha = sha256_hex(b"expected");
        let url = fix_asset_url(&sha);
        let mut fetch = FakeFetch::default();
        fetch.routes.insert(url.clone(), (200, url, b"tampered".to_vec()));
        let store = temp_store("hash");

        let result = fetch_missing_overlays_into(&store, &definition_with(&sha)?, &mut fetch);

        assert!(result.is_err());
        assert!(!GameFixContentStore::contains(&store, &sha));
        Ok(())
    }

    #[test]
    fn body_over_the_size_limit_is_refused() {
        let sha = sha256_hex(b"any");
        let url = fix_asset_url(&sha);
        let big = vec![0_u8; usize::try_from(MAXIMUM_FIX_ASSET_BYTES).unwrap_or(0) + 1];
        let mut fetch = FakeFetch::default();
        fetch.routes.insert(url.clone(), (200, url, big));
        let store = temp_store("big");

        let result = download_fix_asset(&mut fetch, &sha);

        assert!(result.is_err());
        assert!(!GameFixContentStore::contains(&store, &sha));
    }

    #[test]
    fn redirect_to_another_address_is_refused() {
        let sha = sha256_hex(b"redirect body");
        let url = fix_asset_url(&sha);
        let mut fetch = FakeFetch::default();
        fetch.routes.insert(
            url.clone(),
            (200, "https://evil.test/file".to_owned(), b"redirect body".to_vec()),
        );

        let result = download_fix_asset(&mut fetch, &sha);

        assert!(result.is_err());
    }

    #[test]
    fn non_200_status_is_refused() {
        let sha = sha256_hex(b"missing");
        let url = fix_asset_url(&sha);
        let mut fetch = FakeFetch::default();
        fetch.routes.insert(url.clone(), (404, url, Vec::new()));

        assert!(download_fix_asset(&mut fetch, &sha).is_err());
    }

    /// Manual run: downloads the real bridge model from the official server and checks its hash.
    /// `cargo test -p sse-fixes --lib real_fix_asset_download -- --ignored --nocapture`
    #[test]
    #[ignore = "manual run against the live download server"]
    fn real_fix_asset_download_matches_the_catalogue_hash() -> Result<()> {
        let sha = "0986c216d1549ca8de1a377a3b20368dc4a5428d3d563abab8a5588f1f57e78a";
        let mut fetch = fix_asset_fetch_config();
        let bytes = download_fix_asset(&mut fetch, sha)?;
        println!("downloaded {} bytes, sha256 {}", bytes.len(), sha256_hex(&bytes));
        assert_eq!(sha256_hex(&bytes), sha);
        Ok(())
    }

    #[test]
    fn addresses_outside_the_official_https_host_are_refused() {
        assert!(is_official_https_url(&fix_asset_url(&"a".repeat(64))));
        assert!(!is_official_https_url(
            "http://save-editor-downloads.save-editor.workers.dev/fix-assets/x"
        ));
        assert!(!is_official_https_url("https://evil.test/fix-assets/x"));
        assert!(!is_official_https_url(
            "https://save-editor-downloads.save-editor.workers.dev@evil.test/x"
        ));
        assert!(!is_sha256_hex("not-a-hash"));
    }
}
