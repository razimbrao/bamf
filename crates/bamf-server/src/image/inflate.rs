//! DEFLATE decompression (RFC 1951) and the zlib wrapper (RFC 1950).
//! Canonical Huffman decoding one bit at a time, in the style of zlib's
//! `puff.c`: simple and fast enough for clipboard images.

use super::Error;

const LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEXT: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

pub(super) const LENGTH_BASE: &[u16; 29] = &LBASE;
pub(super) const LENGTH_EXTRA: &[u8; 29] = &LEXT;
pub(super) const DIST_BASE: &[u16; 30] = &DBASE;
pub(super) const DIST_EXTRA: &[u8; 30] = &DEXT;

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bitbuf: u32,
    bitcnt: u32,
}

impl Bits<'_> {
    fn bits(&mut self, need: u32) -> Result<u32, Error> {
        let mut val = self.bitbuf;
        while self.bitcnt < need {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or(Error("deflate: unexpected end"))?;
            self.pos += 1;
            val |= (byte as u32) << self.bitcnt;
            self.bitcnt += 8;
        }
        self.bitbuf = if need == 32 { 0 } else { val >> need };
        self.bitcnt -= need;
        Ok(if need == 32 {
            val
        } else {
            val & ((1 << need) - 1)
        })
    }

    fn align(&mut self) {
        self.bitbuf = 0;
        self.bitcnt = 0;
    }
}

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, Error> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        // Over-subscribed sets are invalid; incomplete ones are allowed (a
        // single distance code is common).
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return Err(Error("deflate: over-subscribed code"));
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        count[0] = 0;
        Ok(Self { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, Error> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - first < count {
                return Ok(self.symbol[(index + code - first) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Error("deflate: invalid code"))
    }
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    let lit = Huffman::new(&lengths).expect("fixed literal code");
    let dist = Huffman::new(&[5u8; 30]).expect("fixed distance code");
    (lit, dist)
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), Error> {
    let nlen = bits.bits(5)? as usize + 257;
    let ndist = bits.bits(5)? as usize + 1;
    let ncode = bits.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(Error("deflate: bad table counts"));
    }
    let mut clen = [0u8; 19];
    for &i in &CLEN_ORDER[..ncode] {
        clen[i] = bits.bits(3)? as u8;
    }
    let clen = Huffman::new(&clen)?;

    let mut lengths = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < lengths.len() {
        let sym = clen.decode(bits)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths[..i]
                    .last()
                    .ok_or(Error("deflate: repeat with no length"))?;
                (prev, 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if i + repeat > lengths.len() {
            return Err(Error("deflate: too many lengths"));
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(Error("deflate: no end-of-block code"));
    }
    Ok((
        Huffman::new(&lengths[..nlen])?,
        Huffman::new(&lengths[nlen..])?,
    ))
}

fn codes(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), Error> {
    loop {
        let sym = lit.decode(bits)? as usize;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            _ => {
                let s = sym - 257;
                if s >= 29 {
                    return Err(Error("deflate: bad length symbol"));
                }
                let len = LBASE[s] as usize + bits.bits(LEXT[s] as u32)? as usize;
                let d = dist.decode(bits)? as usize;
                if d >= 30 {
                    return Err(Error("deflate: bad distance symbol"));
                }
                let distance = DBASE[d] as usize + bits.bits(DEXT[d] as u32)? as usize;
                if distance > out.len() {
                    return Err(Error("deflate: distance too far back"));
                }
                let start = out.len() - distance;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
        }
        if out.len() > limit {
            return Err(Error("deflate: output larger than expected"));
        }
    }
}

/// Raw DEFLATE. Returns the output and how many input bytes were used.
pub fn inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), Error> {
    let mut bits = Bits {
        data,
        pos: 0,
        bitbuf: 0,
        bitcnt: 0,
    };
    let mut out = Vec::new();
    let (fixed_lit, fixed_dist) = fixed_tables();
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let p = bits.pos;
                let hdr = data
                    .get(p..p + 4)
                    .ok_or(Error("deflate: truncated stored block"))?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
                if len != !nlen & 0xffff {
                    return Err(Error("deflate: bad stored block length"));
                }
                let block = data
                    .get(p + 4..p + 4 + len)
                    .ok_or(Error("deflate: truncated stored block"))?;
                out.extend_from_slice(block);
                bits.pos = p + 4 + len;
                if out.len() > limit {
                    return Err(Error("deflate: output larger than expected"));
                }
            }
            1 => codes(&mut bits, &mut out, &fixed_lit, &fixed_dist, limit)?,
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err(Error("deflate: invalid block type")),
        }
        if last {
            return Ok((out, bits.pos));
        }
    }
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

/// zlib stream → bytes, refusing output above `limit`.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    if data.len() < 6 {
        return Err(Error("zlib: too short"));
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0f != 8 || !(cmf as u16 * 256 + flg as u16).is_multiple_of(31) || flg & 0x20 != 0 {
        return Err(Error("zlib: bad header"));
    }
    let (out, used) = inflate(&data[2..], limit)?;
    let trailer = data
        .get(2 + used..2 + used + 4)
        .ok_or(Error("zlib: missing checksum"))?;
    if u32::from_be_bytes(trailer.try_into().unwrap()) != adler32(&out) {
        return Err(Error("zlib: checksum mismatch"));
    }
    Ok(out)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn decodes_reference_streams() {
        // python3 -c "import zlib; print(zlib.compress(b'hello hello hello hello').hex())"
        let z = hex("789ccb48cdc9c957c8402701680308b1");
        assert_eq!(
            zlib_decompress(&z, 1 << 20).unwrap(),
            b"hello hello hello hello"
        );
        // Stored block (level 0): zlib.compress(b'abc', 0)
        let z = hex("7801010300fcff616263024d0127");
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), b"abc");
    }

    #[test]
    fn decodes_dynamic_huffman() {
        // random.seed(1); d = bytes(random.choice(b'aaaaaaaabbbbccd ')
        // for _ in range(600)); zlib.compress(d, 9) -> one dynamic block.
        let z = hex(concat!(
            "78da45518111c340085ac5d500f79fa180dff673b97c541014206647c040c2120ef8c80f76f2e398",
            "12a540bab28524d6551cff3ad123f26a5de3ef50066d53e5dc9020d0d2b978c637851805f65d1db5",
            "708501e811a0c16a601a3faa441378873d7186f20a8f3ed1af459d4d54a0ac7d5ec2c2d23cd2cdc0",
            "8b866ddba5d2cfd09313489dbce62f861b8c71e26e91735252a4e4a65d10b4f873fb667bf8d16361",
            "07a0cc7e03bc253c03fc42ad71f4d7d54e61decda5ab3b45e7c6beb3c0cce8c9ca80e692b57c7b3d",
            "5f27365dd5c1f0bfa6bd4da74bfb7e00fc17dd24"
        ));
        assert_eq!((z[2] >> 1) & 3, 2, "vector must use a dynamic block");
        let out = zlib_decompress(&z, 1 << 20).unwrap(); // Adler-32 checked
        assert_eq!(out.len(), 600);
        assert!(out.starts_with(b"aaba d caa accadbaabaaaacacaad abaadbaca"));

        assert!(zlib_decompress(&[0x78, 0x9c, 0xff, 0xff, 0, 0, 0, 0], 100).is_err());
        assert!(zlib_decompress(&[0x78, 0x9d, 0, 0, 0, 0], 100).is_err());
    }

    #[test]
    fn respects_the_limit() {
        let z = hex("789ccb48cdc9c957c8402701680308b1");
        assert!(zlib_decompress(&z, 5).is_err());
    }

    #[test]
    fn adler32_known_value() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E60398);
    }

    pub(crate) fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
