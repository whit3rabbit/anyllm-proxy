//! Minimal deterministic PNG encoder (grayscale + RGB, 8-bit, filter=None,
//! single IDAT). Port of pxpipe's `png.ts`.
//!
//! Determinism is the whole point: the Anthropic prompt cache keys on the exact
//! image bytes, so the same pixels must always encode to the same file. We
//! hand-assemble the chunks (CRC32 + length prefixes) and deflate the raw
//! scanlines with `miniz_oxide` at a fixed level. No timestamps, no ancillary
//! chunks, no adaptive filtering — byte-stable by construction. The golden test
//! in `tests/` pins the output sha.

/// Fixed zlib compression level. Level 6 is miniz_oxide's default balance; the
/// exact value does not matter for correctness, only that it never changes
/// (changing it re-encodes every cached image once). Pinned deliberately.
const ZLIB_LEVEL: u8 = 6;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// CRC-32 (IEEE, as PNG specifies) computed on the fly — no static table so the
/// binary carries no extra data. PNG chunks are small; this is not hot.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Assemble a PNG from already-filtered scanlines (each row prefixed with its
/// filter byte). `color_type` is 0 (grayscale) or 2 (truecolor RGB).
fn assemble(width: u32, height: u32, color_type: u8, filtered: &[u8]) -> Vec<u8> {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(color_type);
    ihdr.push(0); // compression
    ihdr.push(0); // filter method
    ihdr.push(0); // interlace

    let idat = miniz_oxide::deflate::compress_to_vec_zlib(filtered, ZLIB_LEVEL);

    let mut out = Vec::with_capacity(idat.len() + 64);
    out.extend_from_slice(&PNG_SIGNATURE);
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// Encode a single-channel (grayscale) framebuffer, row-major, len = w*h.
pub fn encode_gray(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    debug_assert_eq!(pixels.len(), (width * height) as usize);
    let w = width as usize;
    let mut filtered = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(w) {
        filtered.push(0); // filter: None
        filtered.extend_from_slice(row);
    }
    assemble(width, height, 0, &filtered)
}

/// Encode an RGB framebuffer (3 bytes/pixel), row-major, len = w*h*3.
#[allow(dead_code)] // P4+ per-role color rendering
pub fn encode_rgb(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    debug_assert_eq!(pixels.len(), (width * height * 3) as usize);
    let stride = width as usize * 3;
    let mut filtered = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(stride) {
        filtered.push(0); // filter: None
        filtered.extend_from_slice(row);
    }
    assemble(width, height, 2, &filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // CRC-32 of "IEND" per the PNG spec's empty-IEND chunk is 0xAE426082.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn gray_roundtrip_is_deterministic() {
        let px = vec![0u8, 255, 128, 64];
        let a = encode_gray(&px, 2, 2);
        let b = encode_gray(&px, 2, 2);
        assert_eq!(a, b, "same pixels must encode to identical bytes");
        assert_eq!(&a[..8], &PNG_SIGNATURE);
    }
}
