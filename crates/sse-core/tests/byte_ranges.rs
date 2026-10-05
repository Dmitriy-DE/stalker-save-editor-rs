//! Regression tests for paired source/replacement image ranges.

use sse_core::byte_ranges::{verify_unchanged_outside_ranges, ChangedRange};

#[test]
fn accepts_shifted_bytes_when_only_the_declared_span_changes() -> sse_core::Result<()> {
    let ranges = [ChangedRange {
        before: 5..8,
        after: 5..11,
    }];

    verify_unchanged_outside_ranges(b"head-OLD-tail!", b"head-LONGER-tail!", &ranges)
}

#[test]
fn rejects_a_collateral_byte_change_outside_declared_ranges() {
    let ranges = [ChangedRange {
        before: 5..8,
        after: 5..11,
    }];

    assert!(
        verify_unchanged_outside_ranges(b"head-OLD-tail!", b"head-LONGER-tXil!", &ranges)
            .is_err_and(|error| error.to_string().contains("outside declared changed ranges"))
    );
}

#[test]
fn rejects_overlapping_or_unsorted_old_and_new_ranges() {
    let before = [
        ChangedRange {
            before: 2..4,
            after: 2..4,
        },
        ChangedRange {
            before: 3..5,
            after: 5..6,
        },
    ];
    assert!(verify_unchanged_outside_ranges(b"abcdef", b"abXYef", &before)
        .is_err_and(|error| error.to_string().contains("overlap or are out of order")));
}

#[test]
fn rejects_a_changed_range_outside_either_image() {
    let ranges = [ChangedRange {
        before: 4..7,
        after: 4..7,
    }];
    assert!(verify_unchanged_outside_ranges(b"abcd", b"wxyz", &ranges)
        .is_err_and(|error| error.to_string().contains("outside an image")));
}
