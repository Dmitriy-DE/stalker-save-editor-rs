//! Size limits shared by the save readers, writers and discovery.

/// Largest decompressed X-Ray or S2 image that any reader or writer accepts (256 MiB).
pub const MAXIMUM_UNPACKED_BYTES: usize = 256 * 1024 * 1024;
