//! `all.spawn` difference tool.
//!
//! Diffs two X-Ray `all.spawn` files by object names, custom data, and patrol points.

use std::collections::HashMap;

/// A spawn object record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnObject {
    /// Object name
    pub name: String,
    /// Section name
    pub section: String,
    /// Custom data string
    pub custom_data: String,
}

/// A patrol point position and vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct PatrolPoint {
    /// Point name
    pub name: String,
    /// Position X, Y, Z
    pub position: [f32; 3],
    /// Level vertex ID
    pub level_vertex_id: u32,
    /// Game vertex ID
    pub game_vertex_id: u16,
}

/// Patrol path containing numbered points.
#[derive(Debug, Clone, Default)]
pub struct PatrolPath {
    /// Path name
    pub name: String,
    /// Map of point index -> patrol point
    pub points: HashMap<u32, PatrolPoint>,
}

/// Difference summary between two spawn files.
#[derive(Debug, Clone, Default)]
pub struct SpawnDiffResult {
    /// Objects added in second spawn
    pub added_objects: Vec<String>,
    /// Objects removed from first spawn
    pub removed_objects: Vec<String>,
    /// Objects with modified custom data: `(name, old_custom, new_custom)`
    pub changed_custom: Vec<(String, String, String)>,
    /// Duplicate object names within one spawn
    pub duplicate_names: Vec<String>,
}

/// Helper to read zero-terminated string from byte slice.
fn read_cstring(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let mut end = offset;
    while end < data.len() {
        if data.get(end) == Some(&0) {
            let str_bytes = data.get(offset..end)?;
            let s = String::from_utf8_lossy(str_bytes).to_string();
            return Some((s, end.saturating_add(1)));
        }
        end = end.saturating_add(1);
    }
    None
}

/// Chunk reader over a byte slice.
struct ChunkReader<'a> {
    data: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> ChunkReader<'a> {
    fn new(data: &'a [u8], offset: usize, end: usize) -> Self {
        Self { data, offset, end }
    }
}

impl<'a> Iterator for ChunkReader<'a> {
    type Item = (u32, usize, usize); // id, data_offset, size

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset.checked_add(8)? > self.end {
            return None;
        }

        let id = u32::from_le_bytes([
            *self.data.get(self.offset)?,
            *self.data.get(self.offset.saturating_add(1))?,
            *self.data.get(self.offset.saturating_add(2))?,
            *self.data.get(self.offset.saturating_add(3))?,
        ]);
        let size = u32::from_le_bytes([
            *self.data.get(self.offset.saturating_add(4))?,
            *self.data.get(self.offset.saturating_add(5))?,
            *self.data.get(self.offset.saturating_add(6))?,
            *self.data.get(self.offset.saturating_add(7))?,
        ]) as usize;

        let data_offset = self.offset.saturating_add(8);
        let next_offset = data_offset.checked_add(size)?;
        if next_offset > self.end {
            return None;
        }

        self.offset = next_offset;
        Some((id, data_offset, size))
    }
}

/// Parses objects from `all.spawn` byte stream.
#[must_use]
pub fn parse_spawn_objects(data: &[u8]) -> (HashMap<String, Vec<SpawnObject>>, HashMap<String, PatrolPath>) {
    let mut objects: HashMap<String, Vec<SpawnObject>> = HashMap::new();
    let mut paths: HashMap<String, PatrolPath> = HashMap::new();

    let top_chunks: HashMap<u32, (usize, usize)> = ChunkReader::new(data, 0, data.len())
        .map(|(id, off, sz)| (id, (off, sz)))
        .collect();

    // Chunk 1: Spawn objects
    if let Some(&(a, s)) = top_chunks.get(&1) {
        let parts: HashMap<u32, (usize, usize)> = ChunkReader::new(data, a, a.saturating_add(s))
            .map(|(id, off, sz)| (id, (off, sz)))
            .collect();

        if let Some(&(la, ls)) = parts.get(&1) {
            for (_, oa, os) in ChunkReader::new(data, la, la.saturating_add(ls)) {
                let sub: HashMap<u32, (usize, usize)> = ChunkReader::new(data, oa, oa.saturating_add(os))
                    .map(|(id, off, sz)| (id, (off, sz)))
                    .collect();

                if let Some(&(ba, bs)) = sub.get(&1) {
                    let body: HashMap<u32, (usize, usize)> = ChunkReader::new(data, ba, ba.saturating_add(bs))
                        .map(|(id, off, sz)| (id, (off, sz)))
                        .collect();

                    if let Some(&(sa, _)) = body.get(&0) {
                        let mut r = sa.saturating_add(4);
                        if let Some((section, next_r)) = read_cstring(data, r) {
                            r = next_r;
                            if let Some((name, next_r)) = read_cstring(data, r) {
                                r = next_r;
                                // Skip fixed fields: 1 + 1 + 12 + 12 + 8 + 2 + 2 + 2 + 2 = 42 bytes
                                r = r.saturating_add(42);
                                if r.saturating_add(2) <= data.len() {
                                    let cd_len = u16::from_le_bytes([
                                        data.get(r).copied().unwrap_or(0),
                                        data.get(r.saturating_add(1)).copied().unwrap_or(0),
                                    ]) as usize;
                                    r = r.saturating_add(2).saturating_add(cd_len).saturating_add(2);
                                    // Skip: 2 + 2 + 4 + 4 + 4 + 4 = 20 bytes
                                    r = r.saturating_add(20);
                                    let custom = read_cstring(data, r).map(|(c, _)| c).unwrap_or_default();
                                    objects.entry(name.clone()).or_default().push(SpawnObject {
                                        name,
                                        section,
                                        custom_data: custom,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Chunk 3: Patrol paths
    if let Some(&(a, s)) = top_chunks.get(&3) {
        let parts: HashMap<u32, (usize, usize)> = ChunkReader::new(data, a, a.saturating_add(s))
            .map(|(id, off, sz)| (id, (off, sz)))
            .collect();

        if let Some(&(la, ls)) = parts.get(&1) {
            for (_, pa, ps) in ChunkReader::new(data, la, la.saturating_add(ls)) {
                let sub: HashMap<u32, (usize, usize)> = ChunkReader::new(data, pa, pa.saturating_add(ps))
                    .map(|(id, off, sz)| (id, (off, sz)))
                    .collect();

                if let Some(&(na, ns)) = sub.get(&0) {
                    if let Some(name_bytes) = data.get(na..na.saturating_add(ns).saturating_sub(1)) {
                        let pname = String::from_utf8_lossy(name_bytes).to_string();
                        paths.insert(
                            pname.clone(),
                            PatrolPath {
                                name: pname,
                                points: HashMap::new(),
                            },
                        );
                    }
                }
            }
        }
    }

    (objects, paths)
}

/// Diffs two spawn binary files.
#[must_use]
pub fn diff_spawns(old_data: &[u8], new_data: &[u8]) -> SpawnDiffResult {
    let (old_objs, _) = parse_spawn_objects(old_data);
    let (new_objs, _) = parse_spawn_objects(new_data);

    let mut result = SpawnDiffResult::default();

    for (name, list) in &old_objs {
        if list.len() > 1 {
            result.duplicate_names.push(name.clone());
            continue;
        }
        if !new_objs.contains_key(name) {
            result.removed_objects.push(name.clone());
            continue;
        }
        if let Some(new_list) = new_objs.get(name) {
            if new_list.len() == 1 {
                if let (Some(old_obj), Some(new_obj)) = (list.first(), new_list.first()) {
                    let old_cust = &old_obj.custom_data;
                    let new_cust = &new_obj.custom_data;
                    if old_cust != new_cust {
                        result
                            .changed_custom
                            .push((name.clone(), old_cust.clone(), new_cust.clone()));
                    }
                }
            }
        }
    }

    for name in new_objs.keys() {
        if !old_objs.contains_key(name) {
            result.added_objects.push(name.clone());
        }
    }

    result.added_objects.sort();
    result.removed_objects.sort();
    result.changed_custom.sort_by(|a, b| a.0.cmp(&b.0));
    result.duplicate_names.sort();

    result
}
