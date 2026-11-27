//! Image formats for the clipboard, written from scratch: Windows keeps
//! images as DIBs, GNOME exchanges them as PNG. Pure logic, no OS calls.

mod deflate;
pub mod dib;
mod inflate;
pub mod png;

/// Refuse anything above this many pixels (about 16K x 16K).
const MAX_PIXELS: u64 = 1 << 28;

/// 8-bit RGBA pixels, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

/// DIB → PNG. The clipboard path decodes the DIB itself to keep the
/// pixels for the history thumbnail.
#[cfg(test)]
pub fn dib_to_png(dib: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(png::encode(&dib::decode(dib)?))
}

/// PNG → DIB, for clipboard images coming from the client.
pub fn png_to_dib(data: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(dib::encode(&png::decode(data)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_png_dib() {
        let img = Image {
            width: 2,
            height: 2,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
            ],
        };
        let dib = dib::encode(&img);
        let png = dib_to_png(&dib).unwrap();
        assert_eq!(png_to_dib(&png).unwrap(), dib);
    }
}
