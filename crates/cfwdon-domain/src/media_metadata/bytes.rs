//! Bounds-tolerant big- and little-endian reads: a read past the end gives
//! zeros, so the walkers check lengths once instead of at every field.

pub(super) fn at(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

pub(super) fn u16be(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([at(bytes, offset), at(bytes, offset.saturating_add(1))])
}

pub(super) fn u16le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([at(bytes, offset), at(bytes, offset.saturating_add(1))])
}

pub(super) fn u32be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(four(bytes, offset))
}

pub(super) fn u32le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(four(bytes, offset))
}

fn four(bytes: &[u8], offset: usize) -> [u8; 4] {
    [0, 1, 2, 3].map(|i| at(bytes, offset.saturating_add(i)))
}

/// An unsigned big-endian integer of 0, 4 or 8 bytes (ISOBMFF field sizes).
pub(super) fn uint_be(bytes: &[u8], offset: usize, size: usize) -> u64 {
    match size {
        0 => 0,
        4 => u64::from(u32be(bytes, offset)),
        _ => {
            (u64::from(u32be(bytes, offset)) << 32)
                | u64::from(u32be(bytes, offset.saturating_add(4)))
        }
    }
}

/// A four-character code as the big-endian number its bytes spell.
pub(super) const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// Whether `bytes` holds `text` at `offset`.
pub(super) fn starts_with(bytes: &[u8], offset: usize, text: &[u8]) -> bool {
    bytes
        .get(offset..)
        .is_some_and(|rest| rest.starts_with(text))
}

/// `offset + length` as an index into a file of `len` bytes, if it fits.
pub(super) fn end_within(offset: usize, length: u64, len: usize) -> Option<usize> {
    let end = (offset as u64).checked_add(length)?;
    (end <= len as u64).then_some(end as usize)
}
