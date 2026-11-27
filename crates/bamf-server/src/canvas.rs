//! A pixel buffer the settings window draws into before handing it to the
//! screen. Coordinates are logical pixels, multiplied by a whole-number
//! `scale` so the pixel art stays crisp on high-DPI screens.
//! Pure logic, no OS calls.

use crate::image::Image;
use crate::pixfont;

/// 0xRRGGBB.
pub type Color = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn inset(&self, by: i32) -> Area {
        Area::new(self.x + by, self.y + by, self.w - 2 * by, self.h - 2 * by)
    }
}

pub struct Canvas {
    /// Physical size.
    pub width: i32,
    pub height: i32,
    pub scale: i32,
    /// 0x00RRGGBB per pixel, top row first: the layout of a 32-bit
    /// top-down DIB.
    pub pixels: Vec<u32>,
    /// Logical clip rectangle.
    clip: Area,
}

impl Canvas {
    /// A canvas of `w` x `h` logical pixels.
    pub fn new(w: i32, h: i32, scale: i32) -> Self {
        let scale = scale.max(1);
        let (width, height) = (w * scale, h * scale);
        Self {
            width,
            height,
            scale,
            pixels: vec![0; (width * height).max(0) as usize],
            clip: Area::new(0, 0, w, h),
        }
    }

    pub fn logical_size(&self) -> (i32, i32) {
        (self.width / self.scale, self.height / self.scale)
    }

    /// Limits drawing to `area` (logical) until [`Canvas::unclip`].
    pub fn clip(&mut self, area: Area) {
        self.clip = area;
    }

    pub fn unclip(&mut self) {
        let (w, h) = self.logical_size();
        self.clip = Area::new(0, 0, w, h);
    }

    /// Fills a rectangle given in physical pixels, inside the clip.
    fn fill_physical(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let s = self.scale;
        let c = self.clip;
        let x0 = x0.max(c.x * s).max(0);
        let y0 = y0.max(c.y * s).max(0);
        let x1 = x1.min(c.right() * s).min(self.width);
        let y1 = y1.min(c.bottom() * s).min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let row = (y * self.width) as usize;
            self.pixels[row + x0 as usize..row + x1 as usize].fill(color);
        }
    }

    pub fn fill(&mut self, a: Area, color: Color) {
        let s = self.scale;
        self.fill_physical(a.x * s, a.y * s, a.right() * s, a.bottom() * s, color);
    }

    /// A `width`-pixel frame inside `a`.
    pub fn frame(&mut self, a: Area, width: i32, color: Color) {
        self.fill(Area::new(a.x, a.y, a.w, width), color);
        self.fill(Area::new(a.x, a.bottom() - width, a.w, width), color);
        self.fill(Area::new(a.x, a.y, width, a.h), color);
        self.fill(Area::new(a.right() - width, a.y, width, a.h), color);
    }

    /// Draws `text` in the pixel font with each font pixel `dot` logical
    /// pixels wide. Returns the logical width drawn.
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Color, dot: i32) -> i32 {
        let mut cx = x;
        for c in text.chars() {
            if let Some(rows) = pixfont::glyph(c) {
                for (gy, row) in rows.iter().enumerate() {
                    for (gx, b) in row.bytes().enumerate() {
                        if b == b'#' {
                            let px = cx + gx as i32 * dot;
                            let py = y + gy as i32 * dot;
                            self.fill(Area::new(px, py, dot, dot), color);
                        }
                    }
                }
            }
            cx += (pixfont::WIDTH + pixfont::SPACING) * dot;
        }
        text_width(text, dot)
    }

    /// Draws a character-map sprite: each byte of `rows` is looked up in
    /// `palette` (`None` = transparent), each cell `dot` logical pixels.
    pub fn sprite(
        &mut self,
        x: i32,
        y: i32,
        rows: &[&str],
        dot: i32,
        palette: impl Fn(u8) -> Option<Color>,
    ) {
        for (sy, row) in rows.iter().enumerate() {
            for (sx, b) in row.bytes().enumerate() {
                if let Some(c) = palette(b) {
                    self.fill(
                        Area::new(x + sx as i32 * dot, y + sy as i32 * dot, dot, dot),
                        c,
                    );
                }
            }
        }
    }

    /// Draws RGBA pixels with physical pixel size `px` (1 = one image pixel
    /// per screen pixel), top-left at logical `x`, `y`. Alpha is ignored.
    pub fn image(&mut self, x: i32, y: i32, img: &Image, px: i32) {
        let s = self.scale;
        for iy in 0..img.height as i32 {
            for ix in 0..img.width as i32 {
                let i = ((iy * img.width as i32 + ix) * 4) as usize;
                let p = &img.rgba[i..i + 4];
                let color = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
                let (x0, y0) = (x * s + ix * px, y * s + iy * px);
                self.fill_physical(x0, y0, x0 + px, y0 + px, color);
            }
        }
    }
}

/// Logical width of `text` in the pixel font at `dot`.
pub fn text_width(text: &str, dot: i32) -> i32 {
    pixfont::width(text) * dot
}

/// Pixel-font text height at `dot`.
pub fn text_height(dot: i32) -> i32 {
    pixfont::HEIGHT * dot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(c: &Canvas, x: i32, y: i32) -> u32 {
        c.pixels[(y * c.width + x) as usize]
    }

    #[test]
    fn fill_scales_and_clips() {
        let mut c = Canvas::new(10, 10, 2);
        assert_eq!((c.width, c.height), (20, 20));
        c.fill(Area::new(8, 8, 5, 5), 0xff);
        assert_eq!(at(&c, 16, 16), 0xff);
        assert_eq!(at(&c, 15, 16), 0);
        c.clip(Area::new(0, 0, 2, 2));
        c.fill(Area::new(0, 0, 10, 10), 0x1);
        assert_eq!(at(&c, 3, 3), 0x1);
        assert_eq!(at(&c, 4, 4), 0);
        c.unclip();
        c.fill(Area::new(-5, -5, 6, 6), 0x2);
        assert_eq!(at(&c, 0, 0), 0x2);
    }

    #[test]
    fn text_draws_glyph_ink() {
        let mut c = Canvas::new(12, 7, 1);
        assert_eq!(c.text(0, 0, "I", 0xabc, 1), 5);
        // Top row of "I" is solid.
        assert!((0..5).all(|x| at(&c, x, 0) == 0xabc));
        assert_eq!(at(&c, 0, 1), 0);
    }

    #[test]
    fn image_ignores_alpha_and_scales_by_px() {
        let mut c = Canvas::new(4, 4, 1);
        let img = Image {
            width: 1,
            height: 1,
            rgba: vec![0x12, 0x34, 0x56, 0],
        };
        c.image(1, 1, &img, 2);
        assert_eq!(at(&c, 2, 2), 0x123456);
        assert_eq!(at(&c, 3, 3), 0);
    }
}
