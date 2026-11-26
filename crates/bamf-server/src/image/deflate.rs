//! DEFLATE compression with the fixed Huffman code and greedy LZ77 over a
//! hash chain. Not as tight as zlib, but screenshots, which are most
//! clipboard images, compress well with it.

use super::inflate::{adler32, DIST_BASE, DIST_EXTRA, LENGTH_BASE, LENGTH_EXTRA};

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
/// Candidates checked per position: speed over ratio.
const MAX_CHAIN: usize = 16;
const NONE: u32 = u32::MAX;

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        self.acc |= (value as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Huffman codes are sent most significant bit first.
    fn put_code(&mut self, code: u32, len: u32) {
        let reversed = code.reverse_bits() >> (32 - len);
        self.put(reversed, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn put_literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.put_code(0x30 + sym, 8),
        144..=255 => w.put_code(0x190 + sym - 144, 9),
        256..=279 => w.put_code(sym - 256, 7),
        _ => w.put_code(0xC0 + sym - 280, 8),
    }
}

/// Index of the largest base not above `value`.
fn bucket(bases: &[u16], value: usize) -> usize {
    bases
        .iter()
        .rposition(|&b| b as usize <= value)
        .unwrap_or(0)
}

fn put_match(w: &mut BitWriter, len: usize, dist: usize) {
    let l = bucket(LENGTH_BASE, len);
    put_literal(w, 257 + l as u32);
    w.put(
        (len - LENGTH_BASE[l] as usize) as u32,
        LENGTH_EXTRA[l] as u32,
    );
    let d = bucket(DIST_BASE, dist);
    w.put_code(d as u32, 5);
    w.put((dist - DIST_BASE[d] as usize) as u32, DIST_EXTRA[d] as u32);
}

fn hash(data: &[u8], i: usize) -> usize {
    let v = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
    (v.wrapping_mul(2654435761) >> (32 - HASH_BITS)) as usize
}

/// Raw DEFLATE stream (one final fixed-Huffman block).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter {
        out: Vec::with_capacity(data.len() / 4 + 16),
        acc: 0,
        n: 0,
    };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // BTYPE = fixed Huffman

    let mut head = vec![NONE; 1 << HASH_BITS];
    let mut prev = vec![NONE; WINDOW];
    let insert = |head: &mut [u32], prev: &mut [u32], i: usize| {
        if i + MIN_MATCH <= data.len() {
            let h = hash(data, i);
            prev[i % WINDOW] = head[h];
            head[h] = i as u32;
        }
    };

    let mut i = 0;
    while i < data.len() {
        let mut best_len = 0;
        let mut best_dist = 0;
        if i + MIN_MATCH <= data.len() {
            let max = (data.len() - i).min(MAX_MATCH);
            let mut cand = head[hash(data, i)];
            let mut tries = 0;
            while cand != NONE && tries < MAX_CHAIN {
                let c = cand as usize;
                let dist = i - c;
                if dist > WINDOW || dist == 0 {
                    break;
                }
                let len = data[c..c + max]
                    .iter()
                    .zip(&data[i..i + max])
                    .take_while(|(a, b)| a == b)
                    .count();
                if len > best_len {
                    best_len = len;
                    best_dist = dist;
                    if len == max {
                        break;
                    }
                }
                cand = prev[c % WINDOW];
                tries += 1;
            }
        }

        if best_len >= MIN_MATCH {
            put_match(&mut w, best_len, best_dist);
            for k in i..i + best_len {
                insert(&mut head, &mut prev, k);
            }
            i += best_len;
        } else {
            put_literal(&mut w, data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    put_literal(&mut w, 256);
    w.finish()
}

/// zlib stream (RFC 1950) around [`deflate`].
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::super::inflate::zlib_decompress;
    use super::*;

    fn roundtrip(data: &[u8]) -> usize {
        let z = zlib_compress(data);
        assert_eq!(zlib_decompress(&z, data.len()).unwrap(), data);
        z.len()
    }

    #[test]
    fn roundtrips() {
        roundtrip(b"");
        roundtrip(b"a");
        roundtrip(b"hello hello hello hello");
        let all: Vec<u8> = (0..=255).collect();
        roundtrip(&all);
        // Matches across the whole window and at maximum length.
        let mut big = Vec::new();
        for i in 0..200_000u32 {
            big.push((i % 251) as u8 ^ (i / 7000) as u8);
        }
        roundtrip(&big);
    }

    #[test]
    fn compresses_repetitive_data() {
        let flat = vec![0x42u8; 1 << 20];
        assert!(roundtrip(&flat) < 10_000);
    }
}
