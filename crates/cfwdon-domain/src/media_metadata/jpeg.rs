use super::bytes::{starts_with, u16be};
use super::exif::{ExifOrientation, exif_orientation, orientation_tiff};

const SOS: u8 = 0xda;
const EOI: u8 = 0xd9;
const APP0: u8 = 0xe0;
const APP1: u8 = 0xe1;
const APP2: u8 = 0xe2;
const APP14: u8 = 0xee;
const APP15: u8 = 0xef;
const COM: u8 = 0xfe;

fn is_standalone(marker: u8) -> bool {
    marker == 0x01 || (0xd0..=0xd7).contains(&marker)
}

/// Whether a marker segment changes how the image decodes and so stays.
fn keeps_segment(marker: u8, data: &[u8]) -> bool {
    match marker {
        APP0 => starts_with(data, 0, b"JFIF\0"),
        APP2 => starts_with(data, 0, b"ICC_PROFILE\0"),
        APP14 => starts_with(data, 0, b"Adobe"),
        // Every other APPn (EXIF, XMP, MPF, Photoshop/IPTC, Ducky, ...) and comments go.
        APP0..=APP15 | COM => false,
        _ => true,
    }
}

fn push_orientation_app1(out: &mut Vec<u8>, orientation: ExifOrientation) {
    let tiff = orientation_tiff(orientation);
    let length = (2 + 6 + tiff.len()) as u16;
    out.extend_from_slice(&[0xff, APP1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
}

/// Where the entropy-coded data that starts at `pos` ends: the next marker
/// other than a stuffed 0xFF00 or a restart marker, or the end of the file.
fn scan_end(bytes: &[u8], mut pos: usize) -> usize {
    loop {
        let Some(ff) = bytes[pos..]
            .iter()
            .position(|&b| b == 0xff)
            .map(|i| pos + i)
        else {
            return bytes.len();
        };
        if ff + 1 >= bytes.len() {
            return bytes.len();
        }
        match bytes[ff + 1] {
            0xff => pos = ff + 1,
            0x00 | 0xd0..=0xd7 => pos = ff + 2,
            _ => return ff,
        }
    }
}

/// Walk the marker segments up to the end of the image, entropy-coded scans
/// included, keeping image segments and the allow-listed APPn. Anything after
/// EOI (MPF's extra pictures, maker trailers) is dropped. A file that ends
/// inside a scan is kept as it stands, as browsers show a truncated JPEG.
pub(super) fn strip(bytes: &[u8]) -> Result<(Vec<u8>, Option<ExifOrientation>), &'static str> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..2]);
    let mut orientation = None;
    let mut saw_scan = false;
    let mut pos = 2;
    loop {
        if pos >= bytes.len() {
            if saw_scan {
                break;
            }
            return Err("ends before the image data");
        }
        if bytes[pos] != 0xff {
            return Err("expected a marker");
        }
        // Fill bytes: any run of 0xFF before the marker code.
        while bytes.get(pos + 1) == Some(&0xff) {
            pos += 1;
        }
        if pos + 1 >= bytes.len() {
            return Err("ends inside a marker");
        }
        let marker = bytes[pos + 1];
        if marker == EOI {
            out.extend_from_slice(&bytes[pos..pos + 2]);
            break;
        }
        if marker == 0x00 || marker == 0xd8 {
            return Err("unexpected marker");
        }
        if is_standalone(marker) {
            out.extend_from_slice(&bytes[pos..pos + 2]);
            pos += 2;
            continue;
        }
        let length = usize::from(u16be(bytes, pos + 2));
        let end = pos + 2 + length;
        if pos + 4 > bytes.len() || length < 2 || end > bytes.len() {
            return Err("a segment runs past the end");
        }
        let data = &bytes[pos + 4..end];
        if marker == APP1 && starts_with(data, 0, b"Exif\0\0") {
            if orientation.is_none() {
                orientation = exif_orientation(data);
                if let Some(found) = orientation {
                    push_orientation_app1(&mut out, found);
                }
            }
        } else if keeps_segment(marker, data) {
            out.extend_from_slice(&bytes[pos..end]);
        }
        pos = end;
        if marker != SOS {
            continue;
        }
        saw_scan = true;
        let scan = scan_end(bytes, pos);
        out.extend_from_slice(&bytes[pos..scan]);
        pos = scan;
    }
    Ok((out, orientation))
}
