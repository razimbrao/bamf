//! Windows scan codes to Linux evdev key codes.
//!
//! For PC set-1 scan codes 0x01..=0x58 the evdev code has the same value, so
//! only extended (E0-prefixed) keys and a few special keys need a table.

const VK_PAUSE: u32 = 0x13;
const VK_NUMLOCK: u32 = 0x90;

/// Converts a low-level keyboard hook event into an evdev key code.
pub fn to_evdev(scan_code: u32, extended: bool, vk: u32) -> Option<u16> {
    // Pause and Num Lock share scan code 0x45; Windows tells them apart by
    // the virtual key.
    match vk {
        VK_PAUSE => return Some(119),  // KEY_PAUSE
        VK_NUMLOCK => return Some(69), // KEY_NUMLOCK
        _ => {}
    }

    if extended {
        let code = match scan_code {
            // Some systems flag Right Shift as extended in low-level hooks.
            0x36 => 54,  // KEY_RIGHTSHIFT
            0x1C => 96,  // KEY_KPENTER
            0x1D => 97,  // KEY_RIGHTCTRL
            0x35 => 98,  // KEY_KPSLASH
            0x37 => 99,  // KEY_SYSRQ (Print Screen)
            0x38 => 100, // KEY_RIGHTALT (AltGr)
            0x47 => 102, // KEY_HOME
            0x48 => 103, // KEY_UP
            0x49 => 104, // KEY_PAGEUP
            0x4B => 105, // KEY_LEFT
            0x4D => 106, // KEY_RIGHT
            0x4F => 107, // KEY_END
            0x50 => 108, // KEY_DOWN
            0x51 => 109, // KEY_PAGEDOWN
            0x52 => 110, // KEY_INSERT
            0x53 => 111, // KEY_DELETE
            0x5B => 125, // KEY_LEFTMETA
            0x5C => 126, // KEY_RIGHTMETA
            0x5D => 127, // KEY_COMPOSE (menu)
            // Media keys.
            0x10 => 165, // KEY_PREVIOUSSONG
            0x19 => 163, // KEY_NEXTSONG
            0x20 => 113, // KEY_MUTE
            0x22 => 164, // KEY_PLAYPAUSE
            0x24 => 166, // KEY_STOPCD
            0x2E => 114, // KEY_VOLUMEDOWN
            0x30 => 115, // KEY_VOLUMEUP
            _ => return None,
        };
        return Some(code);
    }

    match scan_code {
        0x01..=0x58 => Some(scan_code as u16),
        0x70 => Some(93),  // KEY_KATAKANAHIRAGANA
        0x73 => Some(89),  // KEY_RO: ABNT2 "/?" key
        0x79 => Some(92),  // KEY_HENKAN
        0x7B => Some(94),  // KEY_MUHENKAN
        0x7D => Some(124), // KEY_YEN
        0x7E => Some(121), // KEY_KPCOMMA: ABNT2 keypad "."
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_keys_map_one_to_one() {
        assert_eq!(to_evdev(0x01, false, 0x1B), Some(1)); // Esc
        assert_eq!(to_evdev(0x1E, false, 0x41), Some(30)); // A
        assert_eq!(to_evdev(0x58, false, 0x7B), Some(88)); // F12
        assert_eq!(to_evdev(0x1D, false, 0xA2), Some(29)); // Left Ctrl
    }

    #[test]
    fn extended_keys() {
        assert_eq!(to_evdev(0x1D, true, 0xA3), Some(97)); // Right Ctrl
        assert_eq!(to_evdev(0x38, true, 0xA5), Some(100)); // AltGr
        assert_eq!(to_evdev(0x48, true, 0x26), Some(103)); // Up
        assert_eq!(to_evdev(0x48, false, 0x68), Some(0x48)); // Keypad 8
        assert_eq!(to_evdev(0x5B, true, 0x5B), Some(125)); // Win
    }

    #[test]
    fn abnt2_and_special_keys() {
        assert_eq!(to_evdev(0x73, false, 0xC1), Some(89)); // ABNT2 /?
        assert_eq!(to_evdev(0x7E, false, 0xC2), Some(121)); // ABNT2 keypad .
        assert_eq!(to_evdev(0x56, false, 0xE2), Some(86)); // ABNT2 \|
        assert_eq!(to_evdev(0x45, false, VK_PAUSE), Some(119));
        assert_eq!(to_evdev(0x45, true, VK_NUMLOCK), Some(69));
        assert_eq!(to_evdev(0x6F, false, 0), None);
    }
}
