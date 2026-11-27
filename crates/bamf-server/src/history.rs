//! Clipboard history for the settings window, like Windows' Win+V: newest
//! first, duplicates move to the top, pinned items survive the cap and
//! "clear". Kept in memory only. Pure logic, no OS calls.

use std::time::{Duration, Instant};

use crate::image::Image;

/// Unpinned entries kept; older ones fall off the end.
pub const CAPACITY: usize = 25;

/// Which machine copied it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
        /// Small copy for the list.
        thumb: Image,
    },
}

impl Content {
    fn same(&self, other: &Content) -> bool {
        match (self, other) {
            (Content::Text(a), Content::Text(b)) => a == b,
            (Content::Image { png: a, .. }, Content::Image { png: b, .. }) => a == b,
            _ => false,
        }
    }

    /// One line for the list: the text with line breaks shown as ¶ and
    /// runs of spaces collapsed, or a description of the image.
    pub fn preview(&self, max_chars: usize) -> String {
        match self {
            Content::Text(text) => {
                let mut out = String::new();
                for (i, line) in text.split('\n').map(str::trim).enumerate() {
                    if i > 0 && !line.is_empty() && !out.is_empty() {
                        out.push_str(" ¶ ");
                    }
                    let mut last_space = false;
                    for c in line.chars() {
                        let space = c.is_whitespace();
                        if !(space && last_space) {
                            out.push(if space { ' ' } else { c });
                        }
                        last_space = space;
                    }
                    if out.chars().count() > max_chars {
                        break;
                    }
                }
                if out.chars().count() > max_chars {
                    out = out.chars().take(max_chars).collect();
                    out.push('…');
                }
                out
            }
            Content::Image { width, height, .. } => format!("image {width}×{height}"),
        }
    }

    /// `query` must already be lower case.
    fn matches(&self, query: &str) -> bool {
        match self {
            Content::Text(t) => t.to_lowercase().contains(query),
            Content::Image { .. } => self.preview(40).contains(query),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u64,
    pub content: Content,
    pub origin: Origin,
    pub at: Instant,
    pub pinned: bool,
}

#[derive(Debug, Default)]
pub struct History {
    /// Newest first.
    entries: Vec<Entry>,
    next_id: u64,
}

impl History {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
        }
    }

    /// Adds content copied on either machine. Content already in the list
    /// moves to the top, keeping its origin and pin. Empty text is ignored.
    /// Returns false when nothing was added.
    pub fn push(&mut self, content: Content, origin: Origin, now: Instant) -> bool {
        if let Content::Text(t) = &content {
            if t.trim().is_empty() {
                return false;
            }
        }
        let entry = match self.entries.iter().position(|e| e.content.same(&content)) {
            Some(i) => {
                let mut e = self.entries.remove(i);
                e.at = now;
                e
            }
            None => {
                self.next_id += 1;
                Entry {
                    id: self.next_id,
                    content,
                    origin,
                    at: now,
                    pinned: false,
                }
            }
        };
        self.entries.insert(0, entry);
        let mut unpinned = 0;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            unpinned += 1;
            unpinned <= CAPACITY
        });
        true
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn toggle_pin(&mut self, id: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = !e.pinned;
        }
    }

    /// Removes everything but the pinned entries.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Entries matching `query` (case-insensitive; empty matches all),
    /// newest first.
    pub fn filtered(&self, query: &str) -> impl Iterator<Item = &Entry> + '_ {
        let query = query.to_lowercase();
        self.entries
            .iter()
            .filter(move |e| query.is_empty() || e.content.matches(&query))
    }
}

/// Short age for the list: "now", "5m", "2h", "3d".
pub fn age(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Shrinks `img` to fit in `max_w` x `max_h` (nearest neighbour, never
/// enlarged), with transparent pixels over the list background colour.
pub fn thumbnail(img: &Image, max_w: u32, max_h: u32, background: [u8; 3]) -> Image {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let scale = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        let sy = (y as u64 * h as u64 / th as u64) as u32;
        for x in 0..tw {
            let sx = (x as u64 * w as u64 / tw as u64) as u32;
            let i = ((sy * w + sx) * 4) as usize;
            let px = img.rgba.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
            let a = px[3] as u32;
            for c in 0..3 {
                let v = (px[c] as u32 * a + background[c] as u32 * (255 - a)) / 255;
                rgba.push(v as u8);
            }
            rgba.push(255);
        }
    }
    Image {
        width: tw,
        height: th,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    fn ids(h: &History) -> Vec<String> {
        h.filtered("")
            .map(|e| match &e.content {
                Content::Text(t) => t.clone(),
                c => c.preview(20),
            })
            .collect()
    }

    #[test]
    fn newest_first_and_duplicates_move_up() {
        let now = Instant::now();
        let mut h = History::default();
        h.push(text("a"), Origin::Local, now);
        h.push(text("b"), Origin::Remote, now);
        h.push(text("a"), Origin::Remote, now);
        assert_eq!(ids(&h), ["a", "b"]);
        // The echo of remote content keeps its first origin.
        assert_eq!(h.filtered("").next().unwrap().origin, Origin::Local);
        assert!(!h.push(text("  \n"), Origin::Local, now));
    }

    #[test]
    fn cap_spares_pinned_entries() {
        let now = Instant::now();
        let mut h = History::default();
        h.push(text("keep"), Origin::Local, now);
        let id = h.filtered("").next().unwrap().id;
        h.toggle_pin(id);
        for i in 0..CAPACITY + 5 {
            h.push(text(&i.to_string()), Origin::Local, now);
        }
        assert_eq!(h.len(), CAPACITY + 1);
        assert!(h.get(id).is_some());
        h.clear();
        assert_eq!(ids(&h), ["keep"]);
        h.remove(id);
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn search_is_case_insensitive() {
        let now = Instant::now();
        let mut h = History::default();
        h.push(text("Cargo build"), Origin::Local, now);
        h.push(text("git push"), Origin::Local, now);
        assert_eq!(h.filtered("CARGO").count(), 1);
        assert_eq!(h.filtered("").count(), 2);
    }

    #[test]
    fn preview_is_one_line() {
        assert_eq!(text("a  b\n\n  c\n").preview(40), "a b ¶ c");
        assert_eq!(text("abcdef").preview(3), "abc…");
    }

    #[test]
    fn ages() {
        assert_eq!(age(Duration::from_secs(59)), "now");
        assert_eq!(age(Duration::from_secs(150)), "2m");
        assert_eq!(age(Duration::from_secs(7300)), "2h");
        assert_eq!(age(Duration::from_secs(200_000)), "2d");
    }

    #[test]
    fn thumbnail_fits_and_flattens_alpha() {
        let img = Image {
            width: 4,
            height: 2,
            rgba: [[255, 0, 0, 255], [0, 0, 0, 0]].repeat(4).concat(),
        };
        let t = thumbnail(&img, 2, 2, [10, 20, 30]);
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(&t.rgba[..4], &[255, 0, 0, 255]);
        let small = thumbnail(&img, 100, 100, [0, 0, 0]);
        assert_eq!((small.width, small.height), (4, 2));
    }
}
