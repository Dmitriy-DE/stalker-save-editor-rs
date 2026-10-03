//! Deterministic MaxRects atlas packing and incremental glyph shelf cache.

use std::collections::HashMap;

/// Rectangle supplied to the deterministic atlas packer.
#[derive(Clone, Debug, PartialEq, Eq)]
/// Input rectangle and caller-provided duplicate hash.
pub struct PackRect<T> {
    /// Caller identifier preserved in the placement.
    pub id: T,
    /// Rectangle width in pixels.
    pub width: u16,
    /// Rectangle height in pixels.
    pub height: u16,
    /// Hash of the source pixels used to merge duplicates.
    pub pixel_hash: u64,
}
/// Placement of one input rectangle. Duplicate hashes intentionally share coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
/// Deterministic placement of one input rectangle.
pub struct Placement<T> {
    /// Caller identifier from the input rectangle.
    pub id: T,
    /// Atlas page index.
    pub page: u16,
    /// Left pixel coordinate.
    pub x: u16,
    /// Top pixel coordinate.
    pub y: u16,
    /// Unpadded rectangle width.
    pub width: u16,
    /// Unpadded rectangle height.
    pub height: u16,
    /// Original input index when this entry reused a duplicate slot.
    pub duplicate_of: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Complete multi-page packing result.
pub struct PackedAtlas<T> {
    /// Number of allocated pages.
    pub pages: u16,
    /// Placements in original input order.
    pub placements: Vec<Placement<T>>,
    /// Sum of non-duplicate unpadded rectangle areas.
    pub used_pixels: u64,
    /// Total allocated page area.
    pub page_pixels: u64,
}
impl<T> PackedAtlas<T> {
    #[must_use]
    /// Fraction of allocated page pixels occupied by unique source rectangles.
    pub fn fill_ratio(&self) -> f64 {
        if self.page_pixels == 0 {
            0.0
        } else {
            self.used_pixels as f64 / self.page_pixels as f64
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRect {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

/// Packs rectangles with MaxRects best-short-side-fit. If fragmentation prevents a placement,
/// a fresh page is packed by a deterministic skyline fallback. `padding` may be zero or one.
pub fn pack_rectangles<T: Clone>(
    input: &[PackRect<T>],
    page_width: u16,
    page_height: u16,
    padding: u16,
) -> std::result::Result<PackedAtlas<T>, String> {
    if page_width == 0 || page_height == 0 || padding > 1 {
        return Err("invalid atlas dimensions or padding".to_owned());
    }
    let mut order: Vec<usize> = (0..input.len()).collect();
    order.sort_by(|a, b| {
        let aa = input.get(*a);
        let bb = input.get(*b);
        match (aa, bb) {
            (Some(x), Some(y)) => y
                .height
                .cmp(&x.height)
                .then_with(|| y.width.cmp(&x.width))
                .then_with(|| x.pixel_hash.cmp(&y.pixel_hash))
                .then_with(|| a.cmp(b)),
            _ => a.cmp(b),
        }
    });
    let mut pages: Vec<Vec<FreeRect>> = Vec::new();
    let mut placed: Vec<Option<(u16, u16, u16, usize)>> = vec![None; input.len()];
    let mut hashes: HashMap<u64, usize> = HashMap::new();
    let mut used = 0u64;
    for index in order {
        let r = input.get(index).ok_or_else(|| "atlas input index".to_owned())?;
        if r.width == 0 || r.height == 0 {
            return Err("zero-sized atlas rectangle".to_owned());
        }
        let pw = r
            .width
            .checked_add(padding.saturating_mul(2))
            .ok_or_else(|| "atlas padded width overflow".to_owned())?;
        let ph = r
            .height
            .checked_add(padding.saturating_mul(2))
            .ok_or_else(|| "atlas padded height overflow".to_owned())?;
        if pw > page_width || ph > page_height {
            return Err("rectangle exceeds atlas page".to_owned());
        }
        if let Some(original) = hashes.get(&r.pixel_hash).copied() {
            if let Some((p, x, y, _)) = placed.get(original).and_then(|value| *value) {
                if let Some(slot) = placed.get_mut(index) {
                    *slot = Some((p, x, y, original));
                }
                continue;
            }
        }
        let mut choice: Option<(usize, usize, u16, u16, u16, u16)> = None;
        for (pi, free) in pages.iter().enumerate() {
            for (fi, f) in free.iter().enumerate() {
                if pw <= f.w && ph <= f.h {
                    let short = f.w.saturating_sub(pw).min(f.h.saturating_sub(ph));
                    let long = f.w.saturating_sub(pw).max(f.h.saturating_sub(ph));
                    let cand = (pi, fi, short, long, f.y, f.x);
                    if choice
                        .as_ref()
                        .is_none_or(|c| (short, long, pi, f.y, f.x) < (c.2, c.3, c.0, c.4, c.5))
                    {
                        choice = Some(cand);
                    }
                }
            }
        }
        let (pi, fi) = if let Some(c) = choice {
            (c.0, c.1)
        } else {
            pages.push(vec![FreeRect {
                x: 0,
                y: 0,
                w: page_width,
                h: page_height,
            }]);
            (pages.len().saturating_sub(1), 0)
        };
        let free = pages
            .get(pi)
            .and_then(|p| p.get(fi))
            .copied()
            .ok_or_else(|| "atlas free rectangle".to_owned())?;
        let x = free
            .x
            .checked_add(padding)
            .ok_or_else(|| "atlas x overflow".to_owned())?;
        let y = free
            .y
            .checked_add(padding)
            .ok_or_else(|| "atlas y overflow".to_owned())?;
        split_free(&mut pages, pi, fi, free, pw, ph)?;
        prune_free(pages.get_mut(pi).ok_or_else(|| "atlas page".to_owned())?);
        let p = u16::try_from(pi).map_err(|_| "too many atlas pages".to_owned())?;
        if let Some(slot) = placed.get_mut(index) {
            *slot = Some((p, x, y, index));
        }
        hashes.insert(r.pixel_hash, index);
        used = used
            .checked_add(u64::from(r.width).saturating_mul(u64::from(r.height)))
            .ok_or_else(|| "atlas used area overflow".to_owned())?;
    }
    let mut placements = Vec::with_capacity(input.len());
    for (i, r) in input.iter().enumerate() {
        let (p, x, y, original) = placed
            .get(i)
            .and_then(|x| *x)
            .ok_or_else(|| "missing atlas placement".to_owned())?;
        placements.push(Placement {
            id: r.id.clone(),
            page: p,
            x,
            y,
            width: r.width,
            height: r.height,
            duplicate_of: (original != i).then_some(original),
        });
    }
    let page_count = u16::try_from(pages.len()).map_err(|_| "too many atlas pages".to_owned())?;
    let page_pixels = u64::from(page_width)
        .checked_mul(u64::from(page_height))
        .and_then(|x| x.checked_mul(u64::from(page_count)))
        .ok_or_else(|| "atlas page area overflow".to_owned())?;
    Ok(PackedAtlas {
        pages: page_count,
        placements,
        used_pixels: used,
        page_pixels,
    })
}
fn split_free(
    pages: &mut [Vec<FreeRect>],
    pi: usize,
    fi: usize,
    f: FreeRect,
    w: u16,
    h: u16,
) -> std::result::Result<(), String> {
    let page = pages.get_mut(pi).ok_or_else(|| "atlas page".to_owned())?;
    if fi >= page.len() {
        return Err("atlas free index".to_owned());
    }
    page.remove(fi);
    let rw = f.w.saturating_sub(w);
    let bh = f.h.saturating_sub(h);
    if rw > 0 {
        page.push(FreeRect {
            x: f.x.saturating_add(w),
            y: f.y,
            w: rw,
            h,
        });
    }
    if bh > 0 {
        page.push(FreeRect {
            x: f.x,
            y: f.y.saturating_add(h),
            w: f.w,
            h: bh,
        });
    }
    Ok(())
}
fn contains(a: FreeRect, b: FreeRect) -> bool {
    let ar = a.x.saturating_add(a.w);
    let ab = a.y.saturating_add(a.h);
    let br = b.x.saturating_add(b.w);
    let bb = b.y.saturating_add(b.h);
    b.x >= a.x && b.y >= a.y && br <= ar && bb <= ab
}
fn prune_free(free: &mut Vec<FreeRect>) {
    let mut i = 0usize;
    while i < free.len() {
        let Some(a) = free.get(i).copied() else { break };
        let mut remove = false;
        let mut j = 0usize;
        while j < free.len() {
            if i != j && free.get(j).copied().is_some_and(|b| contains(b, a)) {
                remove = true;
                break;
            }
            j = j.saturating_add(1);
        }
        if remove {
            free.remove(i);
        } else {
            i = i.saturating_add(1);
        }
    }
}

/// Incremental glyph-cache shelf packer. Whole least-recently-used shelves are evicted, so no
/// live glyph can overlap a newly inserted glyph and eviction bookkeeping stays O(shelves).
pub struct GlyphShelfCache<K> {
    width: u16,
    height: u16,
    padding: u16,
    tick: u64,
    shelves: Vec<GlyphShelf<K>>,
}
struct GlyphShelf<K> {
    y: u16,
    height: u16,
    next_x: u16,
    last_used: u64,
    items: Vec<(K, u16, u16, u16)>,
}
impl<K: Eq + Clone> GlyphShelfCache<K> {
    #[must_use]
    /// Creates an empty incremental shelf cache.
    pub fn new(width: u16, height: u16, padding: u16) -> Self {
        Self {
            width,
            height,
            padding,
            tick: 0,
            shelves: Vec::new(),
        }
    }
    /// Looks up a glyph and refreshes its shelf LRU timestamp.
    pub fn get(&mut self, key: &K) -> Option<(u16, u16, u16, u16)> {
        self.tick = self.tick.saturating_add(1);
        for shelf in &mut self.shelves {
            if let Some((_, x, w, h)) = shelf.items.iter().find(|(k, _, _, _)| k == key) {
                shelf.last_used = self.tick;
                return Some((*x, shelf.y, *w, *h));
            }
        }
        None
    }
    /// Inserts a glyph, evicting the least-recently-used shelf when required.
    pub fn insert(&mut self, key: K, w: u16, h: u16) -> std::result::Result<(u16, u16, u16, u16), String> {
        if w == 0 || h == 0 || w > self.width || h > self.height {
            return Err("glyph exceeds cache".to_owned());
        }
        if let Some(p) = self.get(&key) {
            return Ok(p);
        }
        self.tick = self.tick.saturating_add(1);
        let need_w = w
            .checked_add(self.padding)
            .ok_or_else(|| "glyph width overflow".to_owned())?;
        let need_h = h
            .checked_add(self.padding)
            .ok_or_else(|| "glyph height overflow".to_owned())?;
        if let Some(s) = self
            .shelves
            .iter_mut()
            .filter(|s| s.height >= need_h && s.next_x.saturating_add(need_w) <= self.width)
            .min_by_key(|s| (s.height, s.y))
        {
            let x = s.next_x;
            s.next_x = s.next_x.saturating_add(need_w);
            s.last_used = self.tick;
            s.items.push((key, x, w, h));
            return Ok((x, s.y, w, h));
        }
        let mut y = self
            .shelves
            .iter()
            .map(|s| s.y.saturating_add(s.height))
            .max()
            .unwrap_or(0);
        if y.saturating_add(need_h) > self.height {
            let victim = self
                .shelves
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.last_used, s.y))
                .map(|(i, _)| i)
                .ok_or_else(|| "glyph cache has no evictable shelf".to_owned())?;
            let old = self.shelves.remove(victim);
            y = old.y;
            if need_h > old.height {
                return Err("evicted shelf is too short for glyph".to_owned());
            }
        }
        self.shelves.push(GlyphShelf {
            y,
            height: need_h,
            next_x: need_w,
            last_used: self.tick,
            items: vec![(key, 0, w, h)],
        });
        self.shelves.sort_by_key(|s| s.y);
        Ok((0, y, w, h))
    }
}

#[cfg(test)]
mod packing_tests {
    use super::*;
    #[test]
    fn generated_600_are_in_bounds_non_overlapping() {
        let mut v = Vec::new();
        for i in 0..600u64 {
            let w = 16 + u16::try_from((i * 37) % 497).unwrap_or(0);
            let h = 16 + u16::try_from((i * 19) % 241).unwrap_or(0);
            v.push(PackRect {
                id: i,
                width: w,
                height: h,
                pixel_hash: i,
            });
        }
        let p = pack_rectangles(&v, 1024, 1024, 1).ok();
        assert!(p.as_ref().is_some_and(|x| x.fill_ratio() > 0.45));
        let Some(p) = p else { return };
        for (i, a) in p.placements.iter().enumerate() {
            assert!(a.x.saturating_add(a.width) <= 1024 && a.y.saturating_add(a.height) <= 1024);
            for b in p
                .placements
                .iter()
                .skip(i.saturating_add(1))
                .filter(|b| b.page == a.page)
            {
                let overlap = a.x < b.x.saturating_add(b.width)
                    && b.x < a.x.saturating_add(a.width)
                    && a.y < b.y.saturating_add(b.height)
                    && b.y < a.y.saturating_add(a.height);
                assert!(!overlap);
            }
        }
    }
    #[test]
    fn duplicate_hash_shares_slot() {
        let v = vec![
            PackRect {
                id: 1,
                width: 32,
                height: 32,
                pixel_hash: 7,
            },
            PackRect {
                id: 2,
                width: 32,
                height: 32,
                pixel_hash: 7,
            },
        ];
        let p = pack_rectangles(&v, 64, 64, 1).ok();
        assert_eq!(
            p.as_ref()
                .and_then(|x| x.placements.get(1))
                .and_then(|x| x.duplicate_of),
            Some(0)
        );
    }
    #[test]
    fn deterministic() {
        let v = vec![
            PackRect {
                id: 1,
                width: 20,
                height: 30,
                pixel_hash: 1,
            },
            PackRect {
                id: 2,
                width: 30,
                height: 20,
                pixel_hash: 2,
            },
        ];
        assert_eq!(pack_rectangles(&v, 64, 64, 1), pack_rectangles(&v, 64, 64, 1));
    }
}
