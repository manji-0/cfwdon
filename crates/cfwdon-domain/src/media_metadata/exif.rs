use super::bytes::{starts_with, u16be, u16le, u32be, u32le};

/// An EXIF orientation other than 1, the ones a viewer acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExifOrientation(u8);

impl ExifOrientation {
    pub fn new(value: u16) -> Option<Self> {
        (2..=8).contains(&value).then_some(Self(value as u8))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// The Orientation tag (0x0112) of IFD0 in a TIFF-structured EXIF block,
/// which may start with JPEG's `Exif\0\0` header. `None` when it is 1,
/// missing, or the block cannot be read.
pub fn exif_orientation(exif: &[u8]) -> Option<ExifOrientation> {
    let tiff = if starts_with(exif, 0, b"Exif\0\0") {
        &exif[6..]
    } else {
        exif
    };
    let little = starts_with(tiff, 0, b"II");
    if !little && !starts_with(tiff, 0, b"MM") {
        return None;
    }
    let u16_at = |offset| {
        if little {
            u16le(tiff, offset)
        } else {
            u16be(tiff, offset)
        }
    };
    let u32_at = |offset| {
        if little {
            u32le(tiff, offset)
        } else {
            u32be(tiff, offset)
        }
    };
    if tiff.len() < 8 || u16_at(2) != 42 {
        return None;
    }
    let ifd = u32_at(4) as usize;
    if ifd.saturating_add(2) > tiff.len() {
        return None;
    }
    for index in 0..usize::from(u16_at(ifd)) {
        let entry = ifd + 2 + index * 12;
        if entry + 12 > tiff.len() {
            return None;
        }
        if u16_at(entry) == 0x0112 {
            // SHORT, one value, held in the first two bytes of the value field.
            return if u16_at(entry + 2) == 3 && u32_at(entry + 4) == 1 {
                ExifOrientation::new(u16_at(entry + 8))
            } else {
                None
            };
        }
    }
    None
}

/// A big-endian TIFF block whose IFD0 holds only the Orientation tag: the
/// header, an entry count of one, the SHORT entry, and no next IFD.
pub(super) fn orientation_tiff(orientation: ExifOrientation) -> [u8; 26] {
    let mut tiff = *b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\0\0\0\0\0\0\0";
    tiff[19] = orientation.get();
    tiff
}
