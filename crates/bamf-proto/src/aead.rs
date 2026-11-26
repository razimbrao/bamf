//! ChaCha20-Poly1305 AEAD as specified in RFC 8439.
//!
//! Implemented here (instead of pulling a crate) to keep the workspace free of
//! dependencies, and as a study exercise. It is checked against the RFC test
//! vectors below. Not hardened against every side channel; do not reuse it
//! outside this project.

const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// The ChaCha20 block function (RFC 8439, section 2.3).
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[..4].copy_from_slice(&SIGMA);
    for i in 0..8 {
        state[4 + i] = le32(&key[i * 4..]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = le32(&nonce[i * 4..]);
    }

    let mut w = state;
    for _ in 0..10 {
        quarter_round(&mut w, 0, 4, 8, 12);
        quarter_round(&mut w, 1, 5, 9, 13);
        quarter_round(&mut w, 2, 6, 10, 14);
        quarter_round(&mut w, 3, 7, 11, 15);
        quarter_round(&mut w, 0, 5, 10, 15);
        quarter_round(&mut w, 1, 6, 11, 12);
        quarter_round(&mut w, 2, 7, 8, 13);
        quarter_round(&mut w, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&w[i].wrapping_add(state[i]).to_le_bytes());
    }
    out
}

/// XORs `data` with the ChaCha20 key stream starting at block `counter`.
pub fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], data: &mut [u8]) {
    for (i, chunk) in data.chunks_mut(64).enumerate() {
        let stream = chacha20_block(key, counter.wrapping_add(i as u32), nonce);
        for (byte, k) in chunk.iter_mut().zip(stream) {
            *byte ^= k;
        }
    }
}

/// Poly1305 one-time authenticator (RFC 8439, section 2.5), using 26-bit
/// limbs so every product fits in a `u64`.
pub fn poly1305(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    const M: u32 = 0x3ff_ffff;
    let r0 = le32(&key[0..]) & 0x3ff_ffff;
    let r1 = (le32(&key[3..]) >> 2) & 0x3ff_ff03;
    let r2 = (le32(&key[6..]) >> 4) & 0x3ff_c0ff;
    let r3 = (le32(&key[9..]) >> 6) & 0x3f0_3fff;
    let r4 = (le32(&key[12..]) >> 8) & 0x00f_ffff;
    let (r0, r1, r2, r3, r4) = (r0 as u64, r1 as u64, r2 as u64, r3 as u64, r4 as u64);
    let (s1, s2, s3, s4) = (r1 * 5, r2 * 5, r3 * 5, r4 * 5);

    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (0u32, 0u32, 0u32, 0u32, 0u32);

    for chunk in msg.chunks(16) {
        // 16 message bytes plus the "1" byte appended to every block.
        let mut block = [0u8; 17];
        block[..chunk.len()].copy_from_slice(chunk);
        block[chunk.len()] = 1;

        h0 += le32(&block[0..]) & M;
        h1 += (le32(&block[3..]) >> 2) & M;
        h2 += (le32(&block[6..]) >> 4) & M;
        h3 += (le32(&block[9..]) >> 6) & M;
        h4 += (le32(&block[12..]) >> 8) | ((block[16] as u32) << 24);

        let (a0, a1, a2, a3, a4) = (h0 as u64, h1 as u64, h2 as u64, h3 as u64, h4 as u64);
        let d0 = a0 * r0 + a1 * s4 + a2 * s3 + a3 * s2 + a4 * s1;
        let mut d1 = a0 * r1 + a1 * r0 + a2 * s4 + a3 * s3 + a4 * s2;
        let mut d2 = a0 * r2 + a1 * r1 + a2 * r0 + a3 * s4 + a4 * s3;
        let mut d3 = a0 * r3 + a1 * r2 + a2 * r1 + a3 * r0 + a4 * s4;
        let mut d4 = a0 * r4 + a1 * r3 + a2 * r2 + a3 * r1 + a4 * r0;

        let mut c = d0 >> 26;
        h0 = d0 as u32 & M;
        d1 += c;
        c = d1 >> 26;
        h1 = d1 as u32 & M;
        d2 += c;
        c = d2 >> 26;
        h2 = d2 as u32 & M;
        d3 += c;
        c = d3 >> 26;
        h3 = d3 as u32 & M;
        d4 += c;
        c = d4 >> 26;
        h4 = d4 as u32 & M;
        h0 += c as u32 * 5;
        let c = h0 >> 26;
        h0 &= M;
        h1 += c;
    }

    // Full carry.
    let mut c = h1 >> 26;
    h1 &= M;
    h2 += c;
    c = h2 >> 26;
    h2 &= M;
    h3 += c;
    c = h3 >> 26;
    h3 &= M;
    h4 += c;
    c = h4 >> 26;
    h4 &= M;
    h0 += c * 5;
    c = h0 >> 26;
    h0 &= M;
    h1 += c;

    // g = h + 5 - 2^130; use g if it is not negative (i.e. h >= p).
    let mut g0 = h0.wrapping_add(5);
    c = g0 >> 26;
    g0 &= M;
    let mut g1 = h1.wrapping_add(c);
    c = g1 >> 26;
    g1 &= M;
    let mut g2 = h2.wrapping_add(c);
    c = g2 >> 26;
    g2 &= M;
    let mut g3 = h3.wrapping_add(c);
    c = g3 >> 26;
    g3 &= M;
    let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

    let use_g = (g4 >> 31).wrapping_sub(1); // all ones when g4 >= 0
    let keep_h = !use_g;
    h0 = (h0 & keep_h) | (g0 & use_g);
    h1 = (h1 & keep_h) | (g1 & use_g);
    h2 = (h2 & keep_h) | (g2 & use_g);
    h3 = (h3 & keep_h) | (g3 & use_g);
    h4 = (h4 & keep_h) | (g4 & use_g);

    // Pack into 128 bits and add s (the second half of the key).
    let w0 = h0 | (h1 << 26);
    let w1 = (h1 >> 6) | (h2 << 20);
    let w2 = (h2 >> 12) | (h3 << 14);
    let w3 = (h3 >> 18) | (h4 << 8);

    let mut out = [0u8; 16];
    let mut carry = 0u64;
    for (i, w) in [w0, w1, w2, w3].into_iter().enumerate() {
        let f = w as u64 + le32(&key[16 + i * 4..]) as u64 + carry;
        out[i * 4..i * 4 + 4].copy_from_slice(&(f as u32).to_le_bytes());
        carry = f >> 32;
    }
    out
}

fn mac_data(aad: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let pad = |n: usize| (16 - n % 16) % 16;
    let mut data = Vec::with_capacity(aad.len() + ciphertext.len() + 48);
    data.extend_from_slice(aad);
    data.resize(data.len() + pad(aad.len()), 0);
    data.extend_from_slice(ciphertext);
    data.resize(data.len() + pad(ciphertext.len()), 0);
    data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    data
}

fn one_time_key(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    let block = chacha20_block(key, 0, nonce);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&block[..32]);
    otk
}

/// Encrypts `plaintext` and returns `ciphertext || tag`.
pub fn seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let mut out = plaintext.to_vec();
    chacha20_xor(key, 1, nonce, &mut out);
    let tag = poly1305(&one_time_key(key, nonce), &mac_data(aad, &out));
    out.extend_from_slice(&tag);
    out
}

/// Verifies and decrypts `ciphertext || tag`. Returns `None` on failure.
pub fn open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 16 {
        return None;
    }
    let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
    let expected = poly1305(&one_time_key(key, nonce), &mac_data(aad, ciphertext));
    // Constant-time comparison.
    let diff = expected
        .iter()
        .zip(tag)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    if diff != 0 {
        return None;
    }
    let mut plain = ciphertext.to_vec();
    chacha20_xor(key, 1, nonce, &mut plain);
    Some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn arr<const N: usize>(v: &[u8]) -> [u8; N] {
        v.try_into().unwrap()
    }

    /// RFC 8439, section 2.3.2.
    #[test]
    fn chacha20_block_vector() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let nonce = arr(&hex("000000090000004a00000000"));
        let out = chacha20_block(&key, 1, &nonce);
        assert_eq!(
            out.to_vec(),
            hex(
                "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
                 d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
            )
        );
    }

    /// RFC 8439, section 2.5.2.
    #[test]
    fn poly1305_vector() {
        let key = arr(&hex(
            "85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b",
        ));
        let tag = poly1305(&key, b"Cryptographic Forum Research Group");
        assert_eq!(tag.to_vec(), hex("a8061dc1305136c6c22b8baf0c0127a9"));
    }

    /// RFC 8439, section 2.8.2.
    #[test]
    fn aead_vector() {
        let key: [u8; 32] = core::array::from_fn(|i| 0x80 + i as u8);
        let nonce = arr(&hex("070000004041424344454647"));
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let plaintext: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you \
only one tip for the future, sunscreen would be it.";
        let sealed = seal(&key, &nonce, &aad, plaintext);
        assert_eq!(sealed.len(), plaintext.len() + 16);
        assert_eq!(
            sealed[..16].to_vec(),
            hex("d31a8d34648e60db7b86afbc53ef7ec2")
        );
        assert_eq!(
            sealed[plaintext.len()..].to_vec(),
            hex("1ae10b594f09e26a7e902ecbd0600691")
        );
        assert_eq!(open(&key, &nonce, &aad, &sealed).unwrap(), plaintext);
    }

    #[test]
    fn open_rejects_tampering() {
        let key = [7u8; 32];
        let nonce = [1u8; 12];
        let mut sealed = seal(&key, &nonce, b"hdr", b"hello");
        assert!(open(&key, &nonce, b"hdr", &sealed).is_some());
        assert!(open(&key, &nonce, b"HDR", &sealed).is_none());
        sealed[0] ^= 1;
        assert!(open(&key, &nonce, b"hdr", &sealed).is_none());
        assert!(open(&key, &nonce, b"hdr", &[0u8; 5]).is_none());
    }

    /// Edge cases for the final reduction: messages of every length up to
    /// three blocks, with a key that maximises `r` and `s`. Expected tags were
    /// computed with an independent big-integer implementation
    /// (`scripts/poly1305_reference.py`).
    #[test]
    fn poly1305_lengths_match_reference() {
        let key = [0xffu8; 32];
        let msg = [0xffu8; 48];
        let mut acc = [0u8; 16];
        for len in 0..=48 {
            let tag = poly1305(&key, &msg[..len]);
            for (a, t) in acc.iter_mut().zip(tag) {
                *a ^= t;
            }
        }
        assert_eq!(acc.to_vec(), hex(REFERENCE_XOR_OF_TAGS));
    }

    const REFERENCE_XOR_OF_TAGS: &str = "e6f7f7979f3616fa810808f80f36163a";
}
