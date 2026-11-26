//! PNG encoding (8-bit RGB/RGBA) and decoding (all non-interlaced formats)
//! to and from 8-bit RGBA pixels.

use super::deflate::zlib_compress;
use super::inflate::zlib_decompress;
use super::{Error, Image, MAX_PIXELS};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, entry) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *entry = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8)
    })
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Encodes RGBA pixels. Fully opaque images are stored as RGB.
pub fn encode(img: &Image) -> Vec<u8> {
    let opaque = img.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let channels = if opaque { 3 } else { 4 };
    let stride = img.width as usize * channels;
    let raw: Vec<u8> = if opaque {
        img.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect()
    } else {
        img.rgba.clone()
    };

    // Per row, pick the filter with the smallest sum of absolute values
    // (the usual heuristic).
    let mut filtered = Vec::with_capacity((stride + 1) * img.height as usize);
    let zero = vec![0u8; stride];
    let mut candidates: [Vec<u8>; 5] = Default::default();
    for y in 0..img.height as usize {
        let row = &raw[y * stride..(y + 1) * stride];
        let up = if y == 0 {
            &zero[..]
        } else {
            &raw[(y - 1) * stride..y * stride]
        };
        for (f, out) in candidates.iter_mut().enumerate() {
            out.clear();
            for x in 0..stride {
                let a = if x >= channels { row[x - channels] } else { 0 };
                let b = up[x];
                let c = if x >= channels { up[x - channels] } else { 0 };
                out.push(match f {
                    0 => row[x],
                    1 => row[x].wrapping_sub(a),
                    2 => row[x].wrapping_sub(b),
                    3 => row[x].wrapping_sub(((a as u16 + b as u16) / 2) as u8),
                    _ => row[x].wrapping_sub(paeth(a, b, c)),
                });
            }
        }
        let score = |v: &Vec<u8>| {
            v.iter()
                .map(|&b| (b as i8).unsigned_abs() as u64)
                .sum::<u64>()
        };
        let (best, _) = candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| score(v))
            .expect("five candidates");
        filtered.push(best as u8);
        filtered.extend_from_slice(&candidates[best]);
    }

    let mut out = SIGNATURE.to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if opaque { 2 } else { 6 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_compress(&filtered));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Decodes any non-interlaced PNG into 8-bit RGBA.
pub fn decode(png: &[u8]) -> Result<Image, Error> {
    if png.len() < 8 || png[..8] != SIGNATURE {
        return Err(Error("png: bad signature"));
    }
    let mut pos = 8;
    let mut header = None;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    loop {
        let len = png
            .get(pos..pos + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize)
            .ok_or(Error("png: truncated"))?;
        let kind = png.get(pos + 4..pos + 8).ok_or(Error("png: truncated"))?;
        let data = png
            .get(pos + 8..pos + 8 + len)
            .ok_or(Error("png: truncated chunk"))?;
        pos += 12 + len;
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(Error("png: bad IHDR"));
                }
                let w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                let h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                if data[12] != 0 {
                    return Err(Error("png: interlaced images are not supported"));
                }
                header = Some((w, h, data[8], data[9]));
            }
            b"PLTE" => {
                palette = data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [c[0], c[1], c[2], 255])
                    .collect()
            }
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
    }
    let (width, height, depth, color) = header.ok_or(Error("png: missing IHDR"))?;
    if width == 0 || height == 0 || width as u64 * height as u64 > MAX_PIXELS {
        return Err(Error("png: bad or too large dimensions"));
    }
    let channels = match color {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(Error("png: bad color type")),
    };
    let valid_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !valid_depth {
        return Err(Error("png: bad bit depth"));
    }
    for (i, &a) in trns.iter().enumerate().take(palette.len()) {
        if color == 3 {
            palette[i][3] = a;
        }
    }

    let bits_pp = channels * depth as usize;
    let stride = (width as usize * bits_pp).div_ceil(8);
    let bpp = bits_pp.div_ceil(8).max(1);
    let expected = (stride + 1) * height as usize;
    let raw = zlib_decompress(&idat, expected)?;
    if raw.len() != expected {
        return Err(Error("png: wrong amount of image data"));
    }

    // Undo filters in place.
    let mut rows = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, rest) = rows.split_at_mut(y * stride);
        let up: &[u8] = if y == 0 {
            &[]
        } else {
            &done[(y - 1) * stride..]
        };
        let row = &mut rest[..stride];
        for x in 0..stride {
            let a = if x >= bpp { row[x - bpp] } else { 0 };
            let b = if y == 0 { 0 } else { up[x] };
            let c = if x >= bpp && y > 0 { up[x - bpp] } else { 0 };
            row[x] = src[x].wrapping_add(match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => paeth(a, b, c),
                _ => return Err(Error("png: bad filter")),
            });
        }
    }

    // Expand to RGBA8.
    let sample = |row: &[u8], i: usize| -> u16 {
        match depth {
            16 => u16::from_be_bytes([row[i * 2], row[i * 2 + 1]]),
            8 => row[i] as u16,
            d => {
                let per_byte = 8 / d as usize;
                let byte = row[i / per_byte];
                let shift = 8 - d as usize * (i % per_byte + 1);
                (byte >> shift) as u16 & ((1 << d) - 1)
            }
        }
    };
    let to8 = |v: u16| -> u8 {
        match depth {
            16 => (v >> 8) as u8,
            8 => v as u8,
            d => (v as u32 * 255 / ((1 << d) - 1)) as u8,
        }
    };
    let trns_key: Option<Vec<u16>> = (color != 3 && trns.len() >= 2).then(|| {
        trns.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect()
    });

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let row = &rows[y * stride..(y + 1) * stride];
        for x in 0..width as usize {
            let s = |c: usize| sample(row, x * channels + c);
            let px = match color {
                0 => {
                    let v = s(0);
                    let a = if trns_key.as_deref() == Some(&[v]) {
                        0
                    } else {
                        255
                    };
                    let g = to8(v);
                    [g, g, g, a]
                }
                2 => {
                    let (r, g, b) = (s(0), s(1), s(2));
                    let a = if trns_key.as_deref() == Some(&[r, g, b]) {
                        0
                    } else {
                        255
                    };
                    [to8(r), to8(g), to8(b), a]
                }
                3 => *palette
                    .get(s(0) as usize)
                    .ok_or(Error("png: palette index out of range"))?,
                4 => {
                    let g = to8(s(0));
                    [g, g, g, to8(s(1))]
                }
                _ => [to8(s(0)), to8(s(1)), to8(s(2)), to8(s(3))],
            };
            rgba.extend_from_slice(&px);
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::super::inflate::tests::hex;
    use super::*;

    fn gradient(w: u32, h: u32, alpha: bool) -> Image {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if alpha {
                    (x * 255 / w.max(1)) as u8
                } else {
                    255
                };
                rgba.extend_from_slice(&[(x * 3) as u8, (y * 5) as u8, (x ^ y) as u8, a]);
            }
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn roundtrips_rgb_and_rgba() {
        for img in [
            gradient(37, 23, false),
            gradient(64, 1, true),
            gradient(1, 1, true),
        ] {
            let png = encode(&img);
            assert_eq!(decode(&png).unwrap(), img);
        }
    }

    #[test]
    fn decodes_foreign_pngs() {
        // Written with Python's zlib and struct modules.
        // 2x2 RGBA: red, green / blue, transparent white.
        let png = hex(concat!(
            "89504e470d0a1a0a0000000d494844520000000200000002080600000072b60d24",
            "000000134944415478da63f8cfc0f01f0c81340830000048c908f8c534fd0500",
            "00000049454e44ae426082"
        ));
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(
            img.rgba,
            vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0]
        );

        // 3x1, 4-bit palette (red, green, blue) with tRNS (255, 128).
        let png = hex(concat!(
            "89504e470d0a1a0a0000000d4948445200000003000000010403000000e9ce0987",
            "00000009504c5445ff000000ff000000ff2d4acd8a0000000274524e53ff80080f",
            "b36a0000000b4944415478da636054000000250022e98287e50000000049454e",
            "44ae426082"
        ));
        assert_eq!(
            decode(&png).unwrap().rgba,
            vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255]
        );

        // 2x1, 16-bit grey: black, white.
        let png = hex(concat!(
            "89504e470d0a1a0a0000000d494844520000000200000001100000000081d9fc15",
            "0000000d4944415478da636060f8ff1f00030201ff392919be0000000049454e",
            "44ae426082"
        ));
        assert_eq!(
            decode(&png).unwrap().rgba,
            vec![0, 0, 0, 255, 255, 255, 255, 255]
        );
    }
    #[test]
    fn rejects_bad_input() {
        assert!(decode(b"not a png").is_err());
        let mut png = encode(&gradient(4, 4, false));
        let n = png.len();
        png.truncate(n - 20);
        assert!(decode(&png).is_err());
    }
}
