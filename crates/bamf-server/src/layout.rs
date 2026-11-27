//! Screen arrangement for the settings window: where the client screen sits
//! next to the local desktop, how a drag snaps to an edge, and how the
//! choice is written back to the config file. Pure logic, no OS calls.

use crate::geometry::{Rect, Side};

/// Smallest stretch of shared edge a drag may leave, in local pixels.
const MIN_OVERLAP: i32 = 50;
/// Drags round the offset to this many pixels.
const STEP: i32 = 10;
/// A drag this close to an aligned position (tops, centres or bottoms
/// lined up) snaps to it.
const MAGNET: i32 = 40;

/// Where the client screen is: the config's `remote_side` and
/// `remote_offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrangement {
    pub side: Side,
    /// `None`: stretched along the whole edge (no `remote_offset`).
    pub offset: Option<i32>,
}

/// The local desktop and the client screen size, in local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screens {
    pub desktop: Rect,
    pub remote_width: i32,
    pub remote_height: i32,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
}

impl Screens {
    /// Lengths of the local and remote edges along the shared side.
    fn lengths(&self, side: Side) -> (i32, i32) {
        if side.vertical() {
            (self.desktop.height(), self.remote_height)
        } else {
            (self.desktop.width(), self.remote_width)
        }
    }

    /// Offsets that keep at least [`MIN_OVERLAP`] pixels of shared edge
    /// (or as much as the shorter edge has).
    fn offset_range(&self, side: Side) -> (i32, i32) {
        let (local, remote) = self.lengths(side);
        let overlap = MIN_OVERLAP.min(local).min(remote);
        (overlap - remote, local - overlap)
    }

    pub fn clamp_offset(&self, side: Side, offset: i32) -> i32 {
        let (lo, hi) = self.offset_range(side);
        offset.clamp(lo, hi)
    }

    /// The client screen in local desktop coordinates. A stretched
    /// arrangement is drawn lined up with the start of the edge.
    pub fn remote_rect(&self, a: Arrangement) -> Rect {
        let d = self.desktop;
        let (w, h) = (self.remote_width, self.remote_height);
        let off = a.offset.unwrap_or(0);
        let (left, top) = match a.side {
            Side::Right => (d.right, d.top + off),
            Side::Left => (d.left - w, d.top + off),
            Side::Bottom => (d.left + off, d.bottom),
            Side::Top => (d.left + off, d.top - h),
        };
        Rect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        }
    }

    /// The stretch of local edge the cursor crosses, as (start, end) along
    /// the edge in desktop coordinates. Stretched: the whole edge.
    pub fn shared_edge(&self, a: Arrangement) -> (i32, i32) {
        let d = self.desktop;
        let r = self.remote_rect(a);
        let (lo, hi, rlo, rhi) = if a.side.vertical() {
            (d.top, d.bottom, r.top, r.bottom)
        } else {
            (d.left, d.right, r.left, r.right)
        };
        if a.offset.is_none() {
            return (lo, hi);
        }
        (lo.max(rlo), hi.min(rhi))
    }

    /// Where a client screen dragged to `left`/`top` (desktop coordinates)
    /// lands: on the side its centre is closest to, touching that edge,
    /// with the offset rounded and pulled onto aligned positions.
    pub fn snap(&self, left: i32, top: i32) -> Arrangement {
        let d = self.desktop;
        let (w, h) = (self.remote_width as f64, self.remote_height as f64);
        // Centre of the dragged screen relative to the desktop centre, in
        // units of the combined half sizes, so a square-ish test works for
        // any aspect ratio.
        let cx = (left as f64 + w / 2.0) - (d.left + d.right) as f64 / 2.0;
        let cy = (top as f64 + h / 2.0) - (d.top + d.bottom) as f64 / 2.0;
        let nx = cx / ((d.width() as f64 + w) / 2.0);
        let ny = cy / ((d.height() as f64 + h) / 2.0);
        let side = if nx.abs() >= ny.abs() {
            if nx >= 0.0 {
                Side::Right
            } else {
                Side::Left
            }
        } else if ny >= 0.0 {
            Side::Bottom
        } else {
            Side::Top
        };
        let raw = if side.vertical() {
            top - d.top
        } else {
            left - d.left
        };
        Arrangement {
            side,
            offset: Some(self.tidy_offset(side, raw)),
        }
    }

    /// Rounds a dragged offset to [`STEP`] and pulls it onto the aligned
    /// positions nearby.
    fn tidy_offset(&self, side: Side, raw: i32) -> i32 {
        let (local, remote) = self.lengths(side);
        let aligned = [0, (local - remote) / 2, local - remote];
        let offset = match aligned.iter().find(|&&a| (raw - a).abs() <= MAGNET) {
            Some(&a) => a,
            None => (raw as f64 / STEP as f64).round() as i32 * STEP,
        };
        self.clamp_offset(side, offset)
    }

    /// Moves the client screen along its edge by `delta` pixels (arrow
    /// keys). A stretched arrangement starts from 0.
    pub fn nudge(&self, a: Arrangement, delta: i32) -> Arrangement {
        let offset = a.offset.unwrap_or(0) + delta;
        Arrangement {
            side: a.side,
            offset: Some(self.clamp_offset(a.side, offset)),
        }
    }
}

/// Returns `text` (a config file) with `remote_side` and `remote_offset`
/// set to `a`, keeping comments and every other line as they are.
pub fn update_config(text: &str, a: Arrangement) -> String {
    let side_line = format!("remote_side = \"{}\"", a.side.name());
    let offset_line = a.offset.map(|o| format!("remote_offset = {o}"));
    let key_of = |line: &str| {
        let line = line.trim_start();
        if line.starts_with('#') {
            return None;
        }
        line.split_once('=').map(|(k, _)| k.trim().to_string())
    };

    let mut out: Vec<String> = Vec::new();
    let mut wrote_side = false;
    let mut wrote_offset = false;
    for line in text.lines() {
        match key_of(line).as_deref() {
            Some("remote_side") => {
                out.push(side_line.clone());
                wrote_side = true;
            }
            Some("remote_offset") => {
                if let Some(l) = &offset_line {
                    out.push(l.clone());
                }
                wrote_offset = true;
            }
            _ => out.push(line.to_string()),
        }
    }
    if !wrote_side {
        out.push(side_line.clone());
    }
    if let (false, Some(l)) = (wrote_offset, offset_line) {
        // Right after remote_side, so the two stay together.
        let at = out
            .iter()
            .position(|x| *x == side_line)
            .unwrap_or(out.len());
        out.insert(at + 1, l);
    }
    let mut result = out.join("\n");
    if text.ends_with('\n') || text.is_empty() {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screens() -> Screens {
        Screens {
            desktop: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            remote_width: 1920,
            remote_height: 1080,
        }
    }

    fn at(side: Side, offset: i32) -> Arrangement {
        Arrangement {
            side,
            offset: Some(offset),
        }
    }

    #[test]
    fn remote_rect_on_each_side() {
        let s = screens();
        let r = s.remote_rect(at(Side::Right, 300));
        assert_eq!((r.left, r.top, r.right, r.bottom), (1920, 300, 3840, 1380));
        let r = s.remote_rect(at(Side::Top, -100));
        assert_eq!((r.left, r.top), (-100, -1080));
        let r = s.remote_rect(at(Side::Left, 0));
        assert_eq!(r.left, -1920);
    }

    #[test]
    fn shared_edge_is_the_overlap() {
        let s = screens();
        assert_eq!(s.shared_edge(at(Side::Right, 300)), (300, 1080));
        assert_eq!(s.shared_edge(at(Side::Bottom, -500)), (0, 1420));
        let stretched = Arrangement {
            side: Side::Left,
            offset: None,
        };
        assert_eq!(s.shared_edge(stretched), (0, 1080));
    }

    #[test]
    fn snaps_to_the_closest_side() {
        let s = screens();
        assert_eq!(s.snap(2100, 333).side, Side::Right);
        assert_eq!(s.snap(-2000, 100).side, Side::Left);
        assert_eq!(s.snap(200, 1300).side, Side::Bottom);
        assert_eq!(s.snap(-150, -1200).side, Side::Top);
    }

    #[test]
    fn offsets_round_and_stick_to_alignments() {
        let s = screens();
        assert_eq!(s.snap(2100, 333), at(Side::Right, 330));
        assert_eq!(s.snap(2100, 25), at(Side::Right, 0));
        assert_eq!(s.snap(2100, -38), at(Side::Right, 0));
        // Never further than leaving MIN_OVERLAP of shared edge.
        assert_eq!(s.snap(2100, 1070), at(Side::Right, 1030));
        assert_eq!(s.snap(2100, -1070), at(Side::Right, -1030));
    }

    #[test]
    fn magnet_lines_up_centres_and_bottoms() {
        let s = Screens {
            remote_height: 800,
            ..screens()
        };
        assert_eq!(s.snap(2000, 150), at(Side::Right, 140)); // centres
        assert_eq!(s.snap(2000, 270), at(Side::Right, 280)); // bottoms
    }

    #[test]
    fn nudge_moves_along_the_edge_and_clamps() {
        let s = screens();
        let stretched = Arrangement {
            side: Side::Right,
            offset: None,
        };
        assert_eq!(s.nudge(stretched, 10), at(Side::Right, 10));
        assert_eq!(s.nudge(at(Side::Right, 1025), 10), at(Side::Right, 1030));
    }

    const CONFIG: &str = "# comment\nkey = \"abc\"\nremote_side = \"right\"\n\n# remote_offset = 300\npointer_mode = \"absolute\"\n";

    #[test]
    fn config_update_inserts_offset_after_side() {
        let out = update_config(CONFIG, at(Side::Left, -120));
        assert_eq!(
            out,
            "# comment\nkey = \"abc\"\nremote_side = \"left\"\nremote_offset = -120\n\n# remote_offset = 300\npointer_mode = \"absolute\"\n"
        );
    }

    #[test]
    fn config_update_replaces_and_removes_offset() {
        let with = update_config(CONFIG, at(Side::Right, 40));
        let again = update_config(&with, at(Side::Top, 50));
        assert!(again.contains("remote_side = \"top\"\nremote_offset = 50\n"));
        assert_eq!(again.matches("remote_offset = ").count(), 2); // + comment
        let stretched = Arrangement {
            side: Side::Top,
            offset: None,
        };
        let none = update_config(&again, stretched);
        assert!(!none.contains("remote_offset = 50"));
        assert!(none.contains("# remote_offset = 300"));
    }

    #[test]
    fn config_update_appends_missing_side() {
        assert_eq!(
            update_config("key = 1", at(Side::Bottom, 0)),
            "key = 1\nremote_side = \"bottom\"\nremote_offset = 0"
        );
    }
}
