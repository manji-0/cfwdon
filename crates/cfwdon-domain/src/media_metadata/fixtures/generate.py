#!/usr/bin/env python3
"""Regenerate the real-encoder fixtures for the media metadata tests.

Needs Pillow (with AVIF and WebP support) and ffmpeg (libx264, libvpx).
Every file carries the strings the tests look for after stripping:
`secret-camera`, `secret-gps`, `secret-xmp`, `secret-comment`, `secret-title`.
Run from anywhere: `python3 generate.py`.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

from PIL import Image, ImageCms
from PIL.PngImagePlugin import PngInfo

HERE = Path(__file__).resolve().parent

XMP = (
    b'<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>'
    b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
    b'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
    b'<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" '
    b'dc:creator="secret-xmp"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>'
)


def gradient(width: int, height: int) -> Image.Image:
    image = Image.new("RGB", (width, height))
    image.putdata(
        [
            (x * 255 // width, y * 255 // height, (x + y) * 127 // (width + height))
            for y in range(height)
            for x in range(width)
        ]
    )
    return image


def exif(orientation: int) -> Image.Exif:
    data = Image.Exif()
    data[0x010F] = "secret-camera"  # Make
    data[0x0112] = orientation  # Orientation
    data[0x0132] = "2024:01:02 03:04:05"  # DateTime
    gps = data.get_ifd(0x8825)
    gps[1] = "N"  # GPSLatitudeRef
    gps[2] = (35.0, 39.0, 31.0)  # GPSLatitude
    gps[3] = "E"  # GPSLongitudeRef
    gps[4] = (139.0, 44.0, 43.0)  # GPSLongitude
    gps[18] = "secret-gps"  # GPSMapDatum
    return data


def images() -> None:
    icc = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    photo = gradient(64, 48)
    photo.save(
        HERE / "photo.jpg",
        quality=80,
        exif=exif(6),
        icc_profile=icc,
        xmp=XMP,
        comment=b"secret-comment",
    )
    info = PngInfo()
    info.add_text("Comment", "secret-comment")
    info.add_itxt("XML:com.adobe.xmp", XMP.decode())
    small = gradient(16, 12)
    small.save(HERE / "photo.png", exif=exif(8), icc_profile=icc, pnginfo=info)
    small.save(HERE / "photo.webp", quality=80, exif=exif(3), icc_profile=icc, xmp=XMP)
    small.save(HERE / "photo.avif", quality=60, exif=exif(1), icc_profile=icc, xmp=XMP)


def ffmpeg(*args: str) -> None:
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args], check=True
    )


def videos() -> None:
    source = ["-f", "lavfi", "-i", "testsrc=size=16x16:rate=2:duration=1"]
    tags = [
        "-metadata",
        "title=secret-title",
        "-metadata",
        "comment=secret-comment",
        "-metadata",
        "location=+35.6586+139.7454/",
        "-metadata",
        "creation_time=2024-01-02T03:04:05Z",
    ]
    x264 = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]
    ffmpeg(*source, *x264, *tags, "-movflags", "+use_metadata_tags", str(HERE / "clip.mp4"))
    ffmpeg(*source, *x264, *tags, str(HERE / "clip.mov"))
    vp8 = ["-c:v", "libvpx", "-b:v", "20k"]
    ffmpeg(*source, *vp8, *tags, str(HERE / "clip.webm"))
    # A live recording, as MediaRecorder writes one: a Segment of unknown size.
    ffmpeg(*source, *vp8, *tags, "-live", "1", "-f", "webm", str(HERE / "clip-live.webm"))


if __name__ == "__main__":
    images()
    videos()
