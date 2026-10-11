//! Item icons for the inventory, stashes, the add list and the encyclopedia (port of the C# `ItemIconService`).
//!
//! Source order: the installed game's own icon (when its content was read), otherwise the pack shipped with the
//! editor, looked up through the alias table. Missing icons are remembered as missing. PNG decoding runs on a
//! worker thread; a screen draws its placeholder until the icon is ready and then gets an [`IconsReady`] message.
//!
//! The browser build has no worker threads and does not carry the pack: the host delivers it later with
//! [`install_pack`] and turns on [`set_inline_decode`], so an icon is decoded where it is asked for. Until the
//! pack arrives (or when it never does) every item draws the same empty frame as an item without an icon.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use sse_catalog::{GameContent, Stalker2ItemCatalog};
use sse_core::{Error, Result};

use crate::event_loop::Proxy;
use crate::item_icon_pack::ALIASES;
#[cfg(feature = "embedded-item-icons")]
use crate::item_icon_pack::PACK;
use crate::screens::{AppMessage, ScreenId};
use crate::widget::ImageData;

/// Icons kept decoded at once; the least recently stored one is dropped past this.
pub const CACHE_LIMIT: usize = 1024;

/// Sent to a screen when icons finished decoding: the screen draws its rows again.
#[derive(Debug)]
pub struct IconsReady;

#[derive(Clone)]
enum Entry {
    Ready(ImageData),
    Missing,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(String, String), Entry>,
    order: VecDeque<(String, String)>,
    pending: HashSet<(String, String)>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Installed game content by release family (`stalker-soc`, ...), as read by the screens that load it.
fn installed() -> &'static Mutex<HashMap<String, Arc<GameContent>>> {
    static INSTALLED: OnceLock<Mutex<HashMap<String, Arc<GameContent>>>> = OnceLock::new();
    INSTALLED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Release identifier without the Enhanced Edition suffix: `stalker-soc-ee` → `stalker-soc`.
#[must_use]
pub fn family(release: &str) -> &str {
    release.strip_suffix("-ee").unwrap_or(release)
}

/// Keeps the installed content of a release so its icons can be read. A later content for the same family replaces it.
pub fn remember_installed(release: &str, content: Arc<GameContent>) {
    if let Ok(mut map) = installed().lock() {
        map.insert(family(release).to_owned(), content);
    }
}

fn aliases() -> &'static HashMap<String, String> {
    static ALIAS_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    ALIAS_MAP.get_or_init(|| {
        let mut map = HashMap::new();
        if let Ok(value) = sse_catalog::parse_json(ALIASES) {
            for (from, to) in value.as_object().unwrap_or_default() {
                if let Some(to) = to.as_str() {
                    map.insert(from.clone(), to.to_owned());
                }
            }
        }
        map
    })
}

#[cfg(feature = "embedded-item-icons")]
fn pack_entries() -> &'static HashMap<&'static str, &'static [u8]> {
    static PACK_MAP: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    PACK_MAP.get_or_init(|| PACK.iter().map(|(path, bytes)| (*path, *bytes)).collect())
}

/// First bytes of an icon pack file.
pub const PACK_MAGIC: &[u8; 8] = b"SSEICON1";
/// Largest icon pack file accepted by [`install_pack`].
pub const MAX_PACK_BYTES: usize = 64 * 1024 * 1024;
/// Most pictures an icon pack file may hold.
pub const MAX_PACK_ENTRIES: usize = 65_536;

/// An icon pack delivered as a file: its bytes and where each picture lies in them.
struct LoadedPack {
    bytes: Vec<u8>,
    index: HashMap<String, Range<usize>>,
}

fn loaded_pack() -> &'static Mutex<Option<Arc<LoadedPack>>> {
    static LOADED: OnceLock<Mutex<Option<Arc<LoadedPack>>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(None))
}

static INLINE_DECODE: AtomicBool = AtomicBool::new(false);

/// Whether the pictures are compiled into this build (every desktop build; not the browser one).
#[must_use]
pub const fn pack_embedded() -> bool {
    cfg!(feature = "embedded-item-icons")
}

/// Decodes an icon where it is asked for when the screen has no worker proxy. For hosts without worker threads
/// (the browser); a desktop shell keeps decoding on its worker thread.
pub fn set_inline_decode(enabled: bool) {
    INLINE_DECODE.store(enabled, Ordering::Relaxed);
}

/// Writes an icon pack file: the magic, a little-endian `u32` count, then per picture a `u16` path length, the
/// path, a `u32` byte length and the PNG bytes. Pictures are written in path order.
///
/// # Errors
/// Refuses a path or a picture too long for its length field, and more pictures than [`MAX_PACK_ENTRIES`].
pub fn encode_pack(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let refused = |what: &str| Error::Refused(format!("item icon pack: {what}"));
    if entries.len() > MAX_PACK_ENTRIES {
        return Err(refused("too many pictures"));
    }
    let mut sorted: Vec<&(&str, &[u8])> = entries.iter().collect();
    sorted.sort_by_key(|(path, _)| *path);
    let mut out = PACK_MAGIC.to_vec();
    let count = u32::try_from(sorted.len()).map_err(|_| refused("too many pictures"))?;
    out.extend_from_slice(&count.to_le_bytes());
    for (path, bytes) in sorted {
        let path_len = u16::try_from(path.len()).map_err(|_| refused("a path is too long"))?;
        let byte_len = u32::try_from(bytes.len()).map_err(|_| refused("a picture is too large"))?;
        out.extend_from_slice(&path_len.to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&byte_len.to_le_bytes());
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

fn parse_pack(bytes: &[u8]) -> Result<HashMap<String, Range<usize>>> {
    let refused = |what: &str| Error::Refused(format!("item icon pack: {what}"));
    if bytes.len() > MAX_PACK_BYTES {
        return Err(refused("the file is too large"));
    }
    let take = |at: usize, len: usize| -> Result<(&[u8], usize)> {
        let end = at.checked_add(len).ok_or_else(|| refused("a length overflows"))?;
        let part = bytes.get(at..end).ok_or_else(|| refused("the file is cut short"))?;
        Ok((part, end))
    };
    let (magic, at) = take(0, PACK_MAGIC.len())?;
    if magic != PACK_MAGIC {
        return Err(refused("not an icon pack file"));
    }
    let (count, mut at) = take(at, 4)?;
    let count = usize::try_from(u32::from_le_bytes(
        count.try_into().map_err(|_| refused("the file is cut short"))?,
    ))
    .map_err(|_| refused("too many pictures"))?;
    if count > MAX_PACK_ENTRIES {
        return Err(refused("too many pictures"));
    }
    let mut index = HashMap::with_capacity(count);
    for _ in 0..count {
        let (path_len, next) = take(at, 2)?;
        let path_len = usize::from(u16::from_le_bytes(
            path_len.try_into().map_err(|_| refused("the file is cut short"))?,
        ));
        let (path, next) = take(next, path_len)?;
        let path = std::str::from_utf8(path).map_err(|_| refused("a path is not text"))?;
        let (byte_len, next) = take(next, 4)?;
        let byte_len = usize::try_from(u32::from_le_bytes(
            byte_len.try_into().map_err(|_| refused("the file is cut short"))?,
        ))
        .map_err(|_| refused("a picture is too large"))?;
        let (_, end) = take(next, byte_len)?;
        if index.insert(path.to_owned(), next..end).is_some() {
            return Err(refused("a path is listed twice"));
        }
        at = end;
    }
    if at != bytes.len() {
        return Err(refused("unexpected bytes after the last picture"));
    }
    Ok(index)
}

/// Takes an icon pack file delivered at run time and returns how many pictures it holds. Icons that were
/// answered as missing before are asked for again. The pack replaces an earlier delivered one and is looked up
/// before the compiled-in pictures.
///
/// # Errors
/// Refuses a file that is not a well-formed icon pack; the icons known so far are left as they were.
pub fn install_pack(bytes: Vec<u8>) -> Result<usize> {
    let index = parse_pack(&bytes)?;
    let count = index.len();
    let poisoned = |_| Error::Refused("item icon pack: the icon store is unavailable".to_owned());
    *loaded_pack().lock().map_err(poisoned)? = Some(Arc::new(LoadedPack { bytes, index }));
    if let Ok(mut guard) = cache().lock() {
        let Cache { entries, order, .. } = &mut *guard;
        entries.retain(|_, entry| matches!(entry, Entry::Ready(_)));
        order.retain(|slot| entries.contains_key(slot));
    }
    Ok(count)
}

/// PNG bytes of a pack picture: the delivered pack first, then the compiled-in one.
fn pack_png(path: &str) -> Option<Vec<u8>> {
    let loaded = loaded_pack().lock().ok().and_then(|guard| guard.clone());
    if let Some(bytes) = loaded.and_then(|pack| pack.bytes.get(pack.index.get(path)?.clone()).map(<[u8]>::to_vec)) {
        return Some(bytes);
    }
    #[cfg(feature = "embedded-item-icons")]
    {
        pack_entries().get(path).map(|bytes| bytes.to_vec())
    }
    #[cfg(not(feature = "embedded-item-icons"))]
    {
        None
    }
}

/// Whether an icon can be answered now: the pictures are compiled in or were delivered, or the game's own are read.
fn source_available(content: Option<&GameContent>) -> bool {
    content.is_some() || pack_embedded() || loaded_pack().lock().is_ok_and(|guard| guard.is_some())
}

fn store(slot: (String, String), entry: Entry) {
    if let Ok(mut guard) = cache().lock() {
        guard.pending.remove(&slot);
        if guard.entries.insert(slot.clone(), entry).is_none() {
            guard.order.push_back(slot);
        }
        while guard.order.len() > CACHE_LIMIT {
            if let Some(oldest) = guard.order.pop_front() {
                guard.entries.remove(&oldest);
            }
        }
    }
}

/// Relative icon path of an item (`xray/<key>.png`, or the S2 catalog entry), with shared pictures resolved.
#[must_use]
pub fn icon_path(release: &str, key: &str) -> Option<String> {
    if key.trim().is_empty() {
        return None;
    }
    let raw = if family(release).starts_with("stalker2") {
        Stalker2ItemCatalog::load_embedded().icon(Some(key))?.to_owned()
    } else {
        format!("xray/{key}.png")
    };
    Some(aliases().get(&raw).cloned().unwrap_or(raw))
}

/// The decoded icon when it is already known. Never decodes: a miss is answered with `None`.
#[must_use]
pub fn cached(release: &str, key: &str) -> Option<ImageData> {
    let guard = cache().lock().ok()?;
    match guard.entries.get(&(family(release).to_owned(), key.to_owned())) {
        Some(Entry::Ready(image)) => Some(image.clone()),
        _ => None,
    }
}

/// Starts decoding the icon of `key` on a worker thread unless it is known or already being decoded.
/// When done, `screen` receives [`IconsReady`] so it can draw the icon. Without a `proxy` nothing is started,
/// unless [`set_inline_decode`] is on: then the icon is decoded before this returns.
pub fn request(release: &str, key: &str, proxy: Option<&Proxy<AppMessage>>, screen: ScreenId) {
    if key.trim().is_empty() {
        return;
    }
    let family = family(release).to_owned();
    let slot = (family.clone(), key.to_owned());
    {
        let Ok(mut guard) = cache().lock() else { return };
        if guard.entries.contains_key(&slot) || guard.pending.contains(&slot) {
            return;
        }
        guard.pending.insert(slot.clone());
    }
    let content = installed().lock().ok().and_then(|map| map.get(&family).cloned());
    let path = icon_path(release, key);
    let Some(proxy) = proxy.cloned() else {
        // Without a worker the icon is decoded here when the host asked for that. An icon whose source has not
        // arrived yet is left unknown, so it is asked for again later instead of being remembered as missing.
        if INLINE_DECODE.load(Ordering::Relaxed) && source_available(content.as_deref()) {
            let entry = decode(content.as_deref(), path.as_deref(), key).map_or(Entry::Missing, Entry::Ready);
            store(slot, entry);
        } else if let Ok(mut guard) = cache().lock() {
            guard.pending.remove(&slot);
        }
        return;
    };
    let key = key.to_owned();
    sse_app::tasks::spawn_named_detached("item-icon", move || {
        let entry = decode(content.as_deref(), path.as_deref(), &key).map_or(Entry::Missing, Entry::Ready);
        store(slot, entry);
        proxy.send(AppMessage::ToScreen(screen, Box::new(IconsReady)));
    });
}

/// RGBA bytes to premultiplied `0xAARRGGBB` pixels (the layout `ImageData` expects).
fn premultiplied_argb(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u32>> {
    let count = usize::try_from(width.checked_mul(height)?).ok()?;
    if rgba.len() != count.checked_mul(4)? {
        return None;
    }
    let scale = |channel: u8, alpha: u32| {
        u32::from(channel)
            .saturating_mul(alpha)
            .saturating_add(127)
            .checked_div(255)
            .unwrap_or(0)
    };
    Some(
        rgba.chunks_exact(4)
            .filter_map(|px| match *px {
                [r, g, b, a] => {
                    let alpha = u32::from(a);
                    Some((alpha << 24) | (scale(r, alpha) << 16) | (scale(g, alpha) << 8) | scale(b, alpha))
                }
                _ => None,
            })
            .collect(),
    )
}

/// PNG bytes from the installed game first, then from the shipped pack; decoded to premultiplied pixels.
fn decode(content: Option<&GameContent>, path: Option<&str>, key: &str) -> Option<ImageData> {
    let png: Vec<u8> = match content.and_then(|content| content.icon_png(key)) {
        Some(bytes) => bytes,
        None => pack_png(path?)?,
    };
    let image = sse_codecs::png::decode(&png).ok()?;
    let pixels = premultiplied_argb(image.width, image.height, &image.pixels)?;
    ImageData::new(image.width, image.height, pixels).ok()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn edition_suffix_maps_to_the_base_release() {
        assert_eq!(family("stalker-soc-ee"), "stalker-soc");
        assert_eq!(family("stalker-cop"), "stalker-cop");
    }

    #[test]
    fn xray_icons_resolve_through_the_alias_table() {
        // `wpn_knife` shares the picture of `mp_wpn_knife` in the shipped pack (as in the C# desktop app).
        assert_eq!(
            icon_path("stalker-cs", "wpn_knife").as_deref(),
            Some("xray/mp_wpn_knife.png")
        );
        let alias_from = "s2/AArtifactWeirdFlower_.png";
        assert_eq!(
            aliases().get(alias_from).map(String::as_str),
            Some("s2/AArtifactWeirdFlower.png")
        );
    }

    #[cfg(feature = "embedded-item-icons")]
    #[test]
    fn shipped_pack_icon_decodes_to_pixels() {
        let image = decode(None, Some("xray/mp_wpn_knife.png"), "mp_wpn_knife").expect("pack icon decodes");
        assert!(image.width > 0 && image.height > 0);
        assert_eq!(image.pixels.len(), usize::try_from(image.width * image.height).unwrap());
    }

    #[test]
    fn pack_file_round_trips_in_path_order_and_refuses_damage() {
        let pack = encode_pack(&[("xray/b.png", b"bb".as_slice()), ("xray/a.png", b"a".as_slice())]).expect("encodes");
        assert!(pack.starts_with(PACK_MAGIC));
        let index = parse_pack(&pack).expect("parses");
        let first = index.get("xray/a.png").expect("first picture").clone();
        let second = index.get("xray/b.png").expect("second picture").clone();
        assert!(first.start < second.start);
        assert_eq!(pack.get(first), Some(b"a".as_slice()));
        assert_eq!(pack.get(second), Some(b"bb".as_slice()));

        let cut_short = pack.split_last().expect("not empty").1;
        assert!(parse_pack(cut_short).is_err());
        let longer = [pack.as_slice(), &[0]].concat();
        assert!(parse_pack(&longer).is_err());
        let wrong_magic = [b"XSEICON1".as_slice(), pack.get(8..).expect("body")].concat();
        assert!(parse_pack(&wrong_magic).is_err());
        let huge_count = [
            PACK_MAGIC.as_slice(),
            &u32::MAX.to_le_bytes(),
            pack.get(12..).expect("entries"),
        ]
        .concat();
        assert!(parse_pack(&huge_count).is_err());
        let twice = encode_pack(&[("xray/a.png", b"a".as_slice()), ("xray/a.png", b"a".as_slice())]).expect("encodes");
        assert!(parse_pack(&twice).is_err());
        assert!(parse_pack(b"").is_err());
    }

    #[test]
    fn unknown_key_is_not_an_icon() {
        assert!(icon_path("stalker-cs", "  ").is_none());
        assert!(decode(None, Some("xray/does_not_exist.png"), "does_not_exist").is_none());
    }
}
