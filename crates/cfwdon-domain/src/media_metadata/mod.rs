//! Removing the metadata a camera, phone or editor writes into a file (EXIF
//! with its GPS position, XMP, IPTC, comments, device and timestamps) before
//! it is published, as Mastodon's libvips and ffmpeg re-encodes do. A Worker
//! has neither, so the container is rewritten losslessly instead: images
//! drop metadata segments and chunks (HEIF blanks its metadata items in
//! place), videos overwrite metadata boxes and elements with filler of the
//! same length. No pixel or sample data is decoded or changed, and the walk
//! is linear in the file size.
//!
//! What stays is what changes how the file looks or plays: ICC profiles and
//! colour chunks, JFIF and Adobe markers, and the EXIF orientation, which
//! goes back in as a new EXIF block holding only that tag so a rotated phone
//! photo still displays upright.

mod bytes;
mod edits;
mod exif;
mod heif;
mod jpeg;
mod mp4;
mod png;
mod webm;
mod webp;

#[cfg(test)]
mod tests;

pub use exif::{ExifOrientation, exif_orientation};

/// The containers the stripper rewrites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaContainer {
    Jpeg,
    Png,
    Webp,
    /// HEIF still images: AVIF and HEIC.
    Heif,
    /// ISO BMFF video and audio: MP4, QuickTime MOV, M4V, M4A, 3GP.
    Mp4,
    /// EBML: WebM and Matroska.
    Webm,
}

impl MediaContainer {
    pub const fn is_image(self) -> bool {
        matches!(self, Self::Jpeg | Self::Png | Self::Webp | Self::Heif)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Webp => "WebP",
            Self::Heif => "HEIF",
            Self::Mp4 => "MP4",
            Self::Webm => "WebM",
        }
    }

    /// The container `bytes` starts like, from its magic numbers.
    fn of(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Self::Jpeg)
        } else if bytes.starts_with(png::SIGNATURE) {
            Some(Self::Png)
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            Some(Self::Webp)
        } else if heif::is_heif_image(bytes) {
            Some(Self::Heif)
        } else if webm::starts_like_webm(bytes) {
            Some(Self::Webm)
        } else if mp4::starts_like_mp4(bytes) {
            Some(Self::Mp4)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrippedMedia {
    pub container: MediaContainer,
    pub bytes: Vec<u8>,
    /// The orientation the stripped image keeps, if any.
    pub orientation: Option<ExifOrientation>,
}

/// What stripping found: a rewritten file, or bytes of no container it
/// knows, handed back as they came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataStrip {
    Stripped(StrippedMedia),
    UnknownFormat(Vec<u8>),
}

/// A file that starts like a known container but cannot be walked to its end.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("malformed {}: {reason}", container.name())]
pub struct MalformedMedia {
    pub container: MediaContainer,
    pub reason: &'static str,
}

/// How a container loses its metadata: a copy without it, or overwrites
/// in the buffer the file arrived in.
enum Rewrite {
    Copy(Vec<u8>, Option<ExifOrientation>),
    /// Same-length rewrites: a video is too large to copy within a Worker's
    /// memory.
    InPlace(edits::Edits),
}

fn rewrite(container: MediaContainer, bytes: &[u8]) -> Result<Rewrite, MalformedMedia> {
    let copy = |(out, orientation)| Rewrite::Copy(out, orientation);
    match container {
        MediaContainer::Jpeg => jpeg::strip(bytes).map(copy),
        MediaContainer::Png => png::strip(bytes).map(copy),
        MediaContainer::Webp => webp::strip(bytes).map(copy),
        MediaContainer::Heif => heif::strip(bytes).map(Rewrite::InPlace),
        MediaContainer::Mp4 => mp4::strip(bytes).map(Rewrite::InPlace),
        MediaContainer::Webm => webm::strip(bytes).map(Rewrite::InPlace),
    }
    .map_err(|reason| MalformedMedia { container, reason })
}

/// Rewrite a JPEG, PNG, WebP, HEIF, MP4 or WebM file without its metadata,
/// judged by the bytes, not a declared type. `UnknownFormat` for anything
/// else (GIF, MP3, ...); `MalformedMedia` when the container cannot be
/// walked, so that nothing is passed on unstripped.
pub fn strip_media_metadata(mut bytes: Vec<u8>) -> Result<MetadataStrip, MalformedMedia> {
    let Some(container) = MediaContainer::of(&bytes) else {
        return Ok(MetadataStrip::UnknownFormat(bytes));
    };
    let (bytes, orientation) = match rewrite(container, &bytes)? {
        Rewrite::Copy(out, orientation) => (out, orientation),
        Rewrite::InPlace(edits) => {
            edits.apply(&mut bytes);
            (bytes, None)
        }
    };
    Ok(MetadataStrip::Stripped(StrippedMedia {
        container,
        bytes,
        orientation,
    }))
}

/// What to do with an already stored object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoredMediaRestrip {
    /// Write these bytes in its place.
    Rewrite(Vec<u8>),
    /// Nothing to strip: already clean, or a format without metadata to
    /// strip, such as GIF.
    Unchanged,
    /// A TIFF, which keeps its EXIF: no lossless rewrite drops it.
    Tiff,
}

/// The stored object `bytes` without its metadata, judged by the bytes as on
/// upload. Unlike an upload, the declared type is not checked: the object is
/// already public, and only its metadata goes. A video is rewritten in the
/// buffer it came in, and nothing is copied when nothing changes.
pub fn restripped_stored_media(mut bytes: Vec<u8>) -> Result<StoredMediaRestrip, MalformedMedia> {
    let Some(container) = MediaContainer::of(&bytes) else {
        return Ok(if is_tiff(&bytes) {
            StoredMediaRestrip::Tiff
        } else {
            StoredMediaRestrip::Unchanged
        });
    };
    Ok(match rewrite(container, &bytes)? {
        Rewrite::Copy(out, _) if out != bytes => StoredMediaRestrip::Rewrite(out),
        Rewrite::InPlace(edits) if edits.change(&bytes) => {
            edits.apply(&mut bytes);
            StoredMediaRestrip::Rewrite(bytes)
        }
        Rewrite::Copy(..) | Rewrite::InPlace(_) => StoredMediaRestrip::Unchanged,
    })
}

fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*")
}

/// Why an upload cannot be stored without its metadata.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UnstrippableUpload {
    #[error(transparent)]
    Malformed(#[from] MalformedMedia),
    /// The declared type is one the stripper handles, but the bytes are not
    /// that kind of container.
    #[error("the bytes are not a {content_type} file")]
    NotTheDeclaredType { content_type: String },
    /// TIFF (and the camera raw formats built on it) is EXIF through and
    /// through; no lossless rewrite drops the tags without rewriting the file.
    #[error("TIFF files cannot be stored without their EXIF metadata")]
    Tiff,
}

/// Declared types whose bytes must be one of the image containers stripped.
const IMAGE_CONTAINER_TYPES: [&str; 7] = [
    "image/jpeg",
    "image/jpg",
    "image/png",
    "image/webp",
    "image/avif",
    "image/heic",
    "image/heif",
];

/// Declared types whose bytes must be one of the video containers stripped.
const VIDEO_CONTAINER_TYPES: [&str; 3] = ["video/mp4", "video/quicktime", "video/webm"];

/// The bytes to store for an upload declared as `content_type`: stripped of
/// their metadata whenever they are a known container, judged by the bytes,
/// and refused when they are damaged, when a declared image type is not an
/// image container, or when a declared video type is not a video container.
/// Anything else (GIF, MP3, Ogg, ...) passes through as uploaded.
pub fn strip_upload_metadata(
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, UnstrippableUpload> {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let content_type = content_type.as_str();
    let declared_image = IMAGE_CONTAINER_TYPES.contains(&content_type);
    let declared_video = VIDEO_CONTAINER_TYPES.contains(&content_type);
    let not_declared = || UnstrippableUpload::NotTheDeclaredType {
        content_type: content_type.to_owned(),
    };
    match strip_media_metadata(bytes)? {
        MetadataStrip::Stripped(stripped) => {
            let mismatch = if stripped.container.is_image() {
                declared_video
            } else {
                declared_image
            };
            if mismatch {
                Err(not_declared())
            } else {
                Ok(stripped.bytes)
            }
        }
        MetadataStrip::UnknownFormat(bytes) => {
            if is_tiff(&bytes) {
                Err(UnstrippableUpload::Tiff)
            } else if declared_image || declared_video {
                Err(not_declared())
            } else {
                Ok(bytes)
            }
        }
    }
}
