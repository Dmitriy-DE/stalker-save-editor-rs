//! Deterministic byte and line differences with conservative patch/merge semantics.
use sse_core::{Error, Result};
use std::cmp::{max, min};

#[derive(Clone, Debug, PartialEq, Eq)]
/// One line-level edit operation.
pub enum LineOp {
    /// Unchanged line bytes.
    Equal(Vec<u8>),
    /// Line removed from the old image.
    Delete(Vec<u8>),
    /// Line inserted into the new image.
    Insert(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Unified-diff hunk with exact line bytes.
pub struct Hunk {
    /// One-based old-file start line.
    /// Old-image inclusive start byte.
    pub old_start: usize,
    /// Number of old-file lines covered.
    pub old_len: usize,
    /// One-based new-file start line.
    /// New-image inclusive start byte.
    pub new_start: usize,
    /// Number of new-file lines covered.
    pub new_len: usize,
    /// Ordered operations in the hunk.
    pub ops: Vec<LineOp>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// One changed byte interval in the old and new images.
pub struct ByteRange {
    /// Old-image inclusive start byte.\n    pub old_start: usize,\n    /// Old-image exclusive end byte.
    pub old_end: usize,
    /// New-image inclusive start byte.\n    pub new_start: usize,\n    /// New-image exclusive end byte.
    pub new_end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Overlapping unequal edits encountered by three-way merge.
pub struct MergeConflict {
    /// First base line touched by either side.
    pub base_start: usize,
    /// Exclusive base-line end.
    pub base_end: usize,
    /// Replacement bytes proposed by the left side.
    pub left: Vec<u8>,
    /// Replacement bytes proposed by the right side.
    pub right: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Result of a conservative three-way line merge.
pub struct MergeResult {
    /// Merged bytes, or the unchanged base when conflicts exist.
    pub bytes: Vec<u8>,
    /// Explicit conflicts; empty means the merge succeeded.
    pub conflicts: Vec<MergeConflict>,
}

fn lines(input: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input.get(i) == Some(&b'\n') {
            if let Some(end) = i.checked_add(1) {
                if let Some(s) = input.get(start..end) {
                    out.push(s);
                }
                start = end;
            }
        }
        i = i.saturating_add(1);
    }
    if start < input.len() {
        if let Some(s) = input.get(start..) {
            out.push(s);
        }
    }
    out
}

/// Myers O(ND) line edit script. A bounded fallback avoids quadratic trace memory on hostile inputs.
pub fn line_diff(old: &[u8], new: &[u8], maximum_trace_cells: usize) -> Result<Vec<LineOp>> {
    let a = lines(old);
    let b = lines(new);
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return Ok(b.into_iter().map(|x| LineOp::Insert(x.to_vec())).collect());
    }
    if m == 0 {
        return Ok(a.into_iter().map(|x| LineOp::Delete(x.to_vec())).collect());
    }
    let max_d = n.checked_add(m).ok_or_else(|| Error::damaged("diff size overflow"))?;
    let width = max_d
        .checked_mul(2)
        .and_then(|v| v.checked_add(3))
        .ok_or_else(|| Error::damaged("diff width overflow"))?;
    if width.saturating_mul(max_d.saturating_add(1)) > maximum_trace_cells {
        return linear_fallback(&a, &b);
    }
    let off = max_d
        .checked_add(1)
        .ok_or_else(|| Error::damaged("diff offset overflow"))?;
    let mut v = vec![0isize; width];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max_d {
        let di = isize::try_from(d).map_err(|_| Error::damaged("diff distance"))?;
        let mut k = di.saturating_neg();
        while k <= di {
            let idx = isize::try_from(off)
                .ok()
                .and_then(|o| o.checked_add(k))
                .and_then(|x| usize::try_from(x).ok())
                .ok_or_else(|| Error::damaged("diff diagonal"))?;
            let left = idx.checked_sub(1).and_then(|x| v.get(x)).copied().unwrap_or(-1);
            let right = idx.checked_add(1).and_then(|x| v.get(x)).copied().unwrap_or(-1);
            let mut x = if k == di.saturating_neg() || (k != di && left < right) {
                right
            } else {
                left.saturating_add(1)
            };
            let mut y = x.saturating_sub(k);
            while x >= 0 && y >= 0 {
                let xu = usize::try_from(x).map_err(|_| Error::damaged("diff x"))?;
                let yu = usize::try_from(y).map_err(|_| Error::damaged("diff y"))?;
                if xu >= n || yu >= m || a.get(xu) != b.get(yu) {
                    break;
                }
                x = x.saturating_add(1);
                y = y.saturating_add(1);
            }
            if let Some(slot) = v.get_mut(idx) {
                *slot = x;
            } else {
                return Err(Error::damaged("diff diagonal outside trace"));
            }
            if usize::try_from(x).ok().is_some_and(|z| z >= n) && usize::try_from(y).ok().is_some_and(|z| z >= m) {
                trace.push(v.clone());
                return backtrack(&a, &b, &trace, off);
            }
            k = k.saturating_add(2);
        }
        trace.push(v.clone());
    }
    Err(Error::damaged("diff did not converge"))
}
fn backtrack(a: &[&[u8]], b: &[&[u8]], trace: &[Vec<isize>], off: usize) -> Result<Vec<LineOp>> {
    let mut x = isize::try_from(a.len()).map_err(|_| Error::damaged("diff x"))?;
    let mut y = isize::try_from(b.len()).map_err(|_| Error::damaged("diff y"))?;
    let mut rev = Vec::new();
    let mut d = trace.len();
    while d > 1 {
        d = d.saturating_sub(1);
        let prev = trace
            .get(d.saturating_sub(1))
            .ok_or_else(|| Error::damaged("diff trace"))?;
        let k = x.saturating_sub(y);
        let di = isize::try_from(d).map_err(|_| Error::damaged("diff d"))?;
        let oi = isize::try_from(off).map_err(|_| Error::damaged("diff off"))?;
        let prev_k = if k == di.saturating_neg()\n            || (k != di
                && prev
                    .get(usize::try_from(oi.saturating_add(k).saturating_sub(1)).unwrap_or(usize::MAX))
                    .copied()
                    .unwrap_or(-1)
                    < prev
                        .get(usize::try_from(oi.saturating_add(k).saturating_add(1)).unwrap_or(usize::MAX))
                        .copied()
                        .unwrap_or(-1))
        {
            k.saturating_add(1)
        } else {
            k.saturating_sub(1)
        };
        let px = prev
            .get(usize::try_from(oi.saturating_add(prev_k)).map_err(|_| Error::damaged("diff prev"))?)
            .copied()
            .ok_or_else(|| Error::damaged("diff prev diagonal"))?;
        let py = px.saturating_sub(prev_k);
        while x > px && y > py {
            x = x.saturating_sub(1);
            y = y.saturating_sub(1);
            let xu = usize::try_from(x).map_err(|_| Error::damaged("diff x"))?;
            rev.push(LineOp::Equal(
                a.get(xu).ok_or_else(|| Error::damaged("diff line"))?.to_vec(),
            ));
        }
        if x == px {
            y = y.saturating_sub(1);
            let yu = usize::try_from(y).map_err(|_| Error::damaged("diff y"))?;
            rev.push(LineOp::Insert(
                b.get(yu).ok_or_else(|| Error::damaged("diff line"))?.to_vec(),
            ));
        } else {
            x = x.saturating_sub(1);
            let xu = usize::try_from(x).map_err(|_| Error::damaged("diff x"))?;
            rev.push(LineOp::Delete(
                a.get(xu).ok_or_else(|| Error::damaged("diff line"))?.to_vec(),
            ));
        }
    }
    while x > 0 && y > 0 {
        x = x.saturating_sub(1);
        y = y.saturating_sub(1);
        rev.push(LineOp::Equal(
            a.get(usize::try_from(x).map_err(|_| Error::damaged("x"))?)
                .ok_or_else(|| Error::damaged("line"))?
                .to_vec(),
        ));
    }
    while x > 0 {
        x = x.saturating_sub(1);
        rev.push(LineOp::Delete(
            a.get(usize::try_from(x).map_err(|_| Error::damaged("x"))?)
                .ok_or_else(|| Error::damaged("line"))?
                .to_vec(),
        ));
    }
    while y > 0 {
        y = y.saturating_sub(1);
        rev.push(LineOp::Insert(
            b.get(usize::try_from(y).map_err(|_| Error::damaged("y"))?)
                .ok_or_else(|| Error::damaged("line"))?
                .to_vec(),
        ));
    }
    rev.reverse();
    Ok(rev)
}
fn linear_fallback(a: &[&[u8]], b: &[&[u8]]) -> Result<Vec<LineOp>> {
    // prefix/suffix + replace middle: linear space/time cutoff
    let mut p = 0usize;
    while p < a.len() && p < b.len() && a.get(p) == b.get(p) {
        p = p.saturating_add(1);
    }
    let mut s = 0usize;
    while s < a.len().saturating_sub(p)
        && s < b.len().saturating_sub(p)
        && a.get(a.len().saturating_sub(1_usize.saturating_add(s))) == b.get(b.len().saturating_sub(1_usize.saturating_add(s)))
    {
        s = s.saturating_add(1);
    }
    let mut out = Vec::new();
    for x in a.iter().take(p) {
        out.push(LineOp::Equal((*x).to_vec()));
    }
    for x in a.iter().skip(p).take(a.len().saturating_sub(p.saturating_add(s))) {
        out.push(LineOp::Delete((*x).to_vec()));
    }
    for x in b.iter().skip(p).take(b.len().saturating_sub(p.saturating_add(s))) {
        out.push(LineOp::Insert((*x).to_vec()));
    }
    for x in a.iter().skip(a.len().saturating_sub(s)) {
        out.push(LineOp::Equal((*x).to_vec()));
    }
    Ok(out)
}

/// Builds unified hunks with the requested number of context lines.
pub fn unified_hunks(old: &[u8], new: &[u8], context: usize) -> Result<Vec<Hunk>> {
    let ops = line_diff(old, new, 8_000_000)?;
    let mut hunks = Vec::new();
    let mut oi = 1usize;
    let mut ni = 1usize;
    let mut pending: Vec<LineOp> = Vec::new();
    let mut pre: Vec<Vec<u8>> = Vec::new();
    let mut hs_old = 1usize;
    let mut hs_new = 1usize;
    let mut active = false;
    for op in ops {
        match &op {
            LineOp::Equal(line) => {
                if active {
                    pending.push(op.clone());
                    if pending
                        .iter()
                        .rev()
                        .take(context.saturating_add(1))
                        .all(|x| matches!(x, LineOp::Equal(_)))
                    {
                        let trim = pending.len().saturating_sub(context);
                        let tail = pending.split_off(trim);
                        let (ol, nl) = counts(&pending);
                        hunks.push(Hunk {
                            old_start: hs_old,
                            old_len: ol,
                            new_start: hs_new,
                            new_len: nl,
                            ops: pending,
                        });
                        pending = Vec::new();
                        pre = tail
                            .into_iter()
                            .filter_map(|x| if let LineOp::Equal(v) = x { Some(v) } else { None })
                            .collect();
                        active = false;
                    }
                } else {
                    pre.push(line.clone());
                    if pre.len() > context {
                        pre.remove(0);
                    }
                }
                oi = oi.saturating_add(1);
                ni = ni.saturating_add(1);
            }
            LineOp::Delete(_) | LineOp::Insert(_) => {
                if !active {
                    hs_old = oi.saturating_sub(pre.len());
                    hs_new = ni.saturating_sub(pre.len());
                    for x in pre.drain(..) {
                        pending.push(LineOp::Equal(x));
                    }
                    active = true;
                }
                pending.push(op.clone());
                if matches!(op, LineOp::Delete(_)) {
                    oi = oi.saturating_add(1)
                } else {
                    ni = ni.saturating_add(1)
                }
            }
        }
    }
    if active {
        let (ol, nl) = counts(&pending);
        hunks.push(Hunk {
            old_start: hs_old,
            old_len: ol,
            new_start: hs_new,
            new_len: nl,
            ops: pending,
        });
    }
    Ok(hunks)
}
fn counts(ops: &[LineOp]) -> (usize, usize) {
    let mut a = 0;
    let mut b = 0;
    for x in ops {
        match x {
            LineOp::Equal(_) => {
                a = a.saturating_add(1);\n                b = b.saturating_add(1)
            }
            LineOp::Delete(_) => a = a.saturating_add(1),\n            LineOp::Insert(_) => b = b.saturating_add(1),
        }
    }
    (a, b)
}

/// Applies hunks only when their full old-side context occurs exactly once.
pub fn apply_hunks(input: &[u8], hunks: &[Hunk]) -> Result<Vec<u8>> {
    let mut cur = input.to_vec();
    for h in hunks {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for op in &h.ops {
            match op {
                LineOp::Equal(x) => {
                    old.extend_from_slice(x);
                    new.extend_from_slice(x)
                }
                LineOp::Delete(x) => old.extend_from_slice(x),
                LineOp::Insert(x) => new.extend_from_slice(x),
            }
        }
        let hits = find_all(&cur, &old);
        if hits.len() != 1 {
            return Err(Error::Refused("patch context is not unique".to_owned()));
        }
        let pos = *hits.first().ok_or_else(|| Error::damaged("missing patch context"))?;
        let end = pos
            .checked_add(old.len())
            .ok_or_else(|| Error::damaged("patch range overflow"))?;
        cur.splice(pos..end, new);
    }
    Ok(cur)
}
fn find_all(h: &[u8], n: &[u8]) -> Vec<usize> {
    if n.is_empty() {
        return vec![0];
    }
    h.windows(n.len())
        .enumerate()
        .filter_map(|(i, w)| (w == n).then_some(i))
        .collect()
}

/// Returns changed byte ranges, coalescing gaps no larger than `merge_gap`.
pub fn byte_ranges(a: &[u8], b: &[u8], merge_gap: usize) -> Vec<ByteRange> {
    let common = min(a.len(), b.len());
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..common {
        if a.get(i) != b.get(i) {
            if start.is_none() {
                start = Some(i)
            }
        } else if let Some(s) = start {
            out.push(ByteRange {
                old_start: s,
                old_end: i,
                new_start: s,
                new_end: i,
            });
            start = None
        }
    }
    if let Some(s) = start {
        out.push(ByteRange {
            old_start: s,
            old_end: common,
            new_start: s,
            new_end: common,
        });
    }
    if a.len() != b.len() {
        out.push(ByteRange {
            old_start: common,
            old_end: a.len(),
            new_start: common,
            new_end: b.len(),
        });
    }
    let mut merged: Vec<ByteRange> = Vec::new();
    for r in out {
        if let Some(last) = merged.last_mut() {
            let gap = r
                .old_start
                .saturating_sub(last.old_end)
                .max(r.new_start.saturating_sub(last.new_end));
            if gap <= merge_gap {
                last.old_end = r.old_end;
                last.new_end = r.new_end;
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

/// Conservative three-way merge: identical sides win; disjoint line edits combine; overlapping unequal edits conflict.
pub fn merge3(base: &[u8], left: &[u8], right: &[u8]) -> Result<MergeResult> {
    if left == right {
        return Ok(MergeResult {
            bytes: left.to_vec(),
            conflicts: Vec::new(),
        });
    }
    if left == base {
        return Ok(MergeResult {
            bytes: right.to_vec(),
            conflicts: Vec::new(),
        });
    }
    if right == base {
        return Ok(MergeResult {
            bytes: left.to_vec(),
            conflicts: Vec::new(),
        });
    }
    let le = edits(base, left)?;
    let re = edits(base, right)?;
    let mut all = Vec::new();
    for e in &le {
        all.push((0usize, e.clone()));
    }
    for e in &re {
        all.push((1usize, e.clone()));
    }
    all.sort_by_key(|(_, e)| e.0);
    let mut conflicts = Vec::new();
    for l in &le {
        for r in &re {
            if ranges_overlap(l.0, l.1, r.0, r.1) && l.2 != r.2 {
                conflicts.push(MergeConflict {
                    base_start: min(l.0, r.0),
                    base_end: max(l.1, r.1),
                    left: l.2.clone(),
                    right: r.2.clone(),
                });
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            bytes: base.to_vec(),
            conflicts,
        });
    }
    let bl = lines(base);
    let mut out = Vec::new();
    let mut pos = 0usize;
    for (_, e) in all {
        if e.0 < pos {
            continue;
        }
        for x in bl.iter().skip(pos).take(e.0.saturating_sub(pos)) {
            out.extend_from_slice(x);
        }
        out.extend_from_slice(&e.2);
        pos = e.1;
    }
    for x in bl.iter().skip(pos) {
        out.extend_from_slice(x);
    }
    Ok(MergeResult { bytes: out, conflicts })
}
fn ranges_overlap(a0: usize, a1: usize, b0: usize, b1: usize) -> bool {
    if a0 == a1 && b0 == b1 {
        return a0 == b0;
    }
    a0 < b1 && b0 < a1 || (a0 == a1 && (a0 >= b0 && a0 <= b1)) || (b0 == b1 && (b0 >= a0 && b0 <= a1))
}
fn edits(base: &[u8], other: &[u8]) -> Result<Vec<(usize, usize, Vec<u8>)>> {
    let ops = line_diff(base, other, 8_000_000)?;
    let mut pos = 0usize;
    let mut out = Vec::new();
    let mut cur: Option<(usize, usize, Vec<u8>)> = None;
    for op in ops {
        match op {
            LineOp::Equal(_) => {
                if let Some(e) = cur.take() {
                    out.push(e);
                }
                pos = pos.saturating_add(1)
            }
            LineOp::Delete(_) => {
                let e = cur.get_or_insert((pos, pos, Vec::new()));
                e.1 = e.1.saturating_add(1);
                pos = pos.saturating_add(1)
            }
            LineOp::Insert(x) => {
                let e = cur.get_or_insert((pos, pos, Vec::new()));
                e.2.extend_from_slice(&x)
            }
        }
    }
    if let Some(e) = cur {
        out.push(e)
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_diff_merges() {
        assert_eq!(byte_ranges(b"abcX12Yz", b"abcQ12Rz", 2).len(), 1)
    }
    #[test]
    fn crlf_round_trip() {
        let a = b"a\r\nb\r\n";
        let b = b"a\r\nc\r\n";
        let h = unified_hunks(a, b, 1).unwrap_or_default();
        assert_eq!(apply_hunks(a, &h).ok().as_deref(), Some(b.as_slice()));
    }
    #[test]
    fn ambiguous_patch_refused() {
        let h = Hunk {
            old_start: 1,
            old_len: 1,
            new_start: 1,
            new_len: 1,
            ops: vec![LineOp::Delete(b"x\n".to_vec()), LineOp::Insert(b"y\n".to_vec())],
        };
        assert!(apply_hunks(b"x\nx\n", &[h]).is_err());
    }
    #[test]
    fn merge_disjoint() {
        let r = merge3(b"a\nb\nc\n", b"A\nb\nc\n", b"a\nb\nC\n").ok();
        assert_eq!(r.as_ref().map(|x| x.conflicts.len()), Some(0));
        assert_eq!(r.as_ref().map(|x| x.bytes.as_slice()), Some(b"A\nb\nC\n".as_slice()));
    }
    #[test]
    fn no_trailing_newline() {
        let d = line_diff(b"a\nlast", b"a\nLAST", 10000).unwrap_or_default();
        assert!(d.iter().any(|x| matches!(x, LineOp::Delete(_))));
    }
}
