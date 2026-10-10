//! Byte-range checks shared by the X-Ray and S2 writers.

use crate::{Error, Result};
use std::ops::Range;

/// A span a write changed: `before` in the source image, `after` in the replacement image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedRange {
    /// Range in the source image.
    pub before: Range<usize>,
    /// Range in the replacement image.
    pub after: Range<usize>,
}

/// Fails unless every byte outside the declared changed ranges is equal in both images.
///
/// # Errors
/// Returns `Error::Damaged` when the ranges overlap, are out of order or leave an image, or when a byte outside them
/// differs.
pub fn verify_unchanged_outside_ranges(before: &[u8], after: &[u8], ranges: &[ChangedRange]) -> Result<()> {
    let mut before_cursor = 0;
    let mut after_cursor = 0;

    for changed in ranges {
        if changed.before.start > changed.before.end
            || changed.after.start > changed.after.end
            || changed.before.start < before_cursor
            || changed.after.start < after_cursor
        {
            return Err(Error::damaged("changed ranges overlap or are out of order"));
        }
        if changed.before.end > before.len() || changed.after.end > after.len() {
            return Err(Error::damaged("changed range is outside an image"));
        }
        let before_gap = before
            .get(before_cursor..changed.before.start)
            .ok_or_else(|| Error::damaged("source gap is outside the image"))?;
        let after_gap = after
            .get(after_cursor..changed.after.start)
            .ok_or_else(|| Error::damaged("replacement gap is outside the image"))?;
        if before_gap != after_gap {
            return Err(Error::damaged("save bytes differ outside declared changed ranges"));
        }
        before_cursor = changed.before.end;
        after_cursor = changed.after.end;
    }

    let before_tail = before
        .get(before_cursor..)
        .ok_or_else(|| Error::damaged("source tail is outside the image"))?;
    let after_tail = after
        .get(after_cursor..)
        .ok_or_else(|| Error::damaged("replacement tail is outside the image"))?;
    if before_tail != after_tail {
        return Err(Error::damaged("save bytes differ outside declared changed ranges"));
    }
    Ok(())
}
