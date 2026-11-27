//! The settings window's content, drawn into a [`Canvas`] and driven by
//! plain input events: the Screens tab (drag the client screen next to the
//! local monitors) and the Clipboard tab (history, like Win+V).
//!
//! Pure logic, no OS calls. Text that must stay readable at any length
//! (clipboard previews, values) comes back as [`TextRun`]s for the window to
//! draw with a system font; labels use the pixel font.

use std::time::Instant;

use crate::canvas::{text_height, text_width, Area, Canvas, Color};
use crate::geometry::{Rect, Side};
use crate::history::{age, Content, Entry, History, Origin};
use crate::icon::{self, Look};
use crate::image::Image;
use crate::layout::{Arrangement, Screens};

/// Logical window size.
pub const WIDTH: i32 = 600;
pub const HEIGHT: i32 = 416;

pub const BG: Color = 0x14172b;
const PANEL: Color = 0x0f1224;
const DOTS: Color = 0x1c2040;
const LINE: Color = 0x20243d;
const HOVER: Color = 0x1a1e36;
const BEZEL: Color = 0x0b0e1e;
const HOOD: Color = 0x4b3c9e;
const HOOD_LIGHT: Color = 0x8a74e0;
const HOOD_DARK: Color = 0x2f2468;
const REMOTE: Color = 0x2a2f4d;
const GREY: Color = 0x5c6380;
const YELLOW: Color = 0xffe23a;
const WHITE: Color = 0xffffff;
const TEXT: Color = 0xe6e3f5;
const MUTED: Color = 0x6d6a8f;
const SOFT: Color = 0xa99ce8;
const PALE: Color = 0xcfc6ff;
const GREEN: Color = 0x5dd39e;
const RED: Color = 0xff6b6b;
const OUTLINE: Color = 0x10142a;

const PAD: i32 = 18;
const STAGE: Area = Area::new(PAD, 56, WIDTH - 2 * PAD, 250);
const STATS_Y: i32 = 318;
const STATS_H: i32 = 40;
const FOOTER_Y: i32 = 374;
const BUTTON_H: i32 = 24;
const SEARCH: Area = Area::new(PAD, 56, WIDTH - 2 * PAD, 30);
const LIST: Area = Area::new(PAD, 94, WIDTH - 2 * PAD, ROWS as i32 * ROW_H);
const ROWS: usize = 7;
const ROW_H: i32 = 38;
const THUMB: (i32, i32) = (48, 28);
/// Thumbnails are made at this many pixels per logical pixel, so they stay
/// sharp up to 200% scaling.
pub const THUMB_DENSITY: u32 = 2;
pub const THUMB_SIZE: (u32, u32) = (
    THUMB.0 as u32 * THUMB_DENSITY,
    THUMB.1 as u32 * THUMB_DENSITY,
);
pub const LIST_BG: [u8; 3] = [0x2f, 0x24, 0x68];

/// Gap between monitors drawn next to each other.
const GAP: i32 = 3;
/// Minimum stage margin around the drawing.
const MARGIN: i32 = 8;

const CURSOR: [&str; 12] = [
    "K.......", "KK......", "KWK.....", "KWWK....", "KWWWK...", "KWWWWK..", "KWWWWWK.", "KWWWWWWK",
    "KWWWKKKK", "KWKWK...", "KK.KWK..", "....KK..",
];
const PIN: [&str; 8] = [
    "..YYYY..", "..YYYY..", "..YYYY..", ".YYYYYY.", "YYYYYYYY", "...YY...", "...YY...", "...YY...",
];
const CROSS: [&str; 8] = [
    "RR....RR", ".RR..RR.", "..RRRR..", "...RR...", "..RRRR..", ".RR..RR.", "RR....RR", "........",
];
const GLASS: [&str; 8] = [
    ".MMM....", "M...M...", "M...M...", "M...M...", ".MMM....", "....M...", ".....M..", "......M.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Screens,
    Clipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Identify,
    Reset,
    Apply,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    Remote,
    Button(Button),
    Row(u64),
    Pin(u64),
    Delete(u64),
}

/// What the window should do after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Apply(Arrangement),
    Identify,
    Copy(u64),
    TogglePin(u64),
    Delete(u64),
    Clear,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// Text for the window to draw with the system font, in logical pixels,
/// vertically centred in `area` and cut with an ellipsis when too long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub area: Area,
    pub text: String,
    pub color: Color,
    pub size: i32,
    pub align: Align,
}

/// One local monitor, in desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub rect: Rect,
    pub primary: bool,
}

/// Keys the panel reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Delete,
    Backspace,
    Escape,
    Tab,
}

pub struct Panel {
    pub tab: Tab,
    pub screens: Screens,
    pub monitors: Vec<Monitor>,
    /// What the config file says.
    pub saved: Arrangement,
    /// What is shown, possibly not applied yet.
    pub current: Arrangement,
    pub client_online: bool,
    pub pointer_mode: String,
    /// Footer message: text and whether it is an error.
    pub status: Option<(String, bool)>,
    pub query: String,
    drag: Option<(i32, i32)>,
    /// Arrangement the stage is centred on during a drag.
    frame: Option<Arrangement>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    scroll: usize,
    selected: Option<u64>,
}

/// Maps desktop coordinates onto the stage.
struct StageMap {
    f: f64,
    ox: f64,
    oy: f64,
    left: i32,
    top: i32,
}

impl StageMap {
    fn point(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (self.ox + (x - self.left) as f64 * self.f).round() as i32,
            (self.oy + (y - self.top) as f64 * self.f).round() as i32,
        )
    }

    fn rect(&self, r: Rect) -> Area {
        let (x0, y0) = self.point(r.left, r.top);
        let (x1, y1) = self.point(r.right, r.bottom);
        Area::new(x0, y0, x1 - x0, y1 - y0)
    }

    fn back(&self, x: i32, y: i32) -> (i32, i32) {
        (
            ((x as f64 - self.ox) / self.f).round() as i32 + self.left,
            ((y as f64 - self.oy) / self.f).round() as i32 + self.top,
        )
    }
}

impl Panel {
    pub fn new(
        screens: Screens,
        monitors: Vec<Monitor>,
        saved: Arrangement,
        pointer_mode: &str,
    ) -> Self {
        Self {
            tab: Tab::Screens,
            screens,
            monitors,
            saved,
            current: saved,
            client_online: false,
            pointer_mode: pointer_mode.to_string(),
            status: None,
            query: String::new(),
            drag: None,
            frame: None,
            hover: None,
            pressed: None,
            scroll: 0,
            selected: None,
        }
    }

    fn map(&self) -> StageMap {
        let d = self.screens.desktop;
        let (rw, rh) = (self.screens.remote_width, self.screens.remote_height);
        let room_w = (STAGE.w - 2 * MARGIN) as f64;
        let room_h = (STAGE.h - 2 * MARGIN) as f64;
        // Room for the client screen on any side of the desktop.
        let f = (room_w / (d.width() + 2 * rw) as f64).min(room_h / (d.height() + 2 * rh) as f64);
        // Both screens' extent, relative to the desktop's top-left corner.
        let r = self.screens.remote_rect(self.frame.unwrap_or(self.current));
        let left = r.left.min(d.left) - d.left;
        let top = r.top.min(d.top) - d.top;
        let right = r.right.max(d.right) - d.left;
        let bottom = r.bottom.max(d.bottom) - d.top;
        StageMap {
            f,
            // Centre both screens together; while dragging, hold the view
            // still so the screen does not slide away from the pointer.
            ox: STAGE.x as f64 + (STAGE.w as f64 - (left + right) as f64 * f) / 2.0,
            oy: STAGE.y as f64 + (STAGE.h as f64 - (top + bottom) as f64 * f) / 2.0,
            left: d.left,
            top: d.top,
        }
    }

    fn remote_area(&self) -> Area {
        self.map()
            .rect(self.screens.remote_rect(self.current))
            .inset(GAP)
    }

    fn tab_areas(&self) -> [(Tab, Area, &'static str); 2] {
        let cw = text_width("clipboard", 1);
        let sw = text_width("screens", 1);
        let cx = WIDTH - PAD - cw;
        let sx = cx - 22 - sw;
        [
            (Tab::Screens, Area::new(sx, 20, sw, 18), "screens"),
            (Tab::Clipboard, Area::new(cx, 20, cw, 18), "clipboard"),
        ]
    }

    fn buttons(&self) -> Vec<(Button, Area, &'static str)> {
        let list: &[(Button, &str)] = match self.tab {
            Tab::Screens => &[
                (Button::Identify, "identify"),
                (Button::Reset, "reset"),
                (Button::Apply, "apply"),
            ],
            Tab::Clipboard => &[(Button::Clear, "clear")],
        };
        let mut x = WIDTH - PAD;
        let mut out: Vec<_> = list
            .iter()
            .rev()
            .map(|&(b, label)| {
                let w = text_width(label, 1) + 24;
                x -= w;
                let area = Area::new(x, FOOTER_Y, w, BUTTON_H);
                x -= 8;
                (b, area, label)
            })
            .collect();
        out.reverse();
        out
    }

    fn rows<'a>(&self, history: &'a History) -> Vec<(Area, &'a Entry)> {
        history
            .filtered(&self.query)
            .skip(self.scroll)
            .take(ROWS)
            .enumerate()
            .map(|(i, e)| {
                (
                    Area::new(LIST.x, LIST.y + i as i32 * ROW_H, LIST.w, ROW_H),
                    e,
                )
            })
            .collect()
    }

    fn pin_area(row: Area) -> Area {
        Area::new(row.right() - 52, row.y + 11, 16, 16)
    }

    fn delete_area(row: Area) -> Area {
        Area::new(row.right() - 26, row.y + 11, 16, 16)
    }

    pub fn hit(&self, x: i32, y: i32, history: &History) -> Option<Hit> {
        for (tab, area, _) in self.tab_areas() {
            if Area::new(area.x - 6, 10, area.w + 12, 36).contains(x, y) {
                return Some(Hit::Tab(tab));
            }
        }
        for (b, area, _) in self.buttons() {
            if area.contains(x, y) {
                return Some(Hit::Button(b));
            }
        }
        match self.tab {
            Tab::Screens => {
                if STAGE.contains(x, y) && self.remote_area().contains(x, y) {
                    return Some(Hit::Remote);
                }
            }
            Tab::Clipboard => {
                for (row, e) in self.rows(history) {
                    if row.contains(x, y) {
                        if Self::pin_area(row).inset(-3).contains(x, y) {
                            return Some(Hit::Pin(e.id));
                        }
                        if Self::delete_area(row).inset(-3).contains(x, y) {
                            return Some(Hit::Delete(e.id));
                        }
                        return Some(Hit::Row(e.id));
                    }
                }
            }
        }
        None
    }

    // ---- events ----

    /// True when the window should redraw.
    pub fn mouse_move(&mut self, x: i32, y: i32, history: &History) -> bool {
        if let Some((gx, gy)) = self.drag {
            let map = self.map();
            // The drawn screen is inset by GAP; undo that for its corner.
            let (left, top) = map.back(x - gx - GAP, y - gy - GAP);
            let next = self.screens.snap(left, top);
            if next != self.current {
                self.current = next;
                self.status = None;
                return true;
            }
            return false;
        }
        let hover = self.hit(x, y, history);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn mouse_leave(&mut self) -> bool {
        let changed = self.hover.is_some();
        self.hover = None;
        changed
    }

    /// True when the mouse is over something clickable.
    pub fn over_control(&self) -> bool {
        self.hover.is_some()
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn mouse_down(&mut self, x: i32, y: i32, history: &History) -> Action {
        let hit = self.hit(x, y, history);
        self.pressed = hit;
        if hit == Some(Hit::Remote) {
            let r = self.remote_area();
            self.drag = Some((x - r.x, y - r.y));
            self.frame = Some(self.current);
        }
        Action::Redraw
    }

    pub fn mouse_up(&mut self, x: i32, y: i32, history: &History) -> Action {
        let pressed = self.pressed.take();
        if self.drag.take().is_some() {
            self.frame = None;
            return Action::Redraw;
        }
        let hit = self.hit(x, y, history);
        if hit != pressed {
            return Action::Redraw;
        }
        match hit {
            Some(Hit::Tab(tab)) => {
                self.tab = tab;
                self.status = None;
                Action::Redraw
            }
            Some(Hit::Button(b)) => self.press(b),
            Some(Hit::Row(id)) => {
                self.selected = Some(id);
                Action::Copy(id)
            }
            Some(Hit::Pin(id)) => Action::TogglePin(id),
            Some(Hit::Delete(id)) => Action::Delete(id),
            _ => Action::Redraw,
        }
    }

    fn press(&mut self, b: Button) -> Action {
        match b {
            Button::Identify => Action::Identify,
            Button::Reset => {
                self.current = self.saved;
                self.status = None;
                Action::Redraw
            }
            Button::Apply => Action::Apply(self.current),
            Button::Clear => {
                self.scroll = 0;
                Action::Clear
            }
        }
    }

    pub fn wheel(&mut self, notches: i32, history: &History) -> bool {
        if self.tab != Tab::Clipboard {
            return false;
        }
        let total = history.filtered(&self.query).count();
        let max = total.saturating_sub(ROWS);
        let next = (self.scroll as i64 - notches as i64).clamp(0, max as i64) as usize;
        let changed = next != self.scroll;
        self.scroll = next;
        changed
    }

    pub fn key(&mut self, key: Key, shift: bool, history: &History) -> Action {
        if key == Key::Tab {
            self.tab = match self.tab {
                Tab::Screens => Tab::Clipboard,
                Tab::Clipboard => Tab::Screens,
            };
            self.status = None;
            return Action::Redraw;
        }
        match self.tab {
            Tab::Screens => {
                let step = if shift { 1 } else { 10 };
                let along = self.current.side.vertical();
                let delta = match key {
                    Key::Up if along => -step,
                    Key::Down if along => step,
                    Key::Left if !along => -step,
                    Key::Right if !along => step,
                    Key::Enter => return Action::Apply(self.current),
                    Key::Escape => return Action::Close,
                    _ => return Action::None,
                };
                self.current = self.screens.nudge(self.current, delta);
                self.status = None;
                Action::Redraw
            }
            Tab::Clipboard => self.list_key(key, history),
        }
    }

    fn list_key(&mut self, key: Key, history: &History) -> Action {
        let ids: Vec<u64> = history.filtered(&self.query).map(|e| e.id).collect();
        let at = self
            .selected
            .and_then(|id| ids.iter().position(|&i| i == id));
        match key {
            Key::Up | Key::Down => {
                if ids.is_empty() {
                    return Action::None;
                }
                let next = match (at, key) {
                    (None, _) => 0,
                    (Some(i), Key::Up) => i.saturating_sub(1),
                    (Some(i), _) => (i + 1).min(ids.len() - 1),
                };
                self.selected = Some(ids[next]);
                if next < self.scroll {
                    self.scroll = next;
                } else if next >= self.scroll + ROWS {
                    self.scroll = next + 1 - ROWS;
                }
                Action::Redraw
            }
            Key::Enter => match at {
                Some(i) => Action::Copy(ids[i]),
                None => Action::None,
            },
            Key::Delete => match at {
                Some(i) => {
                    // Keep a selection so Delete can be pressed repeatedly.
                    self.selected = ids
                        .get(i + 1)
                        .or(i.checked_sub(1).and_then(|j| ids.get(j)))
                        .copied();
                    Action::Delete(ids[i])
                }
                None => Action::None,
            },
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.scroll = 0;
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Key::Escape => {
                if self.query.is_empty() {
                    Action::Close
                } else {
                    self.query.clear();
                    self.scroll = 0;
                    Action::Redraw
                }
            }
            _ => Action::None,
        }
    }

    /// A typed character: goes to the clipboard search.
    pub fn char(&mut self, c: char) -> bool {
        if self.tab != Tab::Clipboard || c.is_control() || self.query.chars().count() >= 60 {
            return false;
        }
        self.query.push(c);
        self.scroll = 0;
        self.selected = None;
        true
    }

    /// Keeps scroll and selection valid after the history changed.
    pub fn history_changed(&mut self, history: &History) {
        let total = history.filtered(&self.query).count();
        self.scroll = self.scroll.min(total.saturating_sub(ROWS));
        if self.selected.is_some_and(|id| history.get(id).is_none()) {
            self.selected = None;
        }
    }

    // ---- drawing ----

    pub fn render(&self, c: &mut Canvas, history: &History, now: Instant) -> Vec<TextRun> {
        let mut text = Vec::new();
        c.unclip();
        c.fill(Area::new(0, 0, WIDTH, HEIGHT), BG);
        self.header(c);
        match self.tab {
            Tab::Screens => self.screens_tab(c, &mut text),
            Tab::Clipboard => self.clipboard_tab(c, &mut text, history, now),
        }
        for (b, area, label) in self.buttons() {
            let hover = self.hover == Some(Hit::Button(b));
            let (fill, border, ink) = match (b, hover) {
                (Button::Apply, false) => (YELLOW, YELLOW, OUTLINE),
                (Button::Apply, true) => (0xfff2a8, 0xfff2a8, OUTLINE),
                (_, false) => (BG, HOOD_DARK, SOFT),
                (_, true) => (HOVER, HOOD_LIGHT, TEXT),
            };
            c.fill(area, fill);
            c.frame(area, 2, border);
            let ty = area.y + (area.h - text_height(1)) / 2;
            c.text(area.x + 12, ty, label, ink, 1);
        }
        text
    }

    fn header(&self, c: &mut Canvas) {
        let look = if self.client_online {
            Look::Awake
        } else {
            Look::Dozing
        };
        let imp = icon::render(look, 16);
        c.image(PAD, 12, &flatten(&imp, BG), 2 * c.scale);
        c.text(PAD + 42, 22, "bamf", WHITE, 2);
        for (tab, area, label) in self.tab_areas() {
            let active = tab == self.tab;
            let hover = self.hover == Some(Hit::Tab(tab));
            let ink = if active {
                YELLOW
            } else if hover {
                TEXT
            } else {
                MUTED
            };
            c.text(area.x, area.y + 4, label, ink, 1);
            if active {
                c.fill(Area::new(area.x, area.y + 15, area.w, 2), YELLOW);
            }
        }
    }

    fn screens_tab(&self, c: &mut Canvas, text: &mut Vec<TextRun>) {
        c.fill(STAGE, PANEL);
        let mut y = STAGE.y + 4;
        while y < STAGE.bottom() {
            let mut x = STAGE.x + 4;
            while x < STAGE.right() {
                c.fill(Area::new(x, y, 1, 1), DOTS);
                x += 10;
            }
            y += 10;
        }
        c.clip(STAGE);
        let map = self.map();

        let mut order: Vec<&Monitor> = self.monitors.iter().collect();
        order.sort_by_key(|m| (m.rect.left, m.rect.top));
        let named = order.len() > 1;
        for (i, m) in order.iter().enumerate() {
            let area = map.rect(m.rect).inset(GAP);
            let label = format!("{} windows", i + 1);
            let mut lines = vec![format!("{}×{}", m.rect.width(), m.rect.height())];
            if m.primary && named {
                lines.push("primary".into());
            } else if !named {
                lines.push("this pc".into());
            }
            draw_local(c, text, area, &label, &lines, m.primary);
        }

        let remote = self.remote_area();
        let lifted = self.drag.is_some() || self.hover == Some(Hit::Remote);
        draw_remote(
            c,
            text,
            remote,
            &format!("{} fedora", order.len() + 1),
            &format!(
                "{}×{}",
                self.screens.remote_width, self.screens.remote_height
            ),
            self.client_online,
            lifted,
        );

        // The stretch of edge the cursor crosses.
        let (a, b) = self.screens.shared_edge(self.current);
        let d = self.screens.desktop;
        let len = b - a;
        let (bar, label_at, cursor_at) = match self.current.side {
            Side::Right | Side::Left => {
                let x = if self.current.side == Side::Right {
                    d.right
                } else {
                    d.left
                };
                let (sx, sa) = map.point(x, a);
                let (_, sb) = map.point(x, b);
                (
                    Area::new(sx - 2, sa, 4, sb - sa),
                    (sx - text_width(&format!("{len} px"), 1) / 2, sb + 5),
                    (sx - 1, (sa + sb) / 2 - 6),
                )
            }
            Side::Top | Side::Bottom => {
                let y = if self.current.side == Side::Bottom {
                    d.bottom
                } else {
                    d.top
                };
                let (sa, sy) = map.point(a, y);
                let (sb, _) = map.point(b, y);
                (
                    Area::new(sa, sy - 2, sb - sa, 4),
                    (sb + 6, sy - 3),
                    ((sa + sb) / 2 - 3, sy - 4),
                )
            }
        };
        c.fill(bar, YELLOW);
        c.text(label_at.0, label_at.1, &format!("{len} px"), YELLOW, 1);
        c.sprite(cursor_at.0, cursor_at.1, &CURSOR, 1, |b| match b {
            b'K' => Some(OUTLINE),
            b'W' => Some(WHITE),
            _ => None,
        });
        c.unclip();

        // Read-out of the arrangement.
        let offset = match self.current.offset {
            Some(o) => format!("{o} px"),
            None => "stretched".into(),
        };
        let stats = [
            ("side", self.current.side.name().to_string()),
            ("offset", offset),
            ("shared edge", format!("{len} px")),
            ("pointer", self.pointer_mode.clone()),
        ];
        let w = (STAGE.w - 3 * 6) / 4;
        for (i, (label, value)) in stats.iter().enumerate() {
            let area = Area::new(STAGE.x + i as i32 * (w + 6), STATS_Y, w, STATS_H);
            c.fill(area, PANEL);
            text.push(run(
                Area::new(area.x + 10, area.y + 3, w - 20, 16),
                label,
                MUTED,
                12,
            ));
            text.push(run(
                Area::new(area.x + 10, area.y + 18, w - 20, 20),
                value,
                TEXT,
                14,
            ));
        }

        let (msg, color) = match &self.status {
            Some((m, true)) => (m.clone(), RED),
            Some((m, false)) => (m.clone(), GREEN),
            None if self.current != self.saved => ("not applied yet".to_string(), YELLOW),
            None => (
                "drag fedora · arrows nudge 10 px · shift 1 px".to_string(),
                MUTED,
            ),
        };
        let left_of_buttons = self.buttons()[0].1.x - 10;
        text.push(run(
            Area::new(PAD, FOOTER_Y, left_of_buttons - PAD, BUTTON_H),
            &msg,
            color,
            12,
        ));
    }

    fn clipboard_tab(
        &self,
        c: &mut Canvas,
        text: &mut Vec<TextRun>,
        history: &History,
        now: Instant,
    ) {
        c.fill(SEARCH, PANEL);
        c.sprite(SEARCH.x + 10, SEARCH.y + 11, &GLASS, 1, |b| {
            (b == b'M').then_some(MUTED)
        });
        let (q, color) = if self.query.is_empty() {
            ("type to search".to_string(), MUTED)
        } else {
            (format!("{}_", self.query), TEXT)
        };
        text.push(run(
            Area::new(SEARCH.x + 26, SEARCH.y, SEARCH.w - 36, SEARCH.h),
            &q,
            color,
            13,
        ));

        let rows = self.rows(history);
        if rows.is_empty() {
            let imp = flatten(&icon::render(Look::Asleep, 16), BG);
            let size = 16 * 3;
            c.image(WIDTH / 2 - size / 2, LIST.y + 70, &imp, 3 * c.scale);
            let msg = if self.query.is_empty() {
                "copy something on either computer"
            } else {
                "nothing matches"
            };
            text.push(TextRun {
                area: Area::new(LIST.x, LIST.y + 130, LIST.w, 20),
                text: msg.into(),
                color: MUTED,
                size: 13,
                align: Align::Centre,
            });
        }
        let last = rows.len().saturating_sub(1);
        for (i, (row, e)) in rows.iter().enumerate() {
            let hover = matches!(self.hover, Some(Hit::Row(id) | Hit::Pin(id) | Hit::Delete(id)) if id == e.id);
            if hover || self.selected == Some(e.id) {
                c.fill(*row, HOVER);
            }
            if i != last {
                c.fill(Area::new(row.x, row.bottom() - 1, row.w, 1), LINE);
            }
            let origin = if e.origin == Origin::Local {
                HOOD_LIGHT
            } else {
                GREY
            };
            c.fill(Area::new(row.x + 8, row.y + 15, 8, 8), origin);
            let mut x = row.x + 26;
            let preview_color = match &e.content {
                Content::Image { thumb, .. } => {
                    let box_ = Area::new(x, row.y + 5, THUMB.0, THUMB.1);
                    c.fill(box_, HOOD_DARK);
                    // One pixel of the resampled thumbnail per screen pixel.
                    let shown = shrink(thumb, c.scale as u32, THUMB_DENSITY);
                    let iw = shown.width as i32 / c.scale;
                    let ih = shown.height as i32 / c.scale;
                    c.image(
                        box_.x + (box_.w - iw) / 2,
                        box_.y + (box_.h - ih) / 2,
                        &shown,
                        1,
                    );
                    x += THUMB.0 + 10;
                    SOFT
                }
                Content::Text(_) => TEXT,
            };
            text.push(run(
                Area::new(x, row.y, row.right() - 70 - x, ROW_H),
                &e.content.preview(200),
                preview_color,
                13,
            ));
            if hover {
                let pin = Self::pin_area(*row);
                let del = Self::delete_area(*row);
                let pin_ink = if e.pinned { YELLOW } else { MUTED };
                c.sprite(pin.x + 4, pin.y + 4, &PIN, 1, |b| {
                    (b == b'Y').then_some(pin_ink)
                });
                let del_ink = if self.hover == Some(Hit::Delete(e.id)) {
                    RED
                } else {
                    MUTED
                };
                c.sprite(del.x + 4, del.y + 5, &CROSS, 1, |b| {
                    (b == b'R').then_some(del_ink)
                });
            } else {
                if e.pinned {
                    c.fill(Area::new(row.right() - 62, row.y + 16, 6, 6), YELLOW);
                }
                text.push(TextRun {
                    area: Area::new(row.right() - 50, row.y, 42, ROW_H),
                    text: age(now.saturating_duration_since(e.at)),
                    color: MUTED,
                    size: 12,
                    align: Align::Right,
                });
            }
        }

        // Scroll bar.
        let total = history.filtered(&self.query).count();
        if total > ROWS {
            let track = Area::new(LIST.right() + 6, LIST.y, 3, LIST.h);
            c.fill(track, LINE);
            let h = (LIST.h as usize * ROWS / total).max(12) as i32;
            let y = LIST.y + ((LIST.h - h) as usize * self.scroll / (total - ROWS)) as i32;
            c.fill(Area::new(track.x, y, 3, h), HOOD_DARK);
        }

        // Legend, or the latest message.
        let y = FOOTER_Y + (BUTTON_H - 8) / 2;
        if let Some((m, err)) = &self.status {
            let color = if *err { RED } else { GREEN };
            text.push(run(Area::new(PAD, FOOTER_Y, 360, BUTTON_H), m, color, 12));
        } else {
            let mut x = PAD;
            for (color, label, w) in [
                (HOOD_LIGHT, "windows", 70),
                (GREY, "fedora", 64),
                (YELLOW, "pinned", 64),
            ] {
                c.fill(Area::new(x, y, 8, 8), color);
                text.push(run(
                    Area::new(x + 14, FOOTER_Y, w - 14, BUTTON_H),
                    label,
                    MUTED,
                    12,
                ));
                x += w + 10;
            }
            let count = format!("{} items · memory only", history.len());
            text.push(run(
                Area::new(x + 6, FOOTER_Y, 200, BUTTON_H),
                &count,
                MUTED,
                12,
            ));
        }
    }
}

fn run(area: Area, text: &str, color: Color, size: i32) -> TextRun {
    TextRun {
        area,
        text: text.to_string(),
        color,
        size,
        align: Align::Left,
    }
}

/// Replaces transparent pixels with `bg` (the canvas has no alpha).
fn flatten(img: &[u8], bg: Color) -> Image {
    let size = ((img.len() / 4) as f64).sqrt() as u32;
    let mut rgba = img.to_vec();
    for px in rgba.chunks_mut(4) {
        if px[3] == 0 {
            px[0] = (bg >> 16) as u8;
            px[1] = (bg >> 8) as u8;
            px[2] = bg as u8;
        }
    }
    Image {
        width: size,
        height: size,
        rgba,
    }
}

/// Resamples a thumbnail made at `density` pixels per logical pixel for a
/// canvas at `scale`.
fn shrink(img: &Image, scale: u32, density: u32) -> Image {
    if scale == density {
        return img.clone();
    }
    let w = (img.width * scale / density).max(1);
    let h = (img.height * scale / density).max(1);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let sy = y * img.height / h;
        for x in 0..w {
            let sx = x * img.width / w;
            let i = ((sy * img.width + sx) * 4) as usize;
            rgba.extend_from_slice(&img.rgba[i..i + 4]);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// A local monitor: bezel, screen, taskbar, and the imp on the primary.
fn draw_local(
    c: &mut Canvas,
    text: &mut Vec<TextRun>,
    area: Area,
    label: &str,
    lines: &[String],
    primary: bool,
) {
    c.fill(area, BEZEL);
    let s = area.inset(3);
    c.fill(s, HOOD);
    c.fill(Area::new(s.x, s.y, s.w, 2), HOOD_LIGHT);
    let bar = Area::new(s.x, s.bottom() - 8, s.w, 8);
    c.fill(bar, HOOD_DARK);
    c.fill(Area::new(bar.x + 3, bar.y + 2, 4, 4), YELLOW);
    for i in 0..3 {
        c.fill(Area::new(bar.x + 10 + i * 7, bar.y + 2, 4, 4), HOOD_LIGHT);
    }
    if primary && s.w >= 100 {
        let imp = flatten(&icon::render(Look::Awake, 16), HOOD);
        c.image(s.right() - 20, s.y + 6, &imp, c.scale);
    }
    screen_text(c, text, s, label, lines, PALE, 9);
}

/// The client screen: bezel, GNOME-like top bar and app grid, status.
fn draw_remote(
    c: &mut Canvas,
    text: &mut Vec<TextRun>,
    area: Area,
    label: &str,
    size: &str,
    online: bool,
    lifted: bool,
) {
    c.fill(area, BEZEL);
    if lifted {
        c.frame(area, 2, HOOD_LIGHT);
    }
    let s = area.inset(3);
    c.fill(s, REMOTE);
    c.fill(Area::new(s.x, s.y, s.w, 6), BEZEL);
    c.fill(Area::new(s.x + s.w / 2 - 7, s.y + 2, 14, 2), GREY);
    for i in 0..6 {
        let (gx, gy) = (i % 3, i / 3);
        c.fill(
            Area::new(s.right() - 20 + gx * 6, s.bottom() - 14 + gy * 6, 4, 4),
            GREY,
        );
    }
    let status = if online { "online" } else { "offline" };
    let lines = [size.to_string(), format!("   {status}")];
    let top = screen_text(c, text, s, label, &lines, 0xa9a5c7, 12);
    if let Some(y) = top {
        // Status light in front of the second line.
        let dot = if online { GREEN } else { GREY };
        c.fill(Area::new(s.x + 8, y + 14 + 4, 6, 6), dot);
    }
}

/// Pixel-font label plus system-font lines inside screen `s`, as far as
/// they fit. Returns the y of the first line, if any was drawn.
fn screen_text(
    c: &mut Canvas,
    text: &mut Vec<TextRun>,
    s: Area,
    label: &str,
    lines: &[String],
    color: Color,
    top: i32,
) -> Option<i32> {
    if s.h < 24 || s.w < 40 {
        return None;
    }
    c.text(s.x + 8, s.y + top, label, WHITE, 1);
    let mut y = s.y + top + 11;
    let first = y;
    for line in lines {
        if y + 14 > s.bottom() - 9 {
            break;
        }
        text.push(run(Area::new(s.x + 8, y, s.w - 14, 14), line, color, 12));
        y += 14;
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> Panel {
        let desktop = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        Panel::new(
            Screens {
                desktop,
                remote_width: 1920,
                remote_height: 1080,
            },
            vec![Monitor {
                rect: desktop,
                primary: true,
            }],
            Arrangement {
                side: Side::Right,
                offset: None,
            },
            "absolute",
        )
    }

    fn centre(a: Area) -> (i32, i32) {
        (a.x + a.w / 2, a.y + a.h / 2)
    }

    #[test]
    fn dragging_the_client_screen_moves_it_to_another_side() {
        let mut p = panel();
        let h = History::default();
        let (x, y) = centre(p.remote_area());
        assert_eq!(p.hit(x, y, &h), Some(Hit::Remote));
        p.mouse_down(x, y, &h);
        // Drag it far below the desktop.
        let stage_bottom = STAGE.bottom() - 5;
        let (cx, _) = centre(p.map().rect(p.screens.desktop));
        p.mouse_move(cx, stage_bottom, &h);
        assert_eq!(p.current.side, Side::Bottom);
        assert_eq!(p.mouse_up(cx, stage_bottom, &h), Action::Redraw);
        assert_ne!(p.current, p.saved);
        // Reset brings back the saved arrangement.
        let reset = p.buttons()[1].1;
        p.mouse_down(reset.x + 2, reset.y + 2, &h);
        p.mouse_up(reset.x + 2, reset.y + 2, &h);
        assert_eq!(p.current, p.saved);
    }

    #[test]
    fn arrows_nudge_along_the_edge() {
        let mut p = panel();
        let h = History::default();
        p.key(Key::Down, false, &h);
        assert_eq!(p.current.offset, Some(10));
        p.key(Key::Up, true, &h);
        assert_eq!(p.current.offset, Some(9));
        // Across the edge does nothing.
        assert_eq!(p.key(Key::Left, false, &h), Action::None);
        assert_eq!(p.key(Key::Enter, false, &h), Action::Apply(p.current));
    }

    #[test]
    fn clicking_a_row_copies_it_and_typing_searches() {
        let mut p = panel();
        let mut h = History::default();
        let now = Instant::now();
        h.push(Content::Text("hello".into()), Origin::Local, now);
        h.push(Content::Text("world".into()), Origin::Remote, now);
        p.key(Key::Tab, false, &h);
        assert_eq!(p.tab, Tab::Clipboard);
        let (row, e) = p.rows(&h)[1];
        let id = e.id;
        let (x, y) = (row.x + 100, row.y + 10);
        p.mouse_down(x, y, &h);
        assert_eq!(p.mouse_up(x, y, &h), Action::Copy(id));
        for ch in "wor".chars() {
            assert!(p.char(ch));
        }
        assert_eq!(p.rows(&h).len(), 1);
        p.key(Key::Escape, false, &h);
        assert_eq!(p.rows(&h).len(), 2);
        assert_eq!(p.key(Key::Escape, false, &h), Action::Close);
    }

    #[test]
    fn delete_keeps_a_selection() {
        let mut p = panel();
        let mut h = History::default();
        let now = Instant::now();
        for t in ["a", "b", "c"] {
            h.push(Content::Text(t.into()), Origin::Local, now);
        }
        p.tab = Tab::Clipboard;
        p.key(Key::Down, false, &h);
        let first = p.selected.unwrap();
        assert_eq!(p.key(Key::Delete, false, &h), Action::Delete(first));
        assert!(p.selected.is_some_and(|id| id != first));
    }

    #[test]
    fn renders_both_tabs_without_panicking() {
        let mut p = panel();
        let mut h = History::default();
        let now = Instant::now();
        h.push(Content::Text("x".into()), Origin::Local, now);
        let img = Image {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };
        h.push(
            Content::Image {
                png: vec![1],
                width: 2,
                height: 2,
                thumb: img,
            },
            Origin::Remote,
            now,
        );
        for scale in [1, 2, 3] {
            let mut c = Canvas::new(WIDTH, HEIGHT, scale);
            let runs = p.render(&mut c, &h, now);
            assert!(runs.iter().any(|r| r.text == "right"));
            p.tab = Tab::Clipboard;
            let runs = p.render(&mut c, &h, now);
            assert!(runs.iter().any(|r| r.text == "image 2×2"));
            p.tab = Tab::Screens;
        }
    }
}
