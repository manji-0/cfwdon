/// Returns true when the GIF contains more than one image frame.
pub(crate) fn gif_is_animated(bytes: &[u8]) -> bool {
    gif_image_frame_count(bytes) > 1
}

/// Returns a single-frame GIF suitable for `static_url`, or the original bytes when
/// the GIF is not animated or cannot be parsed.
pub(crate) fn gif_static_bytes(bytes: &[u8]) -> Vec<u8> {
    if !gif_is_animated(bytes) {
        return bytes.to_vec();
    }
    gif_first_frame_bytes(bytes).unwrap_or_else(|| bytes.to_vec())
}

fn gif_image_frame_count(bytes: &[u8]) -> usize {
    let Some(mut offset) = gif_header_end(bytes) else {
        return 0;
    };
    let mut frames = 0;

    while offset < bytes.len() {
        match bytes[offset] {
            0x21 => {
                let Some(next) = gif_skip_extension(bytes, offset + 1) else {
                    return frames;
                };
                offset = next;
            }
            0x2c => {
                frames += 1;
                let Some(next) = gif_skip_image(bytes, offset) else {
                    return frames;
                };
                offset = next;
            }
            0x3b => break,
            _ => return frames,
        }
    }

    frames
}

fn gif_first_frame_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut offset = gif_header_end(bytes)?;
    let start = 0;

    while offset < bytes.len() {
        match bytes[offset] {
            0x21 => offset = gif_skip_extension(bytes, offset + 1)?,
            0x2c => {
                offset = gif_skip_image(bytes, offset)?;
                let mut static_bytes = bytes[start..offset].to_vec();
                static_bytes.push(0x3b);
                return Some(static_bytes);
            }
            0x3b => break,
            _ => return None,
        }
    }

    None
}

fn gif_header_end(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 13 || !matches!(&bytes[..6], b"GIF87a" | b"GIF89a") {
        return None;
    }
    let packed = bytes[10];
    let mut offset = 13;
    if packed & 0x80 != 0 {
        let table_size = 3 * (1 << ((packed & 0x07) + 1));
        offset += table_size;
    }
    if offset <= bytes.len() {
        Some(offset)
    } else {
        None
    }
}

fn gif_skip_extension(bytes: &[u8], mut offset: usize) -> Option<usize> {
    if offset >= bytes.len() {
        return None;
    }
    offset += 1;
    gif_skip_sub_blocks(bytes, offset)
}

fn gif_skip_image(bytes: &[u8], offset: usize) -> Option<usize> {
    if offset + 10 > bytes.len() {
        return None;
    }
    let packed = bytes[offset + 9];
    let mut next = offset + 10;
    if packed & 0x80 != 0 {
        let table_size = 3 * (1 << ((packed & 0x07) + 1));
        next += table_size;
    }
    if next >= bytes.len() {
        return None;
    }
    next += 1;
    gif_skip_sub_blocks(bytes, next)
}

fn gif_skip_sub_blocks(bytes: &[u8], mut offset: usize) -> Option<usize> {
    while offset < bytes.len() {
        let block_size = bytes[offset] as usize;
        offset += 1;
        if block_size == 0 {
            return Some(offset);
        }
        offset += block_size;
        if offset > bytes.len() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{gif_first_frame_bytes, gif_is_animated, gif_static_bytes};

    fn minimal_gif(frame_count: usize) -> Vec<u8> {
        let mut bytes = vec![
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff,
            0xff, 0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let frame = [
            0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x4c, 0x01,
            0x00,
        ];
        for _ in 0..frame_count {
            bytes.extend_from_slice(&frame);
        }
        bytes.push(0x3b);
        bytes
    }

    #[test]
    fn single_frame_gif_is_not_animated() {
        let gif = minimal_gif(1);
        assert!(!gif_is_animated(&gif));
        assert_eq!(gif_static_bytes(&gif), gif);
    }

    #[test]
    fn multi_frame_gif_is_animated_and_static_is_shorter() {
        let gif = minimal_gif(2);
        assert!(gif_is_animated(&gif));
        let static_gif = gif_static_bytes(&gif);
        assert!(static_gif.len() < gif.len());
        assert!(!gif_is_animated(&static_gif));
        assert_eq!(static_gif.last().copied(), Some(0x3b));
        assert_eq!(gif_first_frame_bytes(&gif), Some(static_gif));
    }
}
