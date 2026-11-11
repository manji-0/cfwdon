//! WebM / Matroska (EBML). A metadata element is overwritten where it is by
//! a `Void` element of the same length with a zeroed payload, so SeekHead
//! and Cues positions stay valid and the file keeps its length. Clusters are
//! never read.

use super::bytes::{at, u32be};

type Walk<T> = Result<T, &'static str>;

const EBML: u32 = 0x1a45_dfa3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114d_9b74;
const SEEK: u32 = 0x4dbb;
const SEEK_ID: u32 = 0x53ab;
const INFO: u32 = 0x1549_a966;
const TRACKS: u32 = 0x1654_ae6b;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_NAME: u32 = 0x536e;
const CLUSTER: u32 = 0x1f43_b675;
const CUES: u32 = 0x1c53_bb6b;
const CHAPTERS: u32 = 0x1043_a770;
const TAGS: u32 = 0x1254_c367;
const ATTACHMENTS: u32 = 0x1941_a469;
const VOID: u32 = 0xec;
const CRC32: u32 = 0xbf;

pub(super) fn starts_like_webm(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && u32be(bytes, 0) == EBML
}

/// Segment children that are only metadata: tags (title, artist, date,
/// location, encoder settings) and attachments (cover art, which can carry
/// its own EXIF, and fonts).
const METADATA_ELEMENTS: [u32; 2] = [TAGS, ATTACHMENTS];

/// Segment information that describes the file rather than how to play it:
/// the title, the date it was made, its file names and the identifiers that
/// link it to sibling files. TimestampScale, Duration, ChapterTranslate,
/// MuxingApp and WritingApp (mandatory, and naming software, not a person or
/// place) stay.
const INFO_METADATA: [u32; 9] = [
    0x7ba9,    // Title
    0x4461,    // DateUTC
    0x7384,    // SegmentFilename
    0x3c_83ab, // PrevFilename
    0x3e_83bb, // NextFilename
    0x73a4,    // SegmentUUID
    0x3c_b923, // PrevUUID
    0x3e_b923, // NextUUID
    0x4444,    // SegmentFamily
];

/// The level-1 elements, which end a Cluster of unknown size.
const SEGMENT_CHILDREN: [u32; 10] = [
    SEEK_HEAD,
    INFO,
    TRACKS,
    CLUSTER,
    CUES,
    CHAPTERS,
    TAGS,
    ATTACHMENTS,
    EBML,
    SEGMENT,
];

#[derive(Clone, Copy, Debug)]
struct Element {
    id: u32,
    start: usize,
    body: usize,
    /// The end of the element; `None` for an unknown size.
    end: Option<u64>,
}

/// An element header, or `Cut` when the header itself runs past the limit.
enum Header {
    Element(Element),
    Cut,
}

/// The length of an EBML variable-size integer from its first byte, 0 when invalid.
fn vint_length(first: u8) -> usize {
    first.leading_zeros() as usize + 1
}

/// The element header at `pos`. A size whose value bits are all ones is unknown.
fn element_at(bytes: &[u8], pos: usize, limit: usize) -> Walk<Header> {
    if pos >= limit {
        return Ok(Header::Cut);
    }
    let id_length = vint_length(at(bytes, pos));
    if id_length > 4 {
        return Err("an invalid element id");
    }
    if pos + id_length >= limit {
        return Ok(Header::Cut);
    }
    let id = (0..id_length).fold(0u32, |id, i| (id << 8) | u32::from(bytes[pos + i]));
    let size_at = pos + id_length;
    let size_length = vint_length(at(bytes, size_at));
    if size_length > 8 {
        return Err("an invalid element size");
    }
    if size_at + size_length > limit {
        return Ok(Header::Cut);
    }
    let mask = 0xffu8.checked_shr(size_length as u32).unwrap_or(0);
    let first = bytes[size_at] & mask;
    let mut all_ones = first == mask;
    let mut size = u64::from(first);
    for &byte in &bytes[size_at + 1..size_at + size_length] {
        all_ones &= byte == 0xff;
        size = (size << 8) | u64::from(byte);
    }
    let body = size_at + size_length;
    Ok(Header::Element(Element {
        id,
        start: pos,
        body,
        end: (!all_ones).then(|| body as u64 + size),
    }))
}

/// The children of a master element of known size, which must fit in it.
fn element_children(bytes: &[u8], parent: Element) -> Walk<Vec<Element>> {
    let end = parent.end.ok_or("an element of unknown size")? as usize;
    let mut found = Vec::new();
    let mut pos = parent.body;
    while pos < end {
        let child = match element_at(bytes, pos, end)? {
            Header::Element(child) if child.end.is_some_and(|e| e <= end as u64) => child,
            _ => return Err("an element runs past its parent"),
        };
        found.push(child);
        pos = child.end.unwrap_or_default() as usize;
    }
    Ok(found)
}

/// Overwrite an element with a `Void` element of the same total length and
/// a zeroed payload. The Void's size field takes the bytes the old ID and
/// size took, less the one-byte Void ID, up to the eight bytes a size can
/// have.
fn void_element(out: &mut [u8], element: Element) -> Walk<()> {
    let end = element.end.ok_or("an element of unknown size")? as usize;
    let size_length = (element.body - element.start - 1).min(8);
    let mut size = (end - element.start - 1 - size_length) as u64;
    out[element.start] = VOID as u8;
    out[element.start + 1..end].fill(0);
    for i in (1..=size_length).rev() {
        out[element.start + i] = (size & 0xff) as u8;
        size >>= 8;
    }
    out[element.start + 1] |= 0x80 >> (size_length - 1);
    Ok(())
}

/// Void the children of `master` that `should_void` picks, and its CRC-32,
/// which would no longer match. Whether anything changed.
fn void_children(
    bytes: &[u8],
    out: &mut [u8],
    master: Element,
    should_void: impl Fn(Element) -> Walk<bool>,
) -> Walk<bool> {
    let element_children = element_children(bytes, master)?;
    let mut voided = false;
    for &child in &element_children {
        if should_void(child)? {
            void_element(out, child)?;
            voided = true;
        }
    }
    if voided {
        for &crc in element_children.iter().filter(|child| child.id == CRC32) {
            void_element(out, crc)?;
        }
    }
    Ok(voided)
}

fn element_id_value(bytes: &[u8], element: Element) -> u64 {
    let end = element.end.unwrap_or(element.body as u64) as usize;
    bytes[element.body..end]
        .iter()
        .fold(0u64, |id, &byte| (id << 8) | u64::from(byte))
}

/// A Seek entry that points at an element this stripper voids.
fn seeks_metadata(bytes: &[u8], seek: Element) -> Walk<bool> {
    if seek.id != SEEK {
        return Ok(false);
    }
    Ok(element_children(bytes, seek)?.into_iter().any(|child| {
        child.id == SEEK_ID
            && METADATA_ELEMENTS
                .iter()
                .any(|&id| u64::from(id) == element_id_value(bytes, child))
    }))
}

/// Where a Cluster of unknown size (as a live recorder such as the browser's
/// MediaRecorder writes) ends: at the next level-1 element, or the end of
/// the segment. A last block cut off by the end of the file ends it too.
fn unknown_cluster_end(bytes: &[u8], cluster: Element, limit: usize) -> Walk<usize> {
    let mut pos = cluster.body;
    while pos < limit {
        let Header::Element(child) = element_at(bytes, pos, limit)? else {
            return Ok(limit);
        };
        if SEGMENT_CHILDREN.contains(&child.id) {
            return Ok(pos);
        }
        let end = child.end.ok_or("an element of unknown size in a cluster")?;
        if end >= limit as u64 {
            return Ok(limit);
        }
        pos = end as usize;
    }
    Ok(limit)
}

/// The segment: Tags and Attachments become Void, as do the descriptive
/// Info fields, track names and the SeekHead entries pointing at what was
/// voided. Clusters, Cues, Tracks and Chapters stay. A Cluster cut off by
/// the end of the file is kept, as a browser plays it as far as it goes.
/// Where the segment ends.
fn walk_segment(bytes: &[u8], out: &mut [u8], segment: Element) -> Walk<usize> {
    let limit = segment
        .end
        .map_or(bytes.len(), |end| end.min(bytes.len() as u64) as usize);
    let mut saw_tracks = false;
    let mut changed = false;
    let mut checksums = Vec::new();
    let mut pos = segment.body;
    while pos < limit {
        let Header::Element(child) = element_at(bytes, pos, limit)? else {
            return Err("an element header runs past the end");
        };
        if segment.end.is_none() && (child.id == EBML || child.id == SEGMENT) {
            // A segment of unknown size ends where the next file is chained on.
            return Ok(pos);
        }
        if child.id == CLUSTER {
            pos = match child.end {
                None => unknown_cluster_end(bytes, child, limit)?,
                Some(end) => end.min(limit as u64) as usize,
            };
            continue;
        }
        let end = child
            .end
            .filter(|&end| end <= limit as u64)
            .ok_or("an element runs past the segment")? as usize;
        if METADATA_ELEMENTS.contains(&child.id) {
            void_element(out, child)?;
            changed = true;
        } else if child.id == INFO {
            changed |= void_children(bytes, out, child, |field| {
                Ok(INFO_METADATA.contains(&field.id))
            })?;
        } else if child.id == TRACKS {
            saw_tracks = true;
            let mut renamed = false;
            for entry in element_children(bytes, child)? {
                if entry.id == TRACK_ENTRY {
                    renamed |=
                        void_children(bytes, out, entry, |field| Ok(field.id == TRACK_NAME))?;
                }
            }
            if renamed {
                void_children(bytes, out, child, |entry| Ok(entry.id == CRC32))?;
                changed = true;
            }
        } else if child.id == SEEK_HEAD {
            changed |= void_children(bytes, out, child, |seek| seeks_metadata(bytes, seek))?;
        } else if child.id == CRC32 {
            checksums.push(child);
        }
        pos = end;
    }
    if !saw_tracks {
        return Err("a segment has no Tracks");
    }
    if changed {
        for crc in checksums {
            void_element(out, crc)?;
        }
    }
    Ok(limit)
}

/// Whether the DocType of an EBML header names Matroska or WebM.
fn is_matroska_header(bytes: &[u8], header: Element) -> Walk<bool> {
    Ok(element_children(bytes, header)?.into_iter().any(|child| {
        let Some(end) = child.end.map(|end| end as usize) else {
            return false;
        };
        if child.id != DOC_TYPE || end - child.body > 16 {
            return false;
        }
        let doc_type = &bytes[child.body..end];
        let doc_type = &doc_type[..doc_type
            .iter()
            .rposition(|&b| b != 0)
            .map_or(0, |last| last + 1)];
        doc_type == b"webm" || doc_type == b"matroska"
    }))
}

pub(super) fn strip(bytes: &[u8]) -> Walk<Vec<u8>> {
    let mut out = bytes.to_vec();
    let mut saw_segment = false;
    let mut pos = 0;
    while pos < bytes.len() {
        let Header::Element(element) = element_at(bytes, pos, bytes.len())? else {
            return Err("an element header runs past the end");
        };
        if element.id == SEGMENT {
            saw_segment = true;
            pos = walk_segment(bytes, &mut out, element)?;
            continue;
        }
        let end = element
            .end
            .filter(|&end| end <= bytes.len() as u64)
            .ok_or("an element runs past the end")? as usize;
        if element.id == EBML {
            if !is_matroska_header(bytes, element)? {
                return Err("not a Matroska or WebM document");
            }
        } else if element.id != VOID && element.id != CRC32 {
            return Err("an unknown top-level element");
        }
        pos = end;
    }
    if !saw_segment {
        return Err("has no segment");
    }
    Ok(out)
}
