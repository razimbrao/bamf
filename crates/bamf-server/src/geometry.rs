//! Screen geometry: where the edges are and how the virtual cursor moves on
//! the remote screen. Pure logic, no OS calls.

use bamf_proto::ABS_MAX;

/// Where the remote screen sits relative to the local one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl std::str::FromStr for Side {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "left" => Ok(Side::Left),
            "right" => Ok(Side::Right),
            "top" => Ok(Side::Top),
            "bottom" => Ok(Side::Bottom),
            other => Err(format!("invalid side `{other}` (left, right, top, bottom)")),
        }
    }
}

impl Side {
    /// True when the shared edge is vertical (screens side by side).
    fn vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// Rectangle in local screen coordinates. `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    /// The whole local desktop (all monitors).
    pub local: Rect,
    pub remote_width: u32,
    pub remote_height: u32,
    pub side: Side,
    /// How the two edges line up.
    ///
    /// `None`: the remote edge is stretched along the whole local edge, so
    /// the top of one always meets the top of the other.
    ///
    /// `Some(offset)`: pixel to pixel, like two physical monitors. The
    /// remote screen starts `offset` local pixels below the local top (or to
    /// the right of the local left, for `top`/`bottom`). Negative values
    /// mean it starts above (or to the left). The cursor only crosses where
    /// the edges overlap; elsewhere the edge is a wall.
    pub offset: Option<i32>,
}

/// Result of moving the virtual cursor on the remote screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// Still on the remote screen. `abs` is normalised to 0..=ABS_MAX,
    /// `rel` is the movement actually applied (after clamping), in pixels.
    /// `push` is the part of the movement lost to clamping, i.e. the cursor
    /// pressing against a remote edge. Compositors use that pressure for
    /// pointer barriers (e.g. revealing a dock), so it must still be sent.
    Remote {
        abs: (u16, u16),
        rel: (i32, i32),
        push: (i32, i32),
    },
    /// Crossed back; place the local cursor here.
    Leave { local: (i32, i32) },
}

/// Fraction `pos` along `0..len`, clamped to 0.0..=1.0.
fn fraction(pos: f64, len: f64) -> f64 {
    if len <= 1.0 {
        0.0
    } else {
        (pos / (len - 1.0)).clamp(0.0, 1.0)
    }
}

impl Geometry {
    /// Length of the shared edge on the local screen, in local pixels.
    fn local_len(&self) -> i32 {
        if self.side.vertical() {
            self.local.height()
        } else {
            self.local.width()
        }
    }

    /// Length of the shared edge on the remote screen, in remote pixels.
    fn remote_len(&self) -> f64 {
        if self.side.vertical() {
            self.remote_height as f64
        } else {
            self.remote_width as f64
        }
    }

    /// Position along the shared edge, relative to the start of the local
    /// screen.
    fn local_along(&self, x: i32, y: i32) -> i32 {
        if self.side.vertical() {
            y - self.local.top
        } else {
            x - self.local.left
        }
    }

    /// Local position along the edge → remote position, if the edges
    /// overlap there.
    fn remote_along_of(&self, along: i32) -> Option<f64> {
        let remote_len = self.remote_len();
        match self.offset {
            None => Some(fraction(along as f64, self.local_len() as f64) * (remote_len - 1.0)),
            Some(offset) => {
                let r = (along - offset) as f64;
                (0.0..remote_len).contains(&r).then_some(r)
            }
        }
    }

    /// Remote position along the edge → local position, if the edges
    /// overlap there.
    fn local_along_of(&self, along: f64) -> Option<i32> {
        let local_len = self.local_len();
        match self.offset {
            None => {
                let f = fraction(along, self.remote_len());
                Some((f * (local_len - 1) as f64).round() as i32)
            }
            Some(offset) => {
                let l = along.round() as i32 + offset;
                (0..local_len).contains(&l).then_some(l)
            }
        }
    }

    /// True when the local cursor touches the edge that faces the remote
    /// screen, at a point where the two screens meet.
    pub fn at_edge(&self, x: i32, y: i32) -> bool {
        let l = &self.local;
        let on_edge = match self.side {
            Side::Right => x >= l.right - 1,
            Side::Left => x <= l.left,
            Side::Bottom => y >= l.bottom - 1,
            Side::Top => y <= l.top,
        };
        on_edge && self.remote_along_of(self.local_along(x, y)).is_some()
    }

    /// Virtual cursor position (remote pixels) right after crossing at local
    /// position `(x, y)`.
    pub fn entry_point(&self, x: i32, y: i32) -> (f64, f64) {
        let (rw, rh) = (self.remote_width as f64, self.remote_height as f64);
        let a = self.remote_along_of(self.local_along(x, y)).unwrap_or(0.0);
        match self.side {
            Side::Right => (0.0, a),
            Side::Left => (rw - 1.0, a),
            Side::Bottom => (a, 0.0),
            Side::Top => (a, rh - 1.0),
        }
    }

    pub fn normalise(&self, pos: (f64, f64)) -> (u16, u16) {
        let max = ABS_MAX as f64;
        let x = fraction(pos.0, self.remote_width as f64) * max;
        let y = fraction(pos.1, self.remote_height as f64) * max;
        (x.round() as u16, y.round() as u16)
    }

    /// Moves the virtual cursor `pos` by `(dx, dy)`.
    pub fn apply(&self, pos: &mut (f64, f64), dx: i32, dy: i32) -> Motion {
        let (rw, rh) = (self.remote_width as f64, self.remote_height as f64);
        let nx = pos.0 + dx as f64;
        let ny = pos.1 + dy as f64;

        let leaving = match self.side {
            Side::Right => nx < 0.0,
            Side::Left => nx > rw - 1.0,
            Side::Bottom => ny < 0.0,
            Side::Top => ny > rh - 1.0,
        };
        if leaving {
            let along = if self.side.vertical() {
                ny.clamp(0.0, rh - 1.0)
            } else {
                nx.clamp(0.0, rw - 1.0)
            };
            // Where the local screen does not continue, the edge is a wall.
            if let Some(a) = self.local_along_of(along) {
                return Motion::Leave {
                    local: self.exit_point(a),
                };
            }
        }

        let old = *pos;
        pos.0 = nx.clamp(0.0, rw - 1.0);
        pos.1 = ny.clamp(0.0, rh - 1.0);
        Motion::Remote {
            abs: self.normalise(*pos),
            rel: ((pos.0 - old.0) as i32, (pos.1 - old.1) as i32),
            push: ((nx - pos.0) as i32, (ny - pos.1) as i32),
        }
    }

    /// Local cursor position when coming back at local position `along` the
    /// edge. It lands one pixel inside the edge so it does not bounce right
    /// back.
    fn exit_point(&self, along: i32) -> (i32, i32) {
        let l = &self.local;
        match self.side {
            Side::Right => (l.right - 2, l.top + along),
            Side::Left => (l.left + 1, l.top + along),
            Side::Bottom => (l.left + along, l.bottom - 2),
            Side::Top => (l.left + along, l.top + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(side: Side) -> Geometry {
        Geometry {
            local: Rect {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1440,
            },
            remote_width: 1920,
            remote_height: 1080,
            side,
            offset: None,
        }
    }

    fn geo_offset(offset: i32) -> Geometry {
        Geometry {
            offset: Some(offset),
            ..geo(Side::Right)
        }
    }

    #[test]
    fn edges() {
        let g = geo(Side::Right);
        assert!(g.at_edge(2559, 10));
        assert!(!g.at_edge(2558, 10));
        assert!(geo(Side::Left).at_edge(0, 5));
        assert!(geo(Side::Top).at_edge(5, 0));
        assert!(geo(Side::Bottom).at_edge(5, 1439));
    }

    #[test]
    fn entry_keeps_relative_position() {
        let g = geo(Side::Right);
        let p = g.entry_point(2559, 1439);
        assert_eq!(p, (0.0, 1079.0));
        let p = g.entry_point(2559, 0);
        assert_eq!(p, (0.0, 0.0));
        assert_eq!(geo(Side::Left).entry_point(0, 0), (1919.0, 0.0));
    }

    #[test]
    fn moves_clamps_and_normalises() {
        let g = geo(Side::Right);
        let mut pos = (0.0, 0.0);
        assert_eq!(
            g.apply(&mut pos, 10, -5),
            Motion::Remote {
                abs: g.normalise((10.0, 0.0)),
                rel: (10, 0),
                push: (0, -5)
            }
        );
        g.apply(&mut pos, 100_000, 100_000);
        assert_eq!(pos, (1919.0, 1079.0));
        assert_eq!(g.normalise(pos), (ABS_MAX, ABS_MAX));
        // Pressing against the bottom edge keeps reporting the push.
        assert_eq!(
            g.apply(&mut pos, 0, 7),
            Motion::Remote {
                abs: (ABS_MAX, ABS_MAX),
                rel: (0, 0),
                push: (0, 7)
            }
        );
    }

    #[test]
    fn leaves_through_the_entry_edge() {
        let g = geo(Side::Right);
        let mut pos = (3.0, 1079.0);
        assert_eq!(
            g.apply(&mut pos, -2, 0),
            Motion::Remote {
                abs: g.normalise((1.0, 1079.0)),
                rel: (-2, 0),
                push: (0, 0)
            }
        );
        assert_eq!(
            g.apply(&mut pos, -5, 0),
            Motion::Leave {
                local: (2558, 1439)
            }
        );

        let g = geo(Side::Top);
        let mut pos = (0.0, 1079.0);
        assert_eq!(g.apply(&mut pos, 0, 1), Motion::Leave { local: (0, 1) });
    }

    #[test]
    fn offset_crosses_only_where_edges_overlap() {
        // Remote 1080 px tall, starting 300 px below the local top:
        // local rows 300..=1379 touch it.
        let g = geo_offset(300);
        assert!(!g.at_edge(2559, 299));
        assert!(g.at_edge(2559, 300));
        assert!(g.at_edge(2559, 1379));
        assert!(!g.at_edge(2559, 1380));
        assert_eq!(g.entry_point(2559, 300), (0.0, 0.0));
        assert_eq!(g.entry_point(2559, 800), (0.0, 500.0));
    }

    #[test]
    fn offset_maps_back_pixel_to_pixel() {
        let g = geo_offset(300);
        let mut pos = (2.0, 500.0);
        assert_eq!(
            g.apply(&mut pos, -5, 0),
            Motion::Leave { local: (2558, 800) }
        );
    }

    #[test]
    fn offset_edge_is_a_wall_where_local_screen_ends() {
        // Remote starts 600 px down, so its rows 840.. have no local
        // neighbour (600 + 840 = 1440 is past the local bottom).
        let g = geo_offset(600);
        let mut pos = (0.0, 900.0);
        assert_eq!(
            g.apply(&mut pos, -4, 0),
            Motion::Remote {
                abs: g.normalise((0.0, 900.0)),
                rel: (0, 0),
                push: (-4, 0)
            }
        );
        let mut pos = (0.0, 839.0);
        assert_eq!(
            g.apply(&mut pos, -4, 0),
            Motion::Leave {
                local: (2558, 1439)
            }
        );
    }

    #[test]
    fn negative_offset_starts_above() {
        let g = geo_offset(-200);
        assert!(g.at_edge(2559, 0));
        assert_eq!(g.entry_point(2559, 0), (0.0, 200.0));
        assert!(!g.at_edge(2559, 880));
    }

    #[test]
    fn parses_side() {
        assert_eq!("Right".parse::<Side>().unwrap(), Side::Right);
        assert!("up".parse::<Side>().is_err());
    }
}
