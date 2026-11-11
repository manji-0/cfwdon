use super::png::{crc32, push_chunk};
use super::*;

const PHOTO_JPG: &[u8] = include_bytes!("fixtures/photo.jpg");
const PHOTO_PNG: &[u8] = include_bytes!("fixtures/photo.png");
const PHOTO_WEBP: &[u8] = include_bytes!("fixtures/photo.webp");
const PHOTO_AVIF: &[u8] = include_bytes!("fixtures/photo.avif");
const CLIP_MP4: &[u8] = include_bytes!("fixtures/clip.mp4");
const CLIP_MOV: &[u8] = include_bytes!("fixtures/clip.mov");
const CLIP_WEBM: &[u8] = include_bytes!("fixtures/clip.webm");
const CLIP_LIVE_WEBM: &[u8] = include_bytes!("fixtures/clip-live.webm");

/// What the fixtures and builders hide in their metadata.
const SECRETS: [&str; 8] = [
    "secret-camera",
    "secret-gps",
    "secret-xmp",
    "secret-comment",
    "secret-title",
    "secret-trailer",
    "+35.6586",
    "2024:01:02",
];

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn assert_no_secrets(bytes: &[u8]) {
    for secret in SECRETS {
        assert!(!contains(bytes, secret.as_bytes()), "{secret} survived");
    }
}

fn stripped(bytes: &[u8]) -> StrippedMedia {
    match strip_media_metadata(bytes.to_vec()) {
        Ok(MetadataStrip::Stripped(stripped)) => stripped,
        other => panic!("not stripped: {other:?}"),
    }
}

fn malformed_reason(bytes: &[u8]) -> Option<&'static str> {
    strip_media_metadata(bytes.to_vec())
        .err()
        .map(|error| error.reason)
}

/// Stripping its own output changes nothing: the output is a container the
/// walker reads to its end, with nothing left to drop.
fn assert_stable(out: &StrippedMedia) {
    assert_eq!(stripped(&out.bytes), *out);
}

/// The IFD0 tags of a TIFF block.
fn ifd0_tags(tiff: &[u8]) -> Vec<u16> {
    let little = tiff[0] == b'I';
    let u16_at = |i: usize| {
        let pair = [tiff[i], tiff[i + 1]];
        if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    let ifd = if little {
        u32::from_le_bytes(tiff[4..8].try_into().unwrap())
    } else {
        u32::from_be_bytes(tiff[4..8].try_into().unwrap())
    } as usize;
    (0..u16_at(ifd) as usize)
        .map(|i| u16_at(ifd + 2 + i * 12))
        .collect()
}

/// A TIFF block with a camera make, the orientation (if any) and a GPS IFD.
fn exif_with_gps(orientation: Option<u16>, little: bool) -> Vec<u8> {
    let u16b = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let u32b = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let make = b"secret-camera\0";
    let gps_text = b"secret-gps\0";
    let ifd0_entries = 2 + u16::from(orientation.is_some());
    let ifd0_len = 2 + 12 * ifd0_entries as u32 + 4;
    let gps_ifd = 8 + ifd0_len;
    let gps_len = 2 + 12 + 4;
    let make_at = gps_ifd + gps_len;
    let gps_text_at = make_at + make.len() as u32;
    let mut tiff = Vec::new();
    tiff.extend_from_slice(if little { b"II" } else { b"MM" });
    tiff.extend_from_slice(&u16b(42));
    tiff.extend_from_slice(&u32b(8));
    tiff.extend_from_slice(&u16b(ifd0_entries));
    let entry = |tiff: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]| {
        tiff.extend_from_slice(&u16b(tag));
        tiff.extend_from_slice(&u16b(kind));
        tiff.extend_from_slice(&u32b(count));
        tiff.extend_from_slice(&value);
    };
    entry(&mut tiff, 0x010f, 2, make.len() as u32, u32b(make_at));
    if let Some(value) = orientation {
        let short = u16b(value);
        entry(&mut tiff, 0x0112, 3, 1, [short[0], short[1], 0, 0]);
    }
    entry(&mut tiff, 0x8825, 4, 1, u32b(gps_ifd));
    tiff.extend_from_slice(&[0; 4]);
    tiff.extend_from_slice(&u16b(1));
    entry(
        &mut tiff,
        0x0012,
        2,
        gps_text.len() as u32,
        u32b(gps_text_at),
    );
    tiff.extend_from_slice(&[0; 4]);
    tiff.extend_from_slice(make);
    tiff.extend_from_slice(gps_text);
    tiff
}

// --- JPEG ----------------------------------------------------------------------

/// The marker segments before the first scan, as (marker, payload).
fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut segments = Vec::new();
    let mut pos = 2;
    while pos + 4 <= bytes.len() && bytes[pos] == 0xff {
        let marker = bytes[pos + 1];
        let length = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        segments.push((marker, bytes[pos + 4..pos + 2 + length].to_vec()));
        if marker == 0xda {
            break;
        }
        pos += 2 + length;
    }
    segments
}

fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xff, marker];
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The fixture JPEG with every APPn and comment taken out by hand.
fn plain_jpeg() -> Vec<u8> {
    let mut out = PHOTO_JPG[..2].to_vec();
    let mut pos = 2;
    loop {
        let marker = PHOTO_JPG[pos + 1];
        let length = u16::from_be_bytes([PHOTO_JPG[pos + 2], PHOTO_JPG[pos + 3]]) as usize;
        if marker == 0xda {
            out.extend_from_slice(&PHOTO_JPG[pos..]);
            return out;
        }
        if !(0xe0..=0xef).contains(&marker) && marker != 0xfe {
            out.extend_from_slice(&PHOTO_JPG[pos..pos + 2 + length]);
        }
        pos += 2 + length;
    }
}

/// `plain` with metadata segments inserted after SOI and a trailer after EOI.
fn jpeg_with_metadata(exif: &[u8]) -> Vec<u8> {
    let plain = plain_jpeg();
    let mut out = plain[..2].to_vec();
    out.extend(jpeg_segment(0xe1, &[b"Exif\0\0".as_slice(), exif].concat()));
    out.extend(jpeg_segment(
        0xe1,
        b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>secret-xmp</x:xmpmeta>",
    ));
    out.extend(jpeg_segment(0xed, b"Photoshop 3.0\08BIM secret-camera"));
    out.extend(jpeg_segment(0xfe, b"secret-comment"));
    out.extend_from_slice(&plain[2..]);
    out.extend_from_slice(b"secret-trailer");
    out
}

#[test]
fn jpeg_from_an_encoder_keeps_its_icc_profile_scan_and_orientation_only() {
    let out = stripped(PHOTO_JPG);

    assert_eq!(out.container, MediaContainer::Jpeg);
    assert_eq!(out.orientation.map(ExifOrientation::get), Some(6));
    assert_no_secrets(&out.bytes);
    let segments = jpeg_segments(&out.bytes);
    let app1: Vec<_> = segments.iter().filter(|(m, _)| *m == 0xe1).collect();
    assert_eq!(app1.len(), 1);
    assert_eq!(ifd0_tags(&app1[0].1[6..]), [0x0112]);
    let icc = |bytes: &[u8]| {
        jpeg_segments(bytes)
            .into_iter()
            .filter(|(m, data)| *m == 0xe2 && data.starts_with(b"ICC_PROFILE\0"))
            .collect::<Vec<_>>()
    };
    assert!(!icc(PHOTO_JPG).is_empty());
    assert_eq!(icc(&out.bytes), icc(PHOTO_JPG));
    let scan = |bytes: &[u8]| {
        let sos = bytes.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
        bytes[sos..].to_vec()
    };
    assert_eq!(scan(&out.bytes), scan(PHOTO_JPG));
    assert_stable(&out);
}

#[test]
fn jpeg_drops_exif_xmp_iptc_comments_and_trailer() {
    let out = stripped(&jpeg_with_metadata(&exif_with_gps(None, false)));

    assert_eq!(out.orientation, None);
    assert_eq!(out.bytes, plain_jpeg());
}

#[test]
fn jpeg_keeps_orientation_in_either_byte_order() {
    for little in [false, true] {
        let out = stripped(&jpeg_with_metadata(&exif_with_gps(Some(8), little)));
        assert_no_secrets(&out.bytes);
        assert_eq!(out.orientation.map(ExifOrientation::get), Some(8));
        let app1 = jpeg_segments(&out.bytes)
            .into_iter()
            .find(|(m, _)| *m == 0xe1)
            .unwrap();
        assert_eq!(exif_orientation(&app1.1), out.orientation);
        assert_eq!(app1.1.len(), 6 + 26);
    }
}

#[test]
fn jpeg_drops_an_orientation_of_one_and_leaves_a_clean_file_byte_for_byte() {
    let plain = plain_jpeg();
    assert_eq!(stripped(&plain).bytes, plain);
    let out = stripped(&jpeg_with_metadata(&exif_with_gps(Some(1), true)));
    assert_eq!(out.bytes, plain);
}

#[test]
fn jpeg_truncated_in_the_scan_is_kept_and_before_it_refused() {
    let input = jpeg_with_metadata(&exif_with_gps(Some(3), false));
    let cut = &input[..input.len() - "secret-trailer".len() - 40];
    let out = stripped(cut);
    assert_no_secrets(&out.bytes);
    assert_eq!(out.orientation.map(ExifOrientation::get), Some(3));

    assert_eq!(
        malformed_reason(&input[..60]),
        Some("a segment runs past the end")
    );
    assert_eq!(malformed_reason(&[0xff, 0xd8, 0xff, 0xd9]), None);
    assert_eq!(
        malformed_reason(&[0xff, 0xd8, 0xff]),
        Some("ends inside a marker")
    );
    assert_eq!(
        malformed_reason(&[0xff, 0xd8, 0xff, 0xe1, 0x00, 0x01]),
        Some("a segment runs past the end")
    );
    assert_eq!(
        malformed_reason(&[0xff, 0xd8, 0xff, 0xdb, 0x00, 0x02, 0x12]),
        Some("expected a marker")
    );
    let no_scan = jpeg_segment(0xdb, &[0; 4]);
    assert_eq!(
        malformed_reason(&[&[0xff, 0xd8][..], &no_scan].concat()),
        Some("ends before the image data")
    );
}

// --- PNG -----------------------------------------------------------------------

fn png_chunks(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut chunks = Vec::new();
    let mut pos = 8;
    while pos + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = String::from_utf8_lossy(&bytes[pos + 4..pos + 8]).into_owned();
        let crc = u32::from_be_bytes(
            bytes[pos + 8 + length..pos + 12 + length]
                .try_into()
                .unwrap(),
        );
        assert_eq!(crc32(&bytes[pos + 4..pos + 8 + length]), crc, "{kind} CRC");
        chunks.push((kind, bytes[pos + 8..pos + 8 + length].to_vec()));
        pos += 12 + length;
    }
    chunks
}

fn png_of(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut out = png::SIGNATURE.to_vec();
    for (kind, data) in chunks {
        push_chunk(&mut out, kind, data);
    }
    out
}

fn chunk_types(chunks: &[(String, Vec<u8>)]) -> Vec<&str> {
    chunks.iter().map(|(kind, _)| kind.as_str()).collect()
}

#[test]
fn png_from_an_encoder_keeps_its_colour_chunks_and_orientation_only() {
    let input = png_chunks(PHOTO_PNG);
    assert!(chunk_types(&input).contains(&"tEXt"));
    assert!(chunk_types(&input).contains(&"iTXt"));

    let out = stripped(PHOTO_PNG);

    assert_eq!(out.container, MediaContainer::Png);
    assert_eq!(out.orientation.map(ExifOrientation::get), Some(8));
    assert_no_secrets(&out.bytes);
    let chunks = png_chunks(&out.bytes);
    let expected: Vec<&str> = chunk_types(&input)
        .into_iter()
        .filter(|kind| !matches!(*kind, "tEXt" | "iTXt" | "zTXt" | "tIME"))
        .collect();
    assert_eq!(chunk_types(&chunks), expected);
    let exif = chunks.iter().find(|(kind, _)| kind == "eXIf").unwrap();
    assert_eq!(ifd0_tags(&exif.1), [0x0112]);
    for kept in ["iCCP", "IDAT"] {
        assert_eq!(
            chunks.iter().find(|(kind, _)| kind == kept),
            input.iter().find(|(kind, _)| kind == kept),
        );
    }
    assert_stable(&out);
}

#[test]
fn png_drops_text_time_exif_and_private_chunks() {
    let ihdr = &png_chunks(PHOTO_PNG)[0].1;
    let idat = png_chunks(PHOTO_PNG)
        .into_iter()
        .find(|(kind, _)| kind == "IDAT")
        .unwrap()
        .1;
    let exif = exif_with_gps(Some(1), false);
    let input = png_of(&[
        (b"IHDR", ihdr),
        (b"sRGB", &[0]),
        (b"tEXt", b"Author\0secret-camera"),
        (b"zTXt", b"Comment\0\0secret-comment"),
        (b"tIME", &[7, 232, 1, 2, 3, 4, 5]),
        (b"eXIf", &exif),
        (b"iDOT", b"secret-xmp"),
        (b"IDAT", &idat),
        (b"IEND", b""),
    ]);
    let out = stripped(&input);
    assert_eq!(out.orientation, None);
    assert_eq!(
        out.bytes,
        png_of(&[
            (b"IHDR", ihdr),
            (b"sRGB", &[0]),
            (b"IDAT", &idat),
            (b"IEND", b"")
        ])
    );
    assert_stable(&out);
}

#[test]
fn png_refuses_a_file_it_cannot_walk() {
    let input = PHOTO_PNG;
    assert_eq!(
        malformed_reason(&input[..input.len() - 12]),
        Some("ends before IEND")
    );
    assert_eq!(
        malformed_reason(&input[..50]),
        Some("a chunk runs past the end")
    );
    let no_header = [&input[..8], &input[33..]].concat();
    assert_eq!(
        malformed_reason(&no_header),
        Some("does not start with IHDR")
    );
    let mut bad_type = input.to_vec();
    bad_type[12] = b'1';
    assert_eq!(malformed_reason(&bad_type), Some("invalid chunk type"));
}

// --- WebP ----------------------------------------------------------------------

fn webp_chunks(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
        bytes.len() - 8,
        "RIFF size"
    );
    let mut chunks = Vec::new();
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let kind = String::from_utf8_lossy(&bytes[pos..pos + 4]).into_owned();
        chunks.push((kind, bytes[pos + 8..pos + 8 + size].to_vec()));
        pos += 8 + size + size % 2;
    }
    assert_eq!(pos, bytes.len());
    chunks
}

fn webp_of(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    for (kind, data) in chunks {
        body.extend_from_slice(*kind);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    [
        b"RIFF".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &body,
    ]
    .concat()
}

#[test]
fn webp_from_an_encoder_keeps_its_icc_profile_and_orientation_only() {
    let input = webp_chunks(PHOTO_WEBP);
    assert!(chunk_types(&input).contains(&"XMP "));

    let out = stripped(PHOTO_WEBP);

    assert_eq!(out.container, MediaContainer::Webp);
    assert_eq!(out.orientation.map(ExifOrientation::get), Some(3));
    assert_no_secrets(&out.bytes);
    let chunks = webp_chunks(&out.bytes);
    assert_eq!(chunk_types(&chunks), ["VP8X", "ICCP", "VP8 ", "EXIF"]);
    // ICC (0x20) and EXIF (0x08) remain; XMP (0x04) is gone.
    assert_eq!(chunks[0].1[0] & 0x2c, 0x28);
    assert_eq!(chunks[1], input[1]);
    assert_eq!(ifd0_tags(&chunks[3].1), [0x0112]);
    assert_stable(&out);
}

#[test]
fn webp_drops_exif_xmp_and_unknown_chunks_and_fixes_flags() {
    let input = webp_chunks(PHOTO_WEBP);
    let vp8x = &input[0].1;
    let image = &input.iter().find(|(kind, _)| kind == "VP8 ").unwrap().1;
    let exif = exif_with_gps(None, true);
    let built = webp_of(&[
        (b"VP8X", vp8x),
        (b"VP8 ", image),
        (b"EXIF", &exif),
        (b"XMP ", b"secret-xmp"),
        (b"ABCD", b"secret-trailer"),
    ]);
    let out = stripped(&built);
    assert_eq!(out.orientation, None);
    assert_no_secrets(&out.bytes);
    let chunks = webp_chunks(&out.bytes);
    assert_eq!(chunk_types(&chunks), ["VP8X", "VP8 "]);
    assert_eq!(chunks[0].1[0] & 0x0c, 0);

    let simple = webp_of(&[(b"VP8 ", image)]);
    assert_eq!(stripped(&simple).bytes, simple);
}

#[test]
fn webp_animation_frames_keep_only_their_picture_chunks() {
    let image = &webp_chunks(PHOTO_WEBP)
        .into_iter()
        .find(|(kind, _)| kind == "VP8 ")
        .unwrap()
        .1;
    let frame_header = [7u8; 16];
    let frame = |chunks: &[(&[u8; 4], &[u8])]| webp_of(chunks)[12..].to_vec();
    let anmf = [
        &frame_header[..],
        &frame(&[(b"ALPH", b"a"), (b"VP8 ", image), (b"XYZW", b"secret-gps")]),
    ]
    .concat();
    let vp8x = [&[0x12u8][..], &[0; 9]].concat();
    let input = webp_of(&[(b"VP8X", &vp8x), (b"ANIM", &[0; 6]), (b"ANMF", &anmf)]);

    let out = stripped(&input);
    assert_no_secrets(&out.bytes);
    let chunks = webp_chunks(&out.bytes);
    assert_eq!(chunk_types(&chunks), ["VP8X", "ANIM", "ANMF"]);
    let kept = [
        &frame_header[..],
        &frame(&[(b"ALPH", b"a"), (b"VP8 ", image)]),
    ]
    .concat();
    assert_eq!(chunks[2].1, kept);
    assert_stable(&out);
}

#[test]
fn webp_refuses_a_riff_size_past_the_end_and_a_file_without_image_data() {
    assert_eq!(
        malformed_reason(&PHOTO_WEBP[..PHOTO_WEBP.len() - 1]),
        Some("the RIFF size runs past the end")
    );
    assert_eq!(
        malformed_reason(&webp_of(&[(b"XMP ", b"x")])),
        Some("has no image data")
    );
}

// --- HEIF ----------------------------------------------------------------------

#[test]
fn avif_from_an_encoder_has_its_exif_and_xmp_items_blanked_in_place() {
    let out = stripped(PHOTO_AVIF);

    assert_eq!(out.container, MediaContainer::Heif);
    assert_no_secrets(&out.bytes);
    assert_eq!(out.bytes.len(), PHOTO_AVIF.len());
    let changed = PHOTO_AVIF
        .iter()
        .zip(&out.bytes)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0);
    assert_stable(&out);
}

#[test]
fn heic_is_stripped_like_avif() {
    // Same item layout under a HEIC major brand.
    let mut heic = PHOTO_AVIF.to_vec();
    heic[8..12].copy_from_slice(b"heic");
    let out = stripped(&heic);
    assert_eq!(out.container, MediaContainer::Heif);
    assert_no_secrets(&out.bytes);
}

#[test]
fn heif_refuses_boxes_that_do_not_fit() {
    assert_eq!(
        malformed_reason(&PHOTO_AVIF[..300]),
        Some("the boxes do not fit the file")
    );
}

/// A HEIF image whose EXIF item 1 has the `iloc` entries `entries`, each
/// made of the item id, construction method, data reference, base offset
/// count and extents as written, with field sizes `sizes`, ahead of `mdat`.
fn heif_with_iloc(sizes: u16, entries: &[Vec<u8>], mdat: &[u8]) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"avif\0\0\0\0avifmif1");
    let hdlr = full_box(b"hdlr", &[&[0; 4][..], b"pict", &[0; 13]].concat());
    let infe = mp4_box(b"infe", b"\x02\0\0\0\0\x01\0\0Exif\0");
    let iinf = full_box(b"iinf", &[&1u16.to_be_bytes()[..], &infe].concat());
    let mut iloc = vec![1, 0, 0, 0];
    iloc.extend(sizes.to_be_bytes());
    iloc.extend((entries.len() as u16).to_be_bytes());
    iloc.extend(entries.concat());
    let iloc = mp4_box(b"iloc", &iloc);
    let meta = full_box(b"meta", &[hdlr, iinf, iloc].concat());
    [ftyp, meta, mp4_box(b"mdat", mdat)].concat()
}

#[test]
fn heif_refuses_an_item_located_twice_or_zero_width_extents_repeated() {
    // Version 1 entries with no field wider than zero: every extent reads no
    // bytes and means the whole file, so 65535 of them would refill it
    // 65535 times.
    let repeated = heif_with_iloc(0, &[vec![0, 1, 0, 0, 0, 0, 0xff, 0xff]], b"secret-gps");
    assert_eq!(
        malformed_reason(&repeated),
        Some("unreadable item locations")
    );
    let one = vec![0, 1, 0, 0, 0, 0, 0, 1];
    assert_eq!(
        malformed_reason(&heif_with_iloc(
            0,
            &[one.clone(), one.clone()],
            b"secret-gps"
        )),
        Some("unreadable item locations")
    );
    // Listed once, the same extent is a whole-file item, blanked.
    let out = stripped(&heif_with_iloc(0, &[one], b"secret-gps"));
    assert_no_secrets(&out.bytes);
}

#[test]
fn heif_extents_over_the_same_bytes_are_filled_once() {
    // One item of 4000 four-byte extents, each from the start of `mdat` to
    // the end of the file: 4000 whole-file fills before they were merged.
    let mdat = vec![b'x'; 4096];
    let prefix = heif_with_iloc(0x4400, &[], &[]).len();
    let mut entry = vec![0, 1, 0, 0, 0, 0];
    entry.extend(4000u16.to_be_bytes());
    let entry_len = entry.len() + 4000 * 8;
    let mdat_body = (prefix + entry_len) as u32;
    for _ in 0..4000 {
        entry.extend(mdat_body.to_be_bytes());
        entry.extend(0u32.to_be_bytes());
    }
    let input = heif_with_iloc(0x4400, &[entry], &mdat);
    assert_eq!(&input[mdat_body as usize..][..4], b"xxxx");

    let edits = heif::strip(&input).expect("a readable HEIF");
    assert!(edits.written_bytes() <= input.len());
    let out = stripped(&input);
    assert!(!contains(&out.bytes, b"xxxx"));
}

// --- MP4 / QuickTime ---------------------------------------------------------------

fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat()
}

fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    mp4_box(kind, &[&[0, 0, 0, 0][..], body].concat())
}

/// A track whose one chunk of one sample sits at `offset`.
fn track(id: u32, handler: &[u8; 4], offset: u32, sample_size: u32) -> Vec<u8> {
    let stamp = 0x5ec2_e700u32.to_be_bytes();
    let tkhd = full_box(
        b"tkhd",
        &[&stamp[..], &stamp, &id.to_be_bytes(), &[0; 8]].concat(),
    );
    let mdhd = full_box(b"mdhd", &[&stamp[..], &stamp, &[0; 8]].concat());
    let hdlr = full_box(b"hdlr", &[&[0; 4][..], handler, &[0; 13]].concat());
    let stsc = full_box(b"stsc", &[1u32, 1, 1, 1].map(u32::to_be_bytes).concat());
    let stsz = full_box(
        b"stsz",
        &[0u32, 1, sample_size].map(u32::to_be_bytes).concat(),
    );
    let stco = full_box(b"stco", &[1u32, offset].map(u32::to_be_bytes).concat());
    let stbl = mp4_box(b"stbl", &[stsc, stsz, stco].concat());
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
    mp4_box(b"trak", &[tkhd, mdia].concat())
}

/// A movie with a video track, a GoPro-style GPS track, user data and an
/// XMP `uuid` box; its samples follow in `mdat`.
fn mp4_with_gps_track() -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\x02\0isomavc1");
    let video = b"VIDEOSAMPLE";
    let gps = b"secret-gps-fix";
    let build = |mdat_body: u32| {
        let mvhd = full_box(
            b"mvhd",
            &[0x5ec2_e700u32, 0x5ec2_e700, 1000, 0]
                .map(u32::to_be_bytes)
                .concat(),
        );
        let xyz = mp4_box(b"\xa9xyz", b"\0\x12\0\0+35.6586+139.7454/");
        let udta = mp4_box(b"udta", &xyz);
        let xmp = mp4_box(
            b"uuid",
            &[
                &b"\xbe\x7a\xcf\xcb\x97\xa9\x42\xe8\x9c\x71\x99\x94\x91\xe3\xaf\xac"[..],
                b"secret-xmp",
            ]
            .concat(),
        );
        mp4_box(
            b"moov",
            &[
                mvhd,
                track(1, b"vide", mdat_body, video.len() as u32),
                track(2, b"meta", mdat_body + video.len() as u32, gps.len() as u32),
                udta,
                xmp,
            ]
            .concat(),
        )
    };
    let moov_len = build(0).len();
    let mdat_body = (ftyp.len() + moov_len + 8) as u32;
    [
        ftyp,
        build(mdat_body),
        mp4_box(b"mdat", &[&video[..], gps].concat()),
    ]
    .concat()
}

#[test]
fn mp4_frees_user_data_xmp_and_the_gps_track_in_place() {
    let input = mp4_with_gps_track();
    let out = stripped(&input);

    assert_eq!(out.container, MediaContainer::Mp4);
    assert_eq!(out.bytes.len(), input.len());
    assert_no_secrets(&out.bytes);
    assert!(contains(&out.bytes, b"VIDEOSAMPLE"));
    assert!(!contains(&out.bytes, &0x5ec2_e700u32.to_be_bytes()));
    // The video track is still a track; the GPS track is filler.
    assert_eq!(out.bytes.windows(4).filter(|w| *w == b"trak").count(), 1);
    assert_stable(&out);
}

#[test]
fn mp4_and_mov_from_an_encoder_lose_their_tags_and_keep_their_length() {
    for input in [CLIP_MP4, CLIP_MOV] {
        assert!(contains(input, b"secret-title"));
        let out = stripped(input);
        assert_eq!(out.container, MediaContainer::Mp4);
        assert_eq!(out.bytes.len(), input.len());
        assert_no_secrets(&out.bytes);
        assert_stable(&out);
    }
}

/// A timed metadata track of `sample_count` samples of `sample_size` bytes,
/// `per_chunk` to a chunk, in chunks at `chunk_offsets`.
fn metadata_track(
    per_chunk: u32,
    sample_size: u32,
    sample_count: u32,
    chunk_offsets: &[u32],
) -> Vec<u8> {
    let tkhd = full_box(
        b"tkhd",
        &[&[0; 8][..], &2u32.to_be_bytes(), &[0; 8]].concat(),
    );
    let hdlr = full_box(b"hdlr", &[&[0; 4][..], b"meta", &[0; 13]].concat());
    let stsc = full_box(
        b"stsc",
        &[1u32, 1, per_chunk, 1].map(u32::to_be_bytes).concat(),
    );
    let stsz = full_box(
        b"stsz",
        &[sample_size, sample_count].map(u32::to_be_bytes).concat(),
    );
    let mut stco = (chunk_offsets.len() as u32).to_be_bytes().to_vec();
    for offset in chunk_offsets {
        stco.extend(offset.to_be_bytes());
    }
    let stbl = mp4_box(b"stbl", &[stsc, stsz, full_box(b"stco", &stco)].concat());
    let mdia = mp4_box(b"mdia", &[hdlr, mp4_box(b"minf", &stbl)].concat());
    mp4_box(b"trak", &[tkhd, mdia].concat())
}

/// `ftyp`, then `mdat` holding `samples`, then a `moov` with `track`, whose
/// sample offsets count from where `mdat_body` says the samples are.
fn mp4_with_track(samples: &[u8], track: impl Fn(u32) -> Vec<u8>) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\0\0isom");
    let mdat_body = (ftyp.len() + 8) as u32;
    let mvhd = full_box(b"mvhd", &[0; 96]);
    let moov = mp4_box(b"moov", &[mvhd, track(mdat_body)].concat());
    [ftyp, mp4_box(b"mdat", samples), moov].concat()
}

#[test]
fn mp4_refuses_a_metadata_track_with_more_samples_than_mdat() {
    // 20000 chunks of 100 bytes, all at the same 100 bytes.
    let overlapping = mp4_with_track(&[b'x'; 100], |at| {
        metadata_track(1, 100, 20_000, &vec![at; 20_000])
    });
    assert_eq!(
        malformed_reason(&overlapping),
        Some("a metadata track has more samples than mdat holds")
    );
    // One chunk of four billion one-byte samples, summed one by one before.
    let endless = mp4_with_track(&[b'x'; 64], |at| {
        metadata_track(u32::MAX, 1, u32::MAX, &[at])
    });
    assert_eq!(
        malformed_reason(&endless),
        Some("a metadata track has more samples than mdat holds")
    );
    // Chunks that share bytes but stay within mdat are zeroed.
    let shared = mp4_with_track(b"secret-gps-fix", |at| {
        metadata_track(1, 4, 3, &[at, at + 2, at + 4])
    });
    assert!(!contains(&stripped(&shared).bytes, b"secret-gps"));
}

#[test]
fn mp4_fragments_over_the_same_samples_are_filled_once() {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\0\0isom");
    let mvhd = full_box(b"mvhd", &[0; 96]);
    let samples = vec![b'x'; 4096];
    let moov_len = mp4_box(
        b"moov",
        &[mvhd.clone(), metadata_track(1, 1, 1, &[0])].concat(),
    )
    .len();
    let mdat_body = (ftyp.len() + moov_len + 8) as u32;
    let moov = mp4_box(
        b"moov",
        &[mvhd, metadata_track(1, 1, 1, &[mdat_body])].concat(),
    );
    // 2000 fragments of the metadata track, each one sample over all of mdat.
    let tfhd = full_box(
        b"tfhd",
        &[&2u32.to_be_bytes()[..], &u64::from(mdat_body).to_be_bytes()].concat(),
    );
    let mut tfhd = tfhd;
    tfhd[11] = 0x01; // base data offset
    let mut trun = full_box(b"trun", &[1u32, 0, 4096].map(u32::to_be_bytes).concat());
    trun[10..12].copy_from_slice(&0x0201u16.to_be_bytes()); // data offset, sample sizes
    let moof = mp4_box(b"moof", &mp4_box(b"traf", &[tfhd, trun].concat()));
    let input = [ftyp, moov, mp4_box(b"mdat", &samples), moof.repeat(2000)].concat();

    let edits = mp4::strip(&input).expect("a readable fragmented MP4");
    assert!(edits.written_bytes() <= input.len());
    assert!(!contains(&stripped(&input).bytes, b"xxxx"));
}

#[test]
fn mp4_drops_timed_text_and_timecode_tracks() {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\0\0isom");
    let text = b"secret-gps 35.6586 139.7454";
    let timecode = b"\x00\x11\x22\x33";
    let build = |mdat_body: u32| {
        mp4_box(
            b"moov",
            &[
                full_box(b"mvhd", &[0; 96]),
                track(1, b"sbtl", mdat_body, text.len() as u32),
                track(2, b"tmcd", mdat_body + text.len() as u32, 4),
            ]
            .concat(),
        )
    };
    let mdat_body = (ftyp.len() + build(0).len() + 8) as u32;
    let input = [
        ftyp,
        build(mdat_body),
        mp4_box(b"mdat", &[&text[..], timecode].concat()),
    ]
    .concat();
    let out = stripped(&input);
    assert_no_secrets(&out.bytes);
    assert!(!contains(&out.bytes, timecode));
    assert!(!contains(&out.bytes, b"trak"));
}

#[test]
fn an_image_sequence_loses_its_movie_metadata_too() {
    // An animated AVIF: a still image's `meta` and a movie's `moov`.
    let ftyp = mp4_box(b"ftyp", b"avis\0\0\0\0avismif1");
    let meta = full_box(
        b"meta",
        &full_box(b"hdlr", &[&[0; 4][..], b"pict", &[0; 13]].concat()),
    );
    let mvhd = full_box(
        b"mvhd",
        &[&0x5ec2_e700u32.to_be_bytes()[..], &[0; 92]].concat(),
    );
    let udta = mp4_box(
        b"udta",
        &mp4_box(b"\xa9xyz", b"\0\x12\0\0+35.6586+139.7454/"),
    );
    let input = [
        ftyp,
        meta,
        mp4_box(b"moov", &[mvhd, udta].concat()),
        mp4_box(b"mdat", b"frames"),
    ]
    .concat();
    let out = stripped(&input);
    assert_eq!(out.container, MediaContainer::Heif);
    assert_no_secrets(&out.bytes);
    assert!(!contains(&out.bytes, &0x5ec2_e700u32.to_be_bytes()));
}

#[test]
fn mp4_refuses_a_file_it_cannot_walk() {
    assert_eq!(
        malformed_reason(&mp4_box(b"ftyp", b"isom\0\0\0\0")),
        Some("has no movie box")
    );
    let mut too_big = CLIP_MP4.to_vec();
    let moov = too_big.windows(4).position(|w| w == b"moov").unwrap() - 4;
    too_big[moov..moov + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(malformed_reason(&too_big), Some("a box runs past the end"));
}

// --- WebM ----------------------------------------------------------------------------

#[test]
fn webm_from_an_encoder_voids_tags_and_title_in_place() {
    for input in [CLIP_WEBM, CLIP_LIVE_WEBM] {
        assert!(contains(input, b"secret-title"));
        let out = stripped(input);
        assert_eq!(out.container, MediaContainer::Webm);
        assert_eq!(out.bytes.len(), input.len());
        assert_no_secrets(&out.bytes);
        assert_stable(&out);
    }
}

#[test]
fn webm_refuses_other_ebml_documents_and_broken_files() {
    let mut other = CLIP_WEBM.to_vec();
    let doc_type = other.windows(4).position(|w| w == b"webm").unwrap();
    other[doc_type..doc_type + 4].copy_from_slice(b"abcd");
    assert_eq!(
        malformed_reason(&other),
        Some("not a Matroska or WebM document")
    );
    assert!(malformed_reason(&CLIP_WEBM[..30]).is_some());
}

// --- Damaged input and dispatch ------------------------------------------------------

const FIXTURES: [&[u8]; 8] = [
    PHOTO_JPG,
    PHOTO_PNG,
    PHOTO_WEBP,
    PHOTO_AVIF,
    CLIP_MP4,
    CLIP_MOV,
    CLIP_WEBM,
    CLIP_LIVE_WEBM,
];

#[test]
fn no_truncation_panics_or_passes_on_metadata() {
    for input in FIXTURES {
        for length in (0..=input.len()).step_by(3) {
            if let Ok(MetadataStrip::Stripped(out)) = strip_media_metadata(input[..length].to_vec())
            {
                assert_no_secrets(&out.bytes);
            }
        }
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    // A fixed linear congruential sequence, so a failure reproduces.
    let mut seed = 12345u64;
    let mut next = move || {
        seed = (seed * 1_103_515_245 + 12345) % (1 << 31);
        seed as usize
    };
    for input in FIXTURES {
        for _ in 0..300 {
            let mut corrupted = input.to_vec();
            for _ in 0..4 {
                let at = next() % corrupted.len();
                corrupted[at] = next() as u8;
            }
            let _ = strip_media_metadata(corrupted);
        }
    }
}

#[test]
fn other_bytes_are_an_unknown_format() {
    for bytes in [&b""[..], b"\x01\x02\x03", b"GIF89a\x01\0\x01\0", b"ID3\x04"] {
        assert_eq!(
            strip_media_metadata(bytes.to_vec()),
            Ok(MetadataStrip::UnknownFormat(bytes.to_vec()))
        );
    }
}

#[test]
fn exif_orientation_reads_ifd0_in_either_byte_order() {
    assert_eq!(
        exif_orientation(&exif_with_gps(Some(6), false)).map(ExifOrientation::get),
        Some(6)
    );
    assert_eq!(
        exif_orientation(&exif_with_gps(Some(7), true)).map(ExifOrientation::get),
        Some(7)
    );
    assert_eq!(exif_orientation(&exif_with_gps(None, true)), None);
    assert_eq!(exif_orientation(&exif_with_gps(Some(1), false)), None);
    assert_eq!(exif_orientation(&exif_with_gps(Some(9), false)), None);
    assert_eq!(exif_orientation(b"MM\0\x2a\xff\xff\xff\xff"), None);
}

// --- Uploads -------------------------------------------------------------------------

#[test]
fn uploads_are_stripped_by_their_bytes_whatever_the_declared_type() {
    for (content_type, input) in [
        ("image/jpeg", PHOTO_JPG),
        ("IMAGE/JPEG; charset=binary", PHOTO_JPG),
        ("image/gif", PHOTO_JPG),
        ("application/octet-stream", PHOTO_PNG),
        ("image/heic", PHOTO_AVIF),
        ("video/mp4", CLIP_MP4),
        ("video/quicktime", CLIP_MOV),
        ("audio/mp4", CLIP_MP4),
        ("video/webm", CLIP_LIVE_WEBM),
    ] {
        let stored = strip_upload_metadata(content_type, input.to_vec()).unwrap();
        assert_no_secrets(&stored);
    }
}

#[test]
fn uploads_pass_through_formats_without_a_stripper() {
    let gif = b"GIF89a\x01\0\x01\0".to_vec();
    assert_eq!(strip_upload_metadata("image/gif", gif.clone()), Ok(gif));
    let mp3 = b"ID3\x04\0\0\0\0\0\0".to_vec();
    assert_eq!(strip_upload_metadata("audio/mpeg", mp3.clone()), Ok(mp3));
}

#[test]
fn uploads_are_refused_when_damaged_mislabelled_or_tiff() {
    assert_eq!(
        strip_upload_metadata("image/png", b"hello".to_vec()),
        Err(UnstrippableUpload::NotTheDeclaredType {
            content_type: "image/png".to_owned()
        })
    );
    assert_eq!(
        strip_upload_metadata("Image/PNG; x=y", b"hello".to_vec()),
        Err(UnstrippableUpload::NotTheDeclaredType {
            content_type: "image/png".to_owned()
        })
    );
    assert_eq!(
        strip_upload_metadata("video/mp4", PHOTO_JPG.to_vec()),
        Err(UnstrippableUpload::NotTheDeclaredType {
            content_type: "video/mp4".to_owned()
        })
    );
    assert_eq!(
        strip_upload_metadata("image/webp", CLIP_WEBM.to_vec()),
        Err(UnstrippableUpload::NotTheDeclaredType {
            content_type: "image/webp".to_owned()
        })
    );
    assert!(matches!(
        strip_upload_metadata("image/jpeg", PHOTO_JPG[..100].to_vec()),
        Err(UnstrippableUpload::Malformed(_))
    ));
    assert_eq!(
        strip_upload_metadata("image/tiff", b"II*\0\x08\0\0\0".to_vec()),
        Err(UnstrippableUpload::Tiff)
    );
}

#[test]
fn stored_media_is_rewritten_only_when_it_carries_metadata() {
    let rewrite = restripped_stored_media(PHOTO_JPG)
        .unwrap()
        .expect("carries EXIF");
    assert_no_secrets(&rewrite);
    assert_eq!(restripped_stored_media(&rewrite), Ok(None));
    assert_eq!(restripped_stored_media(b"GIF89a\x01\0\x01\0"), Ok(None));
    assert!(restripped_stored_media(&PHOTO_PNG[..50]).is_err());
}
