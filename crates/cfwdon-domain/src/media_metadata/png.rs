use super::bytes::{fourcc, u32be};
use super::exif::{ExifOrientation, exif_orientation, orientation_tiff};

pub(super) const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

const IHDR: u32 = fourcc(b"IHDR");
const IEND: u32 = fourcc(b"IEND");
const EXIF: u32 = fourcc(b"eXIf");

/// Ancillary chunks that change how the image looks or animates. The rest of
/// the ancillary chunks go: text (`tEXt`, `zTXt`, `iTXt`), `eXIf`, `tIME`, and
/// any private chunk (Apple's `iDOT` holds offsets that dropping chunks would
/// break). Critical chunks always stay.
const KEPT_ANCILLARY: [&[u8; 4]; 16] = [
    b"tRNS", b"cHRM", b"gAMA", b"iCCP", b"sBIT", b"sRGB", b"cICP", b"mDCV", b"cLLI", b"bKGD",
    b"hIST", b"pHYs", b"sPLT", b"acTL", b"fcTL", b"fdAT",
];

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// CRC-32 (ISO 3309), as PNG computes it over a chunk's type and data.
pub(super) fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8)
    })
}

/// Append a PNG chunk with its length and CRC.
pub(super) fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let typed = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[typed..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

pub(super) fn strip(bytes: &[u8]) -> Result<(Vec<u8>, Option<ExifOrientation>), &'static str> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..8]);
    let mut orientation = None;
    let mut pos = 8;
    loop {
        if pos + 12 > bytes.len() {
            return Err("ends before IEND");
        }
        let length = u32be(bytes, pos);
        let kind: &[u8; 4] = bytes[pos + 4..pos + 8].try_into().unwrap_or(&[0; 4]);
        let end = pos as u64 + 12 + u64::from(length);
        if length > 0x7fff_ffff || end > bytes.len() as u64 {
            return Err("a chunk runs past the end");
        }
        let end = end as usize;
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err("invalid chunk type");
        }
        let kind_code = fourcc(kind);
        if pos == 8 && kind_code != IHDR {
            return Err("does not start with IHDR");
        }
        // Bit 5 of the first type byte clear: a critical chunk, needed to decode.
        let critical = kind[0] & 0x20 == 0;
        if kind_code == EXIF {
            if orientation.is_none() {
                orientation = exif_orientation(&bytes[pos + 8..end - 4]);
                if let Some(found) = orientation {
                    push_chunk(&mut out, b"eXIf", &orientation_tiff(found));
                }
            }
        } else if critical || KEPT_ANCILLARY.contains(&kind) {
            out.extend_from_slice(&bytes[pos..end]);
        }
        pos = end;
        if kind_code == IEND {
            break;
        }
    }
    Ok((out, orientation))
}
