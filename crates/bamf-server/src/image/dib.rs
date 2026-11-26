//! Windows device-independent bitmaps (the `CF_DIB` clipboard format):
//! a BITMAPINFOHEADER (or V4/V5 header) followed by the pixels.

use super::{Error, Image, MAX_PIXELS};

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

/// Reads a 24- or 32-bit DIB. 32-bit DIBs whose alpha is all zero (the
/// common case: the channel is simply unused) are treated as opaque.
pub fn decode(dib: &[u8]) -> Result<Image, Error> {
    if dib.len() < 40 {
        return Err(Error("dib: too short"));
    }
    let header = u32_at(dib, 0) as usize;
    if header < 40 || header > dib.len() {
        return Err(Error("dib: bad header size"));
    }
    let width = i32::from_le_bytes(dib[4..8].try_into().unwrap());
    let height = i32::from_le_bytes(dib[8..12].try_into().unwrap());
    let bits = u16_at(dib, 14) as usize;
    let compression = u32_at(dib, 16);
    let colors_used = u32_at(dib, 32) as usize;
    if width <= 0 || height == 0 {
        return Err(Error("dib: bad dimensions"));
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    if (w * h) as u64 > MAX_PIXELS {
        return Err(Error("dib: too large"));
    }
    if !(bits == 24 || bits == 32) {
        return Err(Error("dib: only 24- and 32-bit bitmaps are supported"));
    }

    // Channel masks: in the header for V4/V5, right after it for a plain
    // BITMAPINFOHEADER with BI_BITFIELDS.
    let mut masks = [0x00ff0000u32, 0x0000ff00, 0x000000ff, 0xff000000];
    let mut offset = header;
    match compression {
        BI_RGB => {}
        BI_BITFIELDS if bits == 32 => {
            let at = if header >= 52 { 40 } else { header };
            if dib.len() < at + 12 {
                return Err(Error("dib: missing masks"));
            }
            masks[0] = u32_at(dib, at);
            masks[1] = u32_at(dib, at + 4);
            masks[2] = u32_at(dib, at + 8);
            masks[3] = if header >= 56 { u32_at(dib, 52) } else { 0 };
            if header == 40 {
                offset += 12;
            }
        }
        _ => return Err(Error("dib: compressed bitmaps are not supported")),
    }
    offset += colors_used * 4;

    let stride = (w * bits).div_ceil(32) * 4;
    let pixels = dib
        .get(offset..offset + stride * h)
        .ok_or(Error("dib: truncated pixel data"))?;

    let channel = |px: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let max = mask >> shift;
        (((px & mask) >> shift) * 255 / max) as u8
    };

    let mut rgba = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        // Positive height: bottom-up rows.
        let src_row = if height > 0 { h - 1 - y } else { y };
        let row = &pixels[src_row * stride..];
        for x in 0..w {
            let out = &mut rgba[(y * w + x) * 4..][..4];
            if bits == 24 {
                let p = &row[x * 3..x * 3 + 3];
                out.copy_from_slice(&[p[2], p[1], p[0], 255]);
            } else {
                let px = u32_at(row, x * 4);
                let a = if masks[3] == 0 && compression == BI_RGB {
                    (px >> 24) as u8
                } else {
                    channel(px, masks[3])
                };
                any_alpha |= a != 0;
                out.copy_from_slice(&[
                    channel(px, masks[0]),
                    channel(px, masks[1]),
                    channel(px, masks[2]),
                    a,
                ]);
            }
        }
    }
    if bits == 32 && !any_alpha {
        for px in rgba.as_chunks_mut::<4>().0.iter_mut() {
            px[3] = 255;
        }
    }
    Ok(Image {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

/// 32-bit bottom-up DIB with a plain BITMAPINFOHEADER.
pub fn encode(img: &Image) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut out = Vec::with_capacity(40 + w * h * 4);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&BI_RGB.to_le_bytes());
    out.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution, colours
    for y in (0..h).rev() {
        for p in img.rgba[y * w * 4..(y + 1) * w * 4]
            .as_chunks::<4>()
            .0
            .iter()
        {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Image {
        Image {
            width: 3,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, //
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 0,
            ],
        }
    }

    #[test]
    fn roundtrips_32_bit() {
        let img = sample();
        assert_eq!(decode(&encode(&img)).unwrap(), img);
    }

    #[test]
    fn zero_alpha_means_opaque() {
        let mut img = sample();
        for px in img.rgba.as_chunks_mut::<4>().0.iter_mut() {
            px[3] = 0;
        }
        let back = decode(&encode(&img)).unwrap();
        assert!(back.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn reads_24_bit_padded_rows() {
        // 1x2, bottom-up: rows are padded to 4 bytes.
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0u8; 24]);
        dib.extend_from_slice(&[3, 2, 1, 0]); // bottom row: BGR + pad
        dib.extend_from_slice(&[6, 5, 4, 0]); // top row
        let img = decode(&dib).unwrap();
        assert_eq!(img.rgba, vec![4, 5, 6, 255, 1, 2, 3, 255]);
    }

    #[test]
    fn reads_bitfields() {
        let img = sample();
        let mut dib = encode(&img);
        dib[16..20].copy_from_slice(&BI_BITFIELDS.to_le_bytes());
        let masks: Vec<u8> = [0x00ff0000u32, 0x0000ff00, 0x000000ff]
            .iter()
            .flat_map(|m| m.to_le_bytes())
            .collect();
        dib.splice(40..40, masks);
        let back = decode(&dib).unwrap();
        // No alpha mask with BITFIELDS in a v1 header: opaque.
        assert!(back.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        assert_eq!(&back.rgba[..3], &[255, 0, 0]);
    }
}
