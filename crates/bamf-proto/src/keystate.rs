//! Bitmap of pressed keys, indexed by evdev key code (0–255).

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState([u8; KeyState::BYTES]);

impl KeyState {
    pub const BYTES: usize = 32;

    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; Self::BYTES] {
        &self.0
    }

    /// Codes above 255 are ignored (no common key uses them).
    pub fn set(&mut self, code: u16, down: bool) {
        if code > 255 {
            return;
        }
        let (byte, bit) = ((code / 8) as usize, code % 8);
        if down {
            self.0[byte] |= 1 << bit;
        } else {
            self.0[byte] &= !(1 << bit);
        }
    }

    pub fn is_down(&self, code: u16) -> bool {
        code <= 255 && self.0[(code / 8) as usize] & (1 << (code % 8)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }

    pub fn clear(&mut self) {
        self.0 = [0; Self::BYTES];
    }

    /// Iterates over all pressed key codes.
    pub fn pressed(&self) -> impl Iterator<Item = u16> + '_ {
        (0u16..=255).filter(|c| self.is_down(*c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_clear() {
        let mut s = KeyState::default();
        assert!(s.is_empty());
        s.set(42, true);
        s.set(0, true);
        s.set(300, true);
        assert!(s.is_down(42) && s.is_down(0) && !s.is_down(300));
        assert_eq!(s.pressed().collect::<Vec<_>>(), vec![0, 42]);
        s.set(42, false);
        assert!(!s.is_down(42));
        s.clear();
        assert!(s.is_empty());
    }
}
