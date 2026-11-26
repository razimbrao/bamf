//! Tray icon artwork: a 16x16 pixel-art hooded imp drawn from a character
//! map, the same art as `packaging/windows/icons`. Pure logic, no OS calls.

/// Which face the imp shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Eyes lit: bamf is active.
    Awake,
    /// Eyes closed: waiting for the client.
    Dozing,
    /// Eyes closed and a "z": paused.
    Asleep,
}

const N: usize = 16;

/// K outline, P hood, p hood highlight, D hood shadow, S face shadow,
/// E eyes, G fangs.
const ART: [&str; N] = [
    "......KKKK......",
    "....KKPPPPKK....",
    "...KPPPPPPPPK...",
    "..KPPpPPPPPPPK..",
    ".KPPpKKKKKKPPPK.",
    ".KPpKSSSSSSKPPK.",
    "KPpKSSSSSSSSKPPK",
    "KPpKEESSSSEEKPPK",
    "KPpKEEESSEEEKPPK",
    "KPPKSEESSEESKPPK",
    "KPPKSSSSSSSSKPPK",
    "KPPPKSGSSGSKPPPK",
    "KPPPPKSSSSKPPPPK",
    "KPPPPPKKKKPPPPPK",
    ".KDDPPPPPPPPDDK.",
    "..KKKKKKKKKKKK..",
];

/// The only row of the eyes that stays visible, as a slit, when closed.
const SLIT_ROW: usize = 8;

/// Pixels of the "z" drawn in the top-right corner while asleep.
const Z: [(usize, usize); 8] = [
    (13, 0),
    (14, 0),
    (15, 0),
    (15, 1),
    (14, 2),
    (13, 3),
    (14, 3),
    (15, 3),
];

type Rgb = [u8; 3];

fn awake(ch: u8) -> Rgb {
    match ch {
        b'K' => [0x10, 0x14, 0x2a],
        b'P' => [0x4b, 0x3c, 0x9e],
        b'p' => [0x8a, 0x74, 0xe0],
        b'D' => [0x2f, 0x24, 0x68],
        b'S' => [0x0b, 0x0e, 0x1e],
        b'E' => [0xff, 0xe2, 0x3a],
        _ => [0xff, 0xf2, 0xa8], // G
    }
}

fn asleep(ch: u8) -> Rgb {
    match ch {
        b'K' => [0x14, 0x16, 0x1f],
        b'P' => [0x4a, 0x50, 0x68],
        b'p' => [0x71, 0x78, 0x92],
        b'D' => [0x34, 0x39, 0x49],
        b'S' => [0x0e, 0x10, 0x18],
        b'E' => [0x8d, 0x94, 0xad],
        _ => [0x5c, 0x63, 0x80], // G
    }
}

const Z_COLOR: Rgb = [0xc3, 0xca, 0xe6];

fn at(x: isize, y: isize) -> u8 {
    if !(0..N as isize).contains(&x) || !(0..N as isize).contains(&y) {
        return b'.';
    }
    ART[y as usize].as_bytes()[x as usize]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    [m(0), m(1), m(2)]
}

/// The 16x16 art as RGBA, or `None` for transparent pixels.
fn pixel(look: Look, x: usize, y: usize) -> Option<Rgb> {
    if look == Look::Asleep && Z.contains(&(x, y)) {
        return Some(Z_COLOR);
    }
    let mut ch = at(x as isize, y as isize);
    if ch == b'.' {
        return None;
    }
    if look == Look::Awake {
        let mut c = awake(ch);
        if !matches!(ch, b'K' | b'E' | b'G') {
            // Glow from the eyes and fangs onto the pixels next to them.
            let (x, y) = (x as isize, y as isize);
            let near = [(0, -1), (-1, 0), (1, 0), (0, 1)].map(|(dx, dy)| at(x + dx, y + dy));
            if near.contains(&b'E') {
                c = mix(c, awake(b'E'), 0.4);
            } else if near.contains(&b'G') {
                c = mix(c, awake(b'G'), 0.2);
            }
        }
        Some(c)
    } else {
        if ch == b'E' && y != SLIT_ROW {
            ch = b'S';
        }
        Some(asleep(ch))
    }
}

/// Renders the icon at `size` x `size` pixels (nearest neighbour, so the
/// pixel art stays crisp), as RGBA with straight alpha, top row first.
pub fn render(look: Look, size: usize) -> Vec<u8> {
    let size = size.max(1);
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            if let Some([r, g, b]) = pixel(look, x * N / size, y * N / size) {
                let i = (y * size + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[r, g, b, 0xff]);
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_at(img: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn art_is_square() {
        assert!(ART.iter().all(|r| r.len() == N));
    }

    #[test]
    fn corners_are_transparent() {
        let img = render(Look::Awake, 16);
        assert_eq!(rgba_at(&img, 16, 0, 0)[3], 0);
        assert_eq!(rgba_at(&img, 16, 15, 15)[3], 0);
    }

    #[test]
    fn eyes_glow_only_when_awake() {
        let eye = |look| rgba_at(&render(look, 16), 16, 4, 7);
        assert_eq!(eye(Look::Awake), [0xff, 0xe2, 0x3a, 0xff]);
        assert_ne!(eye(Look::Dozing), eye(Look::Awake));
    }

    #[test]
    fn z_only_when_asleep() {
        let z = |look| rgba_at(&render(look, 16), 16, 14, 0);
        assert_eq!(z(Look::Asleep), [0xc3, 0xca, 0xe6, 0xff]);
        assert_eq!(z(Look::Dozing)[3], 0);
        assert_eq!(z(Look::Awake)[3], 0);
    }

    #[test]
    fn scales_by_whole_pixels() {
        let small = render(Look::Awake, 16);
        let big = render(Look::Awake, 32);
        assert_eq!(big.len(), 32 * 32 * 4);
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(rgba_at(&big, 32, x, y), rgba_at(&small, 16, x / 2, y / 2));
            }
        }
    }
}
