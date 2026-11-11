//! HEIF images (AVIF, HEIC) and the HEIF-style item store an MP4 can carry
//! at file level. Both keep EXIF and XMP as items whose bytes `iloc` locates,
//! usually in `mdat`. Removing them would mean rewriting every offset, so
//! their bytes are overwritten in place instead: an EXIF item becomes an
//! empty IFD (readers that insist on parsing it still can), an XMP item
//! becomes spaces. The orientation of a HEIF image is its `irot` / `imir`
//! properties, which stay.

use std::collections::HashMap;

use super::bytes::{at, end_within, fourcc, starts_with, u16be, u32be, uint_be};
use super::edits::Edits;

/// An ISOBMFF box: its type, where its payload starts, and where it ends.
#[derive(Clone, Copy, Debug)]
pub(super) struct IsoBox {
    pub(super) kind: u32,
    pub(super) body: usize,
    pub(super) end: usize,
}

/// The ISOBMFF box at `pos`, or `None` when its size does not fit within `limit`.
pub(super) fn box_at(bytes: &[u8], pos: usize, limit: usize) -> Option<IsoBox> {
    if pos + 8 > limit {
        return None;
    }
    let size32 = u32be(bytes, pos);
    let kind = u32be(bytes, pos + 4);
    let large = size32 == 1;
    if large && pos + 16 > limit {
        return None;
    }
    let size = match size32 {
        1 => uint_be(bytes, pos + 8, 8),
        0 => (limit - pos) as u64,
        _ => u64::from(size32),
    };
    let header = if large { 16 } else { 8 };
    let end = end_within(pos, size, limit)?;
    (size >= header).then_some(IsoBox {
        kind,
        body: pos + header as usize,
        end,
    })
}

/// The boxes directly inside `[start, end)`; `None` when one does not fit.
pub(super) fn child_boxes(bytes: &[u8], start: usize, end: usize) -> Option<Vec<IsoBox>> {
    let mut boxes = Vec::new();
    let mut pos = start;
    while pos < end {
        let child = box_at(bytes, pos, end)?;
        boxes.push(child);
        pos = child.end;
    }
    Some(boxes)
}

const FTYP: u32 = fourcc(b"ftyp");
const META: u32 = fourcc(b"meta");
const IINF: u32 = fourcc(b"iinf");
const INFE: u32 = fourcc(b"infe");
const ILOC: u32 = fourcc(b"iloc");
const IDAT: u32 = fourcc(b"idat");
const EXIF_ITEM: u32 = fourcc(b"Exif");
const MIME_ITEM: u32 = fourcc(b"mime");

/// Brands of a HEIF still image: AVIF (and its sequences), HEIC, and the
/// generic HEIF image brand.
const IMAGE_BRANDS: [u32; 7] = [
    fourcc(b"avif"),
    fourcc(b"avis"),
    fourcc(b"heic"),
    fourcc(b"heix"),
    fourcc(b"heim"),
    fourcc(b"heis"),
    fourcc(b"mif1"),
];

/// Whether `bytes` starts with an `ftyp` box naming a HEIF image brand, as
/// the major brand or a compatible one.
pub(super) fn is_heif_image(bytes: &[u8]) -> bool {
    if !starts_with(bytes, 4, b"ftyp") {
        return false;
    }
    let Some(ftyp) = box_at(bytes, 0, bytes.len()) else {
        return false;
    };
    if ftyp.end < 16 {
        return false;
    }
    // The major brand, then the compatible brands after the minor version.
    std::iter::once(8)
        .chain((16..ftyp.end.saturating_sub(3)).step_by(4))
        .any(|pos| IMAGE_BRANDS.contains(&u32be(bytes, pos)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MetadataItem {
    Exif,
    Xmp,
}

/// The `iinf` items that hold EXIF or XMP, by item id.
fn metadata_items(bytes: &[u8], iinf: IsoBox) -> Option<HashMap<u32, MetadataItem>> {
    let version = at(bytes, iinf.body);
    let entries = child_boxes(
        bytes,
        iinf.body + 4 + if version == 0 { 2 } else { 4 },
        iinf.end,
    )?;
    let mut items = HashMap::new();
    for infe in entries {
        let infe_version = at(bytes, infe.body);
        if infe.kind != INFE || infe_version < 2 {
            continue;
        }
        let (id, id_size) = if infe_version == 2 {
            (u32::from(u16be(bytes, infe.body + 4)), 2)
        } else {
            (u32be(bytes, infe.body + 4), 4)
        };
        let type_at = infe.body + 4 + id_size + 2;
        if type_at + 4 > infe.end {
            return None;
        }
        let entry = &bytes[..infe.end];
        match u32be(bytes, type_at) {
            EXIF_ITEM => {
                items.insert(id, MetadataItem::Exif);
            }
            MIME_ITEM => {
                // The item name, a null-terminated string, comes before the content type.
                let name_end = entry[type_at + 4..].iter().position(|&b| b == 0)? + type_at + 4;
                if starts_with(entry, name_end + 1, b"application/rdf+xml\0") {
                    items.insert(id, MetadataItem::Xmp);
                }
            }
            _ => {}
        }
    }
    Some(items)
}

/// A byte range in file offsets.
struct Extent {
    start: u64,
    length: u64,
}

/// The byte ranges `iloc` gives each wanted item, in file offsets. Items
/// stored in `idat` count from its body; an item built from other items
/// (construction method 2) holds no bytes of its own.
fn item_extents(
    bytes: &[u8],
    iloc: IsoBox,
    idat_body: Option<usize>,
    wanted: &HashMap<u32, MetadataItem>,
) -> Option<Vec<(u32, Vec<Extent>)>> {
    let version = at(bytes, iloc.body);
    if version > 2 {
        return None;
    }
    let mut pos = iloc.body + 4;
    let sizes = u16be(bytes, pos);
    let offset_size = usize::from(sizes >> 12);
    let length_size = usize::from((sizes >> 8) & 0xf);
    let base_offset_size = usize::from((sizes >> 4) & 0xf);
    let index_size = if version == 0 {
        0
    } else {
        usize::from(sizes & 0xf)
    };
    if ![offset_size, length_size, base_offset_size, index_size]
        .iter()
        .all(|&size| matches!(size, 0 | 4 | 8))
    {
        return None;
    }
    pos += 2;
    let (count, count_size) = if version < 2 {
        (u32::from(u16be(bytes, pos)), 2)
    } else {
        (u32be(bytes, pos), 4)
    };
    pos += count_size;
    let id_size = if version < 2 { 2 } else { 4 };
    let mut extents = Vec::new();
    for _ in 0..count {
        let fixed = id_size + if version == 0 { 0 } else { 2 } + 2 + base_offset_size + 2;
        if pos + fixed > iloc.end {
            return None;
        }
        let id = if id_size == 2 {
            u32::from(u16be(bytes, pos))
        } else {
            u32be(bytes, pos)
        };
        pos += id_size;
        let method = if version == 0 {
            0
        } else {
            u16be(bytes, pos) & 0xf
        };
        // Construction method, then the data reference index.
        pos += if version == 0 { 2 } else { 4 };
        let base = uint_be(bytes, pos, base_offset_size);
        pos += base_offset_size;
        let extent_count = u16be(bytes, pos);
        pos += 2;
        let mut ranges = Vec::new();
        for _ in 0..extent_count {
            if pos + index_size + offset_size + length_size > iloc.end {
                return None;
            }
            pos += index_size;
            let offset = uint_be(bytes, pos, offset_size);
            pos += offset_size;
            let length = uint_be(bytes, pos, length_size);
            pos += length_size;
            if !wanted.contains_key(&id) || method == 2 {
                continue;
            }
            let origin = if method == 1 { idat_body? as u64 } else { 0 };
            let start = origin.checked_add(base)?.checked_add(offset)?;
            // A length of 0 means the rest of the file.
            let length = if length == 0 {
                (bytes.len() as u64).checked_sub(start)?
            } else {
                length
            };
            ranges.push(Extent { start, length });
        }
        if wanted.contains_key(&id) {
            extents.push((id, ranges));
        }
    }
    Some(extents)
}

/// An EXIF item payload with no tags: a zero TIFF-header offset, then an empty IFD0.
const EMPTY_EXIF_ITEM: [u8; 18] = *b"\0\0\0\0MM\0\x2a\0\0\0\x08\0\0\0\0\0\0";

/// Overwrite the bytes of the EXIF and XMP items that a
/// HEIF-style `meta` box (a full box holding `iinf` and `iloc`) of `bytes`
/// lists.
pub(super) fn blank_metadata_items(
    bytes: &[u8],
    edits: &mut Edits,
    meta: IsoBox,
) -> Result<(), &'static str> {
    let children =
        child_boxes(bytes, meta.body + 4, meta.end).ok_or("the meta boxes do not fit")?;
    let find = |kind| children.iter().copied().find(|child| child.kind == kind);
    let items = match find(IINF) {
        Some(iinf) => metadata_items(bytes, iinf).ok_or("unreadable item info")?,
        None => HashMap::new(),
    };
    if items.is_empty() {
        return Ok(());
    }
    let extents = find(ILOC)
        .and_then(|iloc| item_extents(bytes, iloc, find(IDAT).map(|idat| idat.body), &items))
        .ok_or("unreadable item locations")?;
    for (id, ranges) in extents {
        let item = items[&id];
        for range in &ranges {
            let start = usize::try_from(range.start).map_err(|_| "an item runs past the end")?;
            let end =
                end_within(start, range.length, bytes.len()).ok_or("an item runs past the end")?;
            edits.fill(start..end, if item == MetadataItem::Xmp { b' ' } else { 0 });
        }
        if let Some(first) = ranges.first()
            && item == MetadataItem::Exif
            && first.length >= EMPTY_EXIF_ITEM.len() as u64
        {
            edits.write(first.start as usize, &EMPTY_EXIF_ITEM);
        }
    }
    Ok(())
}

pub(super) fn strip(bytes: &[u8]) -> Result<Edits, &'static str> {
    let top = child_boxes(bytes, 0, bytes.len())
        .filter(|top| top.first().is_some_and(|first| first.kind == FTYP))
        .ok_or("the boxes do not fit the file")?;
    let meta = top
        .iter()
        .copied()
        .find(|child| child.kind == META)
        .ok_or("has no meta box")?;
    let mut edits = Edits::default();
    blank_metadata_items(bytes, &mut edits, meta)?;
    Ok(edits)
}
