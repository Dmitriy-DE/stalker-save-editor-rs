//! Item icons for the inventory, stashes, the add list and the encyclopedia (port of the C# `ItemIconService`).
//!
//! Source order: the installed game's own icon (when its content was read), otherwise the pack shipped with the
//! editor, looked up through the alias table. Missing icons are remembered as missing. PNG decoding runs on a
//! worker thread; a screen draws its placeholder until the icon is ready and then gets an [`IconsReady`] message.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use sse_catalog::{GameContent, Stalker2ItemCatalog};

use crate::event_loop::Proxy;
use crate::item_icon_pack::{ALIASES, PACK};
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

fn pack_entries() -> &'static HashMap<&'static str, &'static [u8]> {
    static PACK_MAP: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    PACK_MAP.get_or_init(|| PACK.iter().map(|(path, bytes)| (*path, *bytes)).collect())
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
/// When done, `screen` receives [`IconsReady`] so it can draw the icon.
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
    let Some(proxy) = proxy.cloned() else {
        if let Ok(mut guard) = cache().lock() {
            guard.pending.remove(&slot);
        }
        return;
    };
    let content = installed().lock().ok().and_then(|map| map.get(&family).cloned());
    let path = icon_path(release, key);
    let key = key.to_owned();
    sse_app::tasks::spawn_named_detached("item-icon", move || {
        let entry = decode(content.as_deref(), path.as_deref(), &key).map_or(Entry::Missing, Entry::Ready);
        if let Ok(mut guard) = cache().lock() {
            guard.pending.remove(&slot);
            if guard.entries.insert(slot.clone(), entry).is_none() {
                guard.order.push_back(slot.clone());
            }
            while guard.order.len() > CACHE_LIMIT {
                if let Some(oldest) = guard.order.pop_front() {
                    guard.entries.remove(&oldest);
                }
            }
        }
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
        None => pack_entries().get(&path?)?.to_vec(),
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

    #[test]
    fn shipped_pack_icon_decodes_to_pixels() {
        let image = decode(None, Some("xray/mp_wpn_knife.png"), "mp_wpn_knife").expect("pack icon decodes");
        assert!(image.width > 0 && image.height > 0);
        assert_eq!(image.pixels.len(), usize::try_from(image.width * image.height).unwrap());
    }

    #[test]
    fn unknown_key_is_not_an_icon() {
        assert!(icon_path("stalker-cs", "  ").is_none());
        assert!(decode(None, Some("xray/does_not_exist.png"), "does_not_exist").is_none());
    }
}
