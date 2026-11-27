//! Windows clipboard access: text (`CF_UNICODETEXT`) and images (the
//! registered "PNG" format when an application offers it, else `CF_DIB`).

use std::sync::OnceLock;

use bamf_proto::{MIME_PNG, MIME_TEXT};

use super::ffi::*;
use crate::image;

/// Clipboard content as read on this machine, before any conversion.
pub enum Local {
    Text(String),
    Png(Vec<u8>),
    Dib(Vec<u8>),
}

impl Local {
    /// MIME type and bytes for the client, plus the decoded pixels when
    /// they were needed anyway (DIB → PNG). Conversion happens here, off
    /// the input thread.
    pub fn into_wire(self) -> Result<(&'static str, Vec<u8>, Option<image::Image>), image::Error> {
        Ok(match self {
            Local::Text(text) => (MIME_TEXT, text.into_bytes(), None),
            Local::Png(png) => (MIME_PNG, png, None),
            Local::Dib(dib) => {
                let img = image::dib::decode(&dib)?;
                (MIME_PNG, image::png::encode(&img), Some(img))
            }
        })
    }
}

fn png_format() -> u32 {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    *FORMAT.get_or_init(|| unsafe { RegisterClipboardFormatW(wide("PNG").as_ptr()) })
}

/// Opens the clipboard, retrying briefly: another application may hold it
/// for a moment right after changing it.
fn open(hwnd: HWND) -> Option<Guard> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(hwnd) } != 0 {
            return Some(Guard);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    bamf_proto::warn!("clipboard is busy");
    None
}

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

/// Copies the bytes of a clipboard handle.
unsafe fn handle_bytes(handle: isize) -> Option<Vec<u8>> {
    if handle == 0 {
        return None;
    }
    let size = GlobalSize(handle);
    let ptr = GlobalLock(handle) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    GlobalUnlock(handle);
    Some(bytes)
}

/// Reads the clipboard. Text wins over images: applications that put both
/// (spreadsheets, office suites) mean the text.
pub fn read(hwnd: HWND) -> Option<Local> {
    let _guard = open(hwnd)?;
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            let bytes = handle_bytes(GetClipboardData(CF_UNICODETEXT))?;
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            let text = String::from_utf16_lossy(&units).replace("\r\n", "\n");
            return Some(Local::Text(text));
        }
        if IsClipboardFormatAvailable(png_format()) != 0 {
            if let Some(png) = handle_bytes(GetClipboardData(png_format())) {
                return Some(Local::Png(png));
            }
        }
        if IsClipboardFormatAvailable(CF_DIB) != 0 {
            return handle_bytes(GetClipboardData(CF_DIB)).map(Local::Dib);
        }
    }
    None
}

/// Moves `bytes` into a movable global memory block for SetClipboardData.
unsafe fn global(bytes: &[u8]) -> Option<HGLOBAL> {
    let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
    if mem == 0 {
        return None;
    }
    let ptr = GlobalLock(mem) as *mut u8;
    if ptr.is_null() {
        GlobalFree(mem);
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    GlobalUnlock(mem);
    Some(mem)
}

/// Sets the clipboard from content received from the client.
pub fn write(hwnd: HWND, mime: &str, data: &[u8]) -> Result<(), String> {
    // Convert before taking the clipboard, so it is held only briefly.
    let formats: Vec<(u32, Vec<u8>)> = if mime.starts_with("text/") {
        let text = String::from_utf8_lossy(data)
            .replace("\r\n", "\n")
            .replace('\n', "\r\n");
        let bytes = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        vec![(CF_UNICODETEXT, bytes)]
    } else if mime == MIME_PNG {
        let dib = image::png_to_dib(data).map_err(|e| e.to_string())?;
        vec![(png_format(), data.to_vec()), (CF_DIB, dib)]
    } else {
        return Err(format!("unsupported clipboard type {mime}"));
    };

    let _guard = open(hwnd).ok_or("clipboard is busy")?;
    unsafe {
        EmptyClipboard();
        for (format, bytes) in formats {
            let mem = global(&bytes).ok_or("out of memory")?;
            if SetClipboardData(format, mem) == 0 {
                // Ownership only passes to the system on success.
                GlobalFree(mem);
                return Err("SetClipboardData failed".into());
            }
        }
    }
    Ok(())
}
