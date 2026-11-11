use super::bytes::u32le;
use super::exif::{ExifOrientation, exif_orientation, orientation_tiff};

/// Chunks of an extended WebP that make up the picture; EXIF, XMP and
/// unknown chunks go.
const KEPT_CHUNKS: [&[u8]; 6] = [b"ICCP", b"ANIM", b"ANMF", b"ALPH", b"VP8 ", b"VP8L"];
const IMAGE_CHUNKS: [&[u8]; 3] = [b"VP8 ", b"VP8L", b"ANMF"];

const VP8X_EXIF: u8 = 0x08;
const VP8X_XMP: u8 = 0x04;

fn push_chunk(out: &mut Vec<u8>, chunk: &[u8], size: usize) {
    out.extend_from_slice(chunk);
    if size % 2 == 1 {
        out.push(0);
    }
}

pub(super) fn strip(bytes: &[u8]) -> Result<(Vec<u8>, Option<ExifOrientation>), &'static str> {
    let file_end = 8 + u64::from(u32le(bytes, 4));
    if file_end > bytes.len() as u64 {
        return Err("the RIFF size runs past the end");
    }
    let file_end = file_end as usize;
    // RIFF header, rewritten with the new size once the body is known.
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(b"RIFF\0\0\0\0WEBP");
    // Where the VP8X flags byte landed in `out`, if the file is extended.
    let mut vp8x_flags = None;
    let mut orientation = None;
    let mut saw_image = false;
    let mut pos = 12;
    while pos + 8 <= file_end {
        let kind = &bytes[pos..pos + 4];
        let size = u32le(bytes, pos + 4) as usize;
        let data_end = pos.saturating_add(8).saturating_add(size);
        if data_end > file_end {
            return Err("a chunk runs past the end");
        }
        if pos == 12 && kind == b"VP8X" {
            if size < 10 {
                return Err("VP8X is too short");
            }
            vp8x_flags = Some(out.len() + 8);
            push_chunk(&mut out, &bytes[pos..data_end], size);
        } else if kind == b"EXIF" {
            if vp8x_flags.is_some() && orientation.is_none() {
                orientation = exif_orientation(&bytes[pos + 8..data_end]);
                if let Some(found) = orientation {
                    let tiff = orientation_tiff(found);
                    out.extend_from_slice(b"EXIF");
                    out.extend_from_slice(&(tiff.len() as u32).to_le_bytes());
                    out.extend_from_slice(&tiff);
                }
            }
        } else if KEPT_CHUNKS.contains(&kind) {
            saw_image |= IMAGE_CHUNKS.contains(&kind);
            push_chunk(&mut out, &bytes[pos..data_end], size);
        }
        pos = data_end + size % 2;
    }
    if !saw_image {
        return Err("has no image data");
    }
    if let Some(flags) = vp8x_flags {
        // Say which metadata is left.
        out[flags] &= !(VP8X_EXIF | VP8X_XMP);
        if orientation.is_some() {
            out[flags] |= VP8X_EXIF;
        }
    }
    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok((out, orientation))
}
