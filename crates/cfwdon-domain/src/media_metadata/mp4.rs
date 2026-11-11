//! MP4 / QuickTime (ISO BMFF). Nothing moves: a metadata box is overwritten
//! where it is by a `free` box of the same size with a zeroed payload, so
//! chunk offsets (`stco` / `co64`), fragment offsets and `iloc` extents stay
//! valid and the file keeps its length. Sample data is never read.

use std::collections::{HashMap, HashSet};

use super::bytes::{at, fourcc, u16be, u32be, uint_be};
use super::edits::Edits;
use super::heif::{IsoBox, blank_metadata_items};

type Walk<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Debug)]
struct Mp4Box {
    kind: u32,
    start: usize,
    body: usize,
    end: usize,
    /// Where the box says it ends, past `end` for an `mdat` cut short.
    declared_end: u64,
}

const FTYP: u32 = fourcc(b"ftyp");
const MOOV: u32 = fourcc(b"moov");
const MDAT: u32 = fourcc(b"mdat");
const MOOF: u32 = fourcc(b"moof");
const TRAK: u32 = fourcc(b"trak");
const TRAF: u32 = fourcc(b"traf");
const MDIA: u32 = fourcc(b"mdia");
const MINF: u32 = fourcc(b"minf");
const STBL: u32 = fourcc(b"stbl");
const MVEX: u32 = fourcc(b"mvex");
const TREX: u32 = fourcc(b"trex");
const TKHD: u32 = fourcc(b"tkhd");
const MVHD: u32 = fourcc(b"mvhd");
const MDHD: u32 = fourcc(b"mdhd");
const HDLR: u32 = fourcc(b"hdlr");
const TFHD: u32 = fourcc(b"tfhd");
const TRUN: u32 = fourcc(b"trun");
const STSC: u32 = fourcc(b"stsc");
const STSZ: u32 = fourcc(b"stsz");
const STZ2: u32 = fourcc(b"stz2");
const STCO: u32 = fourcc(b"stco");
const CO64: u32 = fourcc(b"co64");
const META: u32 = fourcc(b"meta");
const UUID: u32 = fourcc(b"uuid");
const PICT: u32 = fourcc(b"pict");

/// Top-level types a QuickTime file without `ftyp` may start with.
const FIRST_BOXES: [u32; 7] = [
    FTYP,
    MOOV,
    MDAT,
    fourcc(b"free"),
    fourcc(b"skip"),
    fourcc(b"wide"),
    fourcc(b"pnot"),
];

pub(super) fn starts_like_mp4(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && FIRST_BOXES.contains(&u32be(bytes, 4))
}

/// Boxes that hold only metadata, wherever the walk meets them: user data
/// (`©xyz` location, `©mak` / `©mod` device, `©day`, GoPro's `GPMF`, `XMP_`,
/// iTunes `meta` / `ilst`), QuickTime and ISO `meta` (the
/// `com.apple.quicktime.location.ISO6709`, make, model and creation date
/// keys), the additional metadata container, Adobe's `XMP_`, and the
/// producer reference time (a wall-clock timestamp).
const METADATA_BOXES: [u32; 5] = [
    fourcc(b"udta"),
    META,
    fourcc(b"meco"),
    fourcc(b"XMP_"),
    fourcc(b"prft"),
];

/// Boxes whose creation and modification times are zeroed.
const TIMESTAMPED: [u32; 3] = [MVHD, TKHD, MDHD];

/// Track handlers whose samples are timed metadata, not picture or sound:
/// ISO timed metadata (GoPro's `gpmd` GPS and sensor track, DJI's `djmd`,
/// Apple's `mebx`) and Google's camera motion metadata (`camm`, which
/// carries GPS fixes).
const METADATA_TRACK_HANDLERS: [u32; 2] = [META, fourcc(b"camm")];

/// `uuid` boxes known to hold metadata wherever they are: XMP, Canon's CMT
/// (EXIF with GPS), Sony's user metadata. Other `uuid` boxes below the top
/// level (PIFF encryption and timing boxes, a `pssh`) can be needed to play.
const METADATA_UUIDS: [[u8; 16]; 3] = [
    *b"\xbe\x7a\xcf\xcb\x97\xa9\x42\xe8\x9c\x71\x99\x94\x91\xe3\xaf\xac",
    *b"\x85\xc0\xb6\x87\x82\x0f\x11\xe0\x81\x11\xf4\xce\x46\x2b\x6a\x48",
    *b"\x55\x53\x4d\x54\x21\xd2\x4f\xce\xbb\x88\x69\x5c\xfa\xc9\xc7\x40",
];

/// Containers nest a handful of levels; anything deeper is not a video.
const MAX_DEPTH: usize = 8;

/// The boxes directly inside `[start, end)`. At the top level the last box
/// may be an `mdat` cut short by the end of the file (an interrupted upload
/// or recording), which a browser still plays as far as it goes; it then
/// ends at the end of the file.
fn boxes(bytes: &[u8], start: usize, end: usize, top: bool) -> Walk<Vec<Mp4Box>> {
    let mut found = Vec::new();
    let mut pos = start;
    while pos < end {
        if pos + 8 > end {
            return Err("a box header runs past the end");
        }
        let size32 = u32be(bytes, pos);
        let kind = u32be(bytes, pos + 4);
        let large = size32 == 1;
        if large && pos + 16 > end {
            return Err("a box header runs past the end");
        }
        let body = pos + if large { 16 } else { 8 };
        let size = match size32 {
            1 => uint_be(bytes, pos + 8, 8),
            0 => (end - pos) as u64,
            _ => u64::from(size32),
        };
        if size < (body - pos) as u64 {
            return Err("a box is smaller than its header");
        }
        let declared_end = (pos as u64).saturating_add(size);
        if declared_end > end as u64 {
            if !(top && kind == MDAT) {
                return Err("a box runs past the end");
            }
            found.push(Mp4Box {
                kind,
                start: pos,
                body,
                end,
                declared_end,
            });
            break;
        }
        let box_end = declared_end as usize;
        found.push(Mp4Box {
            kind,
            start: pos,
            body,
            end: box_end,
            declared_end,
        });
        pos = box_end;
    }
    Ok(found)
}

fn children(bytes: &[u8], parent: Mp4Box) -> Walk<Vec<Mp4Box>> {
    boxes(bytes, parent.body, parent.end, false)
}

fn first_child(boxes: &[Mp4Box], kind: u32) -> Option<Mp4Box> {
    boxes.iter().copied().find(|child| child.kind == kind)
}

/// Turn a box into a `free` box of the same size with a zeroed payload.
fn neutralize(edits: &mut Edits, target: Mp4Box) {
    edits.write(target.start + 4, b"free");
    edits.fill(target.body..target.end, 0);
}

fn is_metadata_uuid(bytes: &[u8], target: Mp4Box) -> bool {
    target.body + 16 <= target.end
        && METADATA_UUIDS
            .iter()
            .any(|uuid| bytes[target.body..target.body + 16] == *uuid)
}

/// Whether a box below the top level holds only metadata.
fn is_metadata_box(bytes: &[u8], target: Mp4Box) -> bool {
    METADATA_BOXES.contains(&target.kind)
        // `©xyz`, `©day`, `©mak` and the other QuickTime text atoms.
        || at(bytes, target.start + 4) == 0xa9
        || (target.kind == UUID && is_metadata_uuid(bytes, target))
}

/// The version byte of a full box, after checking its fields fit.
fn full_box_version(bytes: &[u8], target: Mp4Box, fields: impl Fn(u8) -> usize) -> Walk<u8> {
    let version = at(bytes, target.body);
    if target.body + 4 + fields(version) > target.end {
        return Err("a header box is too short");
    }
    Ok(version)
}

/// Zero the creation and modification times of `mvhd`, `tkhd` or `mdhd`
/// (seconds since 1904, 32 bits in version 0, 64 in version 1), as ffmpeg
/// writes them when the source's `creation_time` is dropped.
fn zero_times(bytes: &[u8], edits: &mut Edits, target: Mp4Box) -> Walk<()> {
    let width = |version| if version == 1 { 16 } else { 8 };
    let version = full_box_version(bytes, target, width)?;
    edits.fill(target.body + 4..target.body + 4 + width(version), 0);
    Ok(())
}

/// The handler type of a `meta` box, QuickTime style (no version) or ISO style.
fn meta_handler(bytes: &[u8], meta: Mp4Box) -> Option<u32> {
    [4, 0].into_iter().find_map(|offset| {
        let hdlr = meta.body + offset;
        (hdlr + 20 <= meta.end && u32be(bytes, hdlr + 4) == HDLR).then(|| u32be(bytes, hdlr + 16))
    })
}

struct Mp4Walk<'a> {
    bytes: &'a [u8],
    edits: Edits,
    /// Top-level `mdat` payloads, in file order, that metadata samples must
    /// lie in; each ends where its header says, which may be past the file's
    /// end.
    mdats: Vec<(u64, u64)>,
    /// Track ids of the timed metadata tracks taken out of `moov`.
    metadata_tracks: HashSet<u32>,
    /// `trex` default sample sizes by track id, for fragments.
    default_sample_sizes: HashMap<u32, u32>,
}

impl Mp4Walk<'_> {
    /// Zero `[start, start + length)`, which must lie within one `mdat`.
    fn zero_samples(&mut self, start: u64, length: u64) -> Walk<()> {
        if length == 0 {
            return Ok(());
        }
        let end = start.saturating_add(length);
        // The last mdat starting at or before `start` (they are in file order).
        let index = self
            .mdats
            .partition_point(|&(mdat_start, _)| mdat_start <= start);
        let inside = index
            .checked_sub(1)
            .and_then(|i| self.mdats.get(i))
            .is_some_and(|&(mdat_start, mdat_end)| start >= mdat_start && end <= mdat_end);
        if !inside {
            return Err("a metadata sample lies outside mdat");
        }
        // Samples of an mdat cut short are zeroed as far as the file goes.
        let len = self.bytes.len() as u64;
        self.edits
            .fill(start.min(len) as usize..end.min(len) as usize, 0);
        Ok(())
    }

    /// Zero the samples of a track from its sample table: one range per
    /// chunk, as the samples of a chunk are contiguous. Time is linear in
    /// the number of chunks and samples listed.
    fn zero_track_samples(&mut self, stbl: Mp4Box) -> Walk<()> {
        let bytes = self.bytes;
        let tables = children(bytes, stbl)?;
        let stsc = first_child(&tables, STSC);
        let sizes = first_child(&tables, STSZ).or_else(|| first_child(&tables, STZ2));
        let chunks = first_child(&tables, STCO).or_else(|| first_child(&tables, CO64));
        let (Some(stsc), Some(sizes), Some(chunks)) = (stsc, sizes, chunks) else {
            if stsc.is_some() || sizes.is_some() || chunks.is_some() {
                return Err("an incomplete sample table");
            }
            return Ok(());
        };
        let fits = |table: Mp4Box, header: u64, count: u64, entry_bits: u64| {
            (table.body as u64 + header + (count * entry_bits).div_ceil(8)) <= table.end as u64
        };
        let offset_size = if chunks.kind == CO64 { 8 } else { 4 };
        let chunk_count = u32be(bytes, chunks.body + 4);
        if !fits(chunks, 8, u64::from(chunk_count), offset_size as u64 * 8) {
            return Err("a chunk table runs past its box");
        }
        let stsc_count = u32be(bytes, stsc.body + 4) as usize;
        if !fits(stsc, 8, stsc_count as u64, 96) {
            return Err("a sample-to-chunk table runs past its box");
        }
        let compact = sizes.kind == STZ2;
        let field_bits = if compact {
            at(bytes, sizes.body + 7)
        } else {
            32
        };
        let constant_size = if compact {
            0
        } else {
            u32be(bytes, sizes.body + 4)
        };
        let sample_count = u32be(bytes, sizes.body + 8);
        if compact && !matches!(field_bits, 4 | 8 | 16) {
            return Err("an unknown compact sample size");
        }
        if sizes.body + 12 > sizes.end
            || (constant_size == 0
                && !fits(sizes, 12, u64::from(sample_count), u64::from(field_bits)))
        {
            return Err("a sample size table runs past its box");
        }
        let table = sizes.body + 12;
        let sample_size = |index: usize| -> u64 {
            if constant_size != 0 {
                return u64::from(constant_size);
            }
            u64::from(match field_bits {
                32 => u32be(bytes, table + index * 4),
                16 => u32::from(u16be(bytes, table + index * 2)),
                8 => u32::from(at(bytes, table + index)),
                _ => {
                    let byte = at(bytes, table + (index >> 1));
                    u32::from(if index.is_multiple_of(2) {
                        byte >> 4
                    } else {
                        byte & 0xf
                    })
                }
            })
        };
        let entry_at = |entry: usize| stsc.body + 8 + entry * 12;
        let sample_count = sample_count as usize;
        let mut entry = 0;
        let mut sample = 0;
        let mut chunk = 1u32;
        while chunk <= chunk_count && sample < sample_count {
            while entry + 1 < stsc_count && u32be(bytes, entry_at(entry + 1)) <= chunk {
                entry += 1;
            }
            let per_chunk = if stsc_count == 0 || u32be(bytes, entry_at(entry)) > chunk {
                0
            } else {
                u32be(bytes, entry_at(entry) + 4) as usize
            };
            let in_chunk = per_chunk.min(sample_count - sample);
            let length: u64 = (sample..sample + in_chunk).map(sample_size).sum();
            sample += in_chunk;
            let offset = uint_be(
                bytes,
                chunks.body + 8 + (chunk as usize - 1) * offset_size,
                offset_size,
            );
            self.zero_samples(offset, length)?;
            chunk += 1;
        }
        Ok(())
    }

    /// Walk a container below `moov` or `traf`, neutralizing metadata.
    fn walk_container(&mut self, container: Mp4Box, depth: usize) -> Walk<()> {
        if depth > MAX_DEPTH {
            return Err("boxes nest too deep");
        }
        for child in children(self.bytes, container)? {
            if is_metadata_box(self.bytes, child) {
                neutralize(&mut self.edits, child);
            } else if TIMESTAMPED.contains(&child.kind) {
                zero_times(self.bytes, &mut self.edits, child)?;
            } else if child.kind == MDIA || child.kind == MINF {
                self.walk_container(child, depth + 1)?;
            }
        }
        Ok(())
    }

    /// A track: a timed metadata track is taken out whole (its `trak`
    /// becomes `free`) and its samples in `mdat` are zeroed; any other track
    /// keeps everything but its user data, `meta` and times.
    fn walk_track(&mut self, trak: Mp4Box, depth: usize) -> Walk<()> {
        let bytes = self.bytes;
        let track_children = children(bytes, trak)?;
        let mdia_children = match first_child(&track_children, MDIA) {
            Some(mdia) => children(bytes, mdia)?,
            None => Vec::new(),
        };
        let handler = first_child(&mdia_children, HDLR)
            .filter(|hdlr| hdlr.body + 12 <= hdlr.end)
            .map(|hdlr| u32be(bytes, hdlr.body + 8));
        if !handler.is_some_and(|handler| METADATA_TRACK_HANDLERS.contains(&handler)) {
            return self.walk_container(trak, depth + 1);
        }
        if let Some(tkhd) = first_child(&track_children, TKHD) {
            let version = full_box_version(bytes, tkhd, |v| if v == 1 { 20 } else { 12 })?;
            let id_at = tkhd.body + if version == 1 { 20 } else { 12 };
            self.metadata_tracks.insert(u32be(bytes, id_at));
        }
        let stbl = match first_child(&mdia_children, MINF) {
            Some(minf) => first_child(&children(bytes, minf)?, STBL),
            None => None,
        };
        if let Some(stbl) = stbl {
            self.zero_track_samples(stbl)?;
        }
        neutralize(&mut self.edits, trak);
        Ok(())
    }

    fn walk_moov(&mut self, moov: Mp4Box) -> Walk<()> {
        let bytes = self.bytes;
        let moov_children = children(bytes, moov)?;
        for mvex in moov_children.iter().filter(|child| child.kind == MVEX) {
            for trex in children(bytes, *mvex)?
                .into_iter()
                .filter(|child| child.kind == TREX)
            {
                if trex.body + 24 > trex.end {
                    return Err("trex is too short");
                }
                self.default_sample_sizes
                    .insert(u32be(bytes, trex.body + 4), u32be(bytes, trex.body + 16));
            }
        }
        for child in moov_children {
            if child.kind == TRAK {
                self.walk_track(child, 1)?;
            } else if is_metadata_box(bytes, child) {
                neutralize(&mut self.edits, child);
            } else if TIMESTAMPED.contains(&child.kind) {
                zero_times(bytes, &mut self.edits, child)?;
            }
        }
        Ok(())
    }

    /// Where a track fragment's data ends, after zeroing it when the track
    /// is a timed metadata track. `implicit_base` is where its data counts
    /// from when `tfhd` gives no base: the `moof` for the first fragment, the
    /// end of the previous fragment's data after that (ISO/IEC 14496-12,
    /// 8.8.7).
    fn fragment_data(&mut self, moof: Mp4Box, traf: Mp4Box, implicit_base: u64) -> Walk<u64> {
        let bytes = self.bytes;
        let traf_children = children(bytes, traf)?;
        let tfhd = first_child(&traf_children, TFHD)
            .filter(|tfhd| tfhd.body + 8 <= tfhd.end)
            .ok_or("a track fragment has no tfhd")?;
        let flags = flags_of(bytes, tfhd);
        let track = u32be(bytes, tfhd.body + 4);
        let mut field = tfhd.body + 8;
        let mut read = |size: usize| -> Walk<u64> {
            if field + size > tfhd.end {
                return Err("tfhd is too short");
            }
            let value = uint_be(bytes, field, size);
            field += size;
            Ok(value)
        };
        let base = if flags & TFHD_BASE_DATA_OFFSET != 0 {
            read(8)?
        } else if flags & TFHD_DEFAULT_BASE_IS_MOOF != 0 {
            moof.start as u64
        } else {
            implicit_base
        };
        if flags & TFHD_SAMPLE_DESCRIPTION_INDEX != 0 {
            read(4)?;
        }
        if flags & TFHD_DEFAULT_DURATION != 0 {
            read(4)?;
        }
        let default_size = if flags & TFHD_DEFAULT_SIZE != 0 {
            Some(read(4)?)
        } else {
            self.default_sample_sizes
                .get(&track)
                .map(|&size| u64::from(size))
        };
        let metadata = self.metadata_tracks.contains(&track);
        let mut cursor = base;
        for trun in traf_children.into_iter().filter(|child| child.kind == TRUN) {
            let run_flags = flags_of(bytes, trun);
            let count = u64::from(u32be(bytes, trun.body + 4));
            let mut pos = trun.body + 8;
            let mut start = cursor;
            if run_flags & TRUN_DATA_OFFSET != 0 {
                // A signed offset from the base.
                let offset = i64::from(u32be(bytes, pos) as i32);
                start = base
                    .checked_add_signed(offset)
                    .ok_or("a track run starts before the file")?;
                pos += 4;
            }
            if run_flags & TRUN_FIRST_SAMPLE_FLAGS != 0 {
                pos += 4;
            }
            let fields: Vec<u32> = TRUN_SAMPLE_FIELDS
                .into_iter()
                .filter(|bit| run_flags & bit != 0)
                .collect();
            let stride = fields.len() * 4;
            if pos as u64 + count * stride as u64 > trun.end as u64 {
                return Err("a track run runs past its box");
            }
            let length = if run_flags & TRUN_SAMPLE_SIZE != 0 {
                let size_at = fields
                    .iter()
                    .position(|&bit| bit == TRUN_SAMPLE_SIZE)
                    .unwrap_or(0)
                    * 4;
                (0..count as usize)
                    .map(|i| u64::from(u32be(bytes, pos + i * stride + size_at)))
                    .sum()
            } else if count > 0 {
                count * default_size.ok_or("a track run has no sample size")?
            } else {
                0
            };
            if metadata {
                self.zero_samples(start, length)?;
            }
            cursor = start.saturating_add(length);
        }
        Ok(cursor)
    }

    /// A movie fragment: metadata boxes go, the fragments of timed metadata
    /// tracks become `free` and their samples are zeroed. Sample positions
    /// are only worked out when there is such a track.
    fn walk_moof(&mut self, moof: Mp4Box) -> Walk<()> {
        let bytes = self.bytes;
        let mut implicit_base = moof.start as u64;
        for child in children(bytes, moof)? {
            if child.kind == TRAF {
                if !self.metadata_tracks.is_empty() {
                    implicit_base = self.fragment_data(moof, child, implicit_base)?;
                    let tfhd = first_child(&children(bytes, child)?, TFHD);
                    if tfhd.is_some_and(|tfhd| {
                        self.metadata_tracks.contains(&u32be(bytes, tfhd.body + 4))
                    }) {
                        neutralize(&mut self.edits, child);
                        continue;
                    }
                }
                self.walk_container(child, 2)?;
            } else if is_metadata_box(bytes, child) {
                neutralize(&mut self.edits, child);
            }
        }
        Ok(())
    }
}

const TFHD_BASE_DATA_OFFSET: u32 = 0x1;
const TFHD_SAMPLE_DESCRIPTION_INDEX: u32 = 0x2;
const TFHD_DEFAULT_DURATION: u32 = 0x8;
const TFHD_DEFAULT_SIZE: u32 = 0x10;
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x20000;
const TRUN_DATA_OFFSET: u32 = 0x1;
const TRUN_FIRST_SAMPLE_FLAGS: u32 = 0x4;
const TRUN_SAMPLE_FIELDS: [u32; 4] = [0x100, 0x200, 0x400, 0x800];
const TRUN_SAMPLE_SIZE: u32 = 0x200;

fn flags_of(bytes: &[u8], target: Mp4Box) -> u32 {
    (u32::from(at(bytes, target.body + 1)) << 16) | u32::from(u16be(bytes, target.body + 2))
}

pub(super) fn strip(bytes: &[u8]) -> Walk<Edits> {
    let top = boxes(bytes, 0, bytes.len(), true)?;
    if !top.iter().any(|child| child.kind == MOOV) {
        return Err("has no movie box");
    }
    let mut walk = Mp4Walk {
        bytes,
        edits: Edits::default(),
        mdats: top
            .iter()
            .filter(|child| child.kind == MDAT)
            .map(|mdat| (mdat.body as u64, mdat.declared_end))
            .collect(),
        metadata_tracks: HashSet::new(),
        default_sample_sizes: HashMap::new(),
    };
    for moov in top.iter().filter(|child| child.kind == MOOV) {
        walk.walk_moov(*moov)?;
    }
    for &child in &top {
        if child.kind == MOOF {
            walk.walk_moof(child)?;
        } else if child.kind == META && meta_handler(bytes, child) == Some(PICT) {
            // A HEIF-style item store: its EXIF and XMP items are blanked in place.
            let meta = IsoBox {
                kind: child.kind,
                body: child.body,
                end: child.end,
            };
            blank_metadata_items(bytes, &mut walk.edits, meta)?;
        } else if child.kind == UUID || (child.kind != FTYP && is_metadata_box(bytes, child)) {
            // No top-level `uuid` box is needed to play: XMP, Sony's profile
            // and user metadata, a camera's own blocks.
            neutralize(&mut walk.edits, child);
        }
    }
    Ok(walk.edits)
}
