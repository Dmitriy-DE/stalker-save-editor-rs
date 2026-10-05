//! Contracts every other crate builds on: errors, the owned save buffer, the checked byte cursor.
//!
//! Rules that hold for the whole workspace (see `AGENTS.md`):
//! * a save is read into one owned buffer and never copied "to be safe";
//! * every offset is computed with checked arithmetic and every read is bounds-checked;
//! * a reader that does not understand a field keeps its bytes, it never drops them;
//! * a heuristic that locates data must match in exactly one place or report nothing.

mod buffer;
mod cursor;
mod error;

pub use buffer::SaveBuffer;
pub use cursor::Cursor;
pub use error::{Error, ExitCode, Result};

use std::ops::Range;

/// A changed span in the source image and its corresponding span in the prepared image.
///
/// Paired ranges account for insertions and deletions: bytes outside each pair must remain
/// identical even when later offsets move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteChangeRange {
    /// Span in the original image. Empty for a pure insertion.
    pub before: Range<usize>,
    /// Corresponding span in the prepared image. Empty for a pure deletion.
    pub after: Range<usize>,
}

/// Verifies that every byte outside the declared changed spans is preserved exactly.
///
/// Ranges must be ordered, non-overlapping, and pair unchanged regions of equal length.
///
/// # Errors
/// Returns an error for invalid ranges or any unexpected difference in an unmodified region.
pub fn verify_unmodified_bytes(source: &[u8], output: &[u8], changes: &[ByteChangeRange]) -> Result<()> {
    let mut source_cursor = 0_usize;
    let mut output_cursor = 0_usize;

    for change in changes {
        if change.before.start < source_cursor
            || change.before.start > change.before.end
            || change.before.end > source.len()
            || change.after.start < output_cursor
            || change.after.start > change.after.end
            || change.after.end > output.len()
        {
            return Err(Error::damaged("changed byte ranges are invalid or overlap"));
        }

        let source_region = source
            .get(source_cursor..change.before.start)
            .ok_or_else(|| Error::damaged("source byte range is invalid"))?;
        let output_region = output
            .get(output_cursor..change.after.start)
            .ok_or_else(|| Error::damaged("prepared byte range is invalid"))?;
        if source_region != output_region {
            return Err(Error::damaged("unmodified save bytes differ before a changed range"));
        }

        source_cursor = change.before.end;
        output_cursor = change.after.end;
    }

    let source_tail = source
        .get(source_cursor..)
        .ok_or_else(|| Error::damaged("source byte tail is invalid"))?;
    let output_tail = output
        .get(output_cursor..)
        .ok_or_else(|| Error::damaged("prepared byte tail is invalid"))?;
    if source_tail != output_tail {
        return Err(Error::damaged(
            "unmodified save bytes differ after the final changed range",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod byte_change_tests {
    use super::{verify_unmodified_bytes, ByteChangeRange};

    #[test]
    fn checks_unmodified_bytes_across_insertions_and_edits() {
        let source = b"ABCdefGHI";
        let output = b"ABCXYZdEfGHI";
        let changes = [
            ByteChangeRange {
                before: 3..3,
                after: 3..6,
            },
            ByteChangeRange {
                before: 4..5,
                after: 7..8,
            },
        ];

        assert!(verify_unmodified_bytes(source, output, &changes).is_ok());
    }

    #[test]
    fn rejects_unexpected_changes_outside_ranges() {
        let source = b"unchanged";
        let output = b"unXhanged";

        assert!(verify_unmodified_bytes(source, output, &[]).is_err());
    }

    #[test]
    fn rejects_overlapping_or_out_of_bounds_change_ranges() {
        let source = b"abcdef";
        let output = b"abXYef";
        let overlapping = [
            ByteChangeRange {
                before: 1..4,
                after: 1..4,
            },
            ByteChangeRange {
                before: 3..5,
                after: 3..5,
            },
        ];
        let out_of_bounds = [ByteChangeRange {
            before: 5..7,
            after: 5..7,
        }];

        assert!(verify_unmodified_bytes(source, output, &overlapping).is_err());
        assert!(verify_unmodified_bytes(source, output, &out_of_bounds).is_err());
    }
}
