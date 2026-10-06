//! The real folder icon, taken from the shell rather than drawn by us.
//!
//! These rows are actual directories, so they should carry the icon the operating
//! system gives them — including the custom icon a folder sets for itself through
//! `desktop.ini`, which a hand-drawn glyph can never reproduce.
//!
//! The icon is located with `SHGetFileInfoW` and then extracted at the exact size
//! wanted with `PrivateExtractIconsW`. Asking for the size directly matters: the
//! usual `SHGFI_ICON` route hands back a 32px icon, and upscaling that to 88px
//! produces exactly the soft, muddy result this is meant to avoid. Windows ships
//! folder icons at 16, 32, 48 and 256, and this picks the right one.

use crate::{ArtError, Thumb, ThumbSize};
use std::path::Path;

/// Which icon to draw, and at what size — the key a rendered icon is cached under.
///
/// Almost every folder resolves to the same source file and index, so caching on
/// this collapses a whole library down to one rendered icon per size while still
/// letting a folder with a custom icon have its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconKey {
    pub file: String,
    pub index: i32,
    pub size: ThumbSize,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
    use windows_sys::Win32::UI::Shell::{
        SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICONLOCATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Asks the shell which file and index hold this path's icon.
    ///
    /// The real path is passed rather than `SHGFI_USEFILEATTRIBUTES`, so a folder
    /// that has set its own icon reports that icon instead of the generic one.
    pub fn locate(path: &Path, size: ThumbSize) -> Option<IconKey> {
        let wide_path = wide(&path.display().to_string());
        let mut info = SHFILEINFOW::default();

        // SAFETY: `wide_path` is NUL-terminated and outlives the call; `info` is a
        // correctly sized, zeroed SHFILEINFOW.
        let ok = unsafe {
            SHGetFileInfoW(
                wide_path.as_ptr(),
                FILE_ATTRIBUTE_DIRECTORY,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICONLOCATION,
            )
        };
        let end = info
            .szDisplayName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDisplayName.len());
        let file = String::from_utf16_lossy(&info.szDisplayName[..end]);
        if ok != 0 && !file.is_empty() {
            return Some(IconKey {
                file,
                index: info.iIcon,
                size,
            });
        }

        // A program carries its own icon, so the shell names no location for it
        // - each .exe is its own source. The first icon in the file is the one
        // Explorer shows.
        if carries_own_icon(path) {
            return Some(IconKey {
                file: path.display().to_string(),
                index: 0,
                size,
            });
        }
        None
    }

    fn carries_own_icon(path: &Path) -> bool {
        path.is_file()
            && path.extension().is_some_and(|e| {
                ["exe", "dll", "ico"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            })
    }

    /// Renders a located icon to PNG at its key's size.
    pub fn render(key: &IconKey) -> Result<Thumb, ArtError> {
        let px = key.size.px();
        let file = wide(&key.file);
        let mut icon: HICON = std::ptr::null_mut();

        // SHGetFileInfoW reports the icon's location the way the shell records
        // it, where a *negative* index means a resource ID rather than an offset
        // into the file's icon table. `SHDefExtractIconW` understands that
        // convention; `PrivateExtractIconsW` does not, and silently hands back
        // the wrong icon — every drive looked identical because C:'s icon is
        // resource -36 and it was being read as index 36.
        //
        // This matters beyond drives: a folder that sets its own icon through
        // `desktop.ini` usually names it by resource ID too.
        //
        // SAFETY: `file` is NUL-terminated; `icon` is a valid destination for
        // one HICON, and the small-icon slot is declined with a null pointer.
        let hr = unsafe {
            SHDefExtractIconW(
                file.as_ptr(),
                key.index,
                0,
                &mut icon,
                std::ptr::null_mut(),
                px,
            )
        };

        // Falls back to the index-based extractor, which is still right for the
        // ordinary case of a positive index into an icon table.
        if hr != 0 || icon.is_null() {
            // SAFETY: same contract as above; exactly one icon is requested.
            let got = unsafe {
                PrivateExtractIconsW(
                    file.as_ptr(),
                    key.index,
                    px as i32,
                    px as i32,
                    &mut icon,
                    std::ptr::null_mut(),
                    1,
                    0,
                )
            };
            if got == 0 || icon.is_null() {
                return Err(ArtError::Shell("no icon at that index".into()));
            }
        }

        let result = to_rgba(icon);
        // SAFETY: `icon` came from PrivateExtractIconsW and is not used again.
        unsafe { DestroyIcon(icon) };

        let (w, h, rgba) = result?;
        let img = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| ArtError::Shell("icon pixels did not fit its dimensions".into()))?;

        // The extracted icon is usually already the requested size, but Windows
        // falls back to the nearest available when it is not, so normalise.
        let img = image::DynamicImage::ImageRgba8(img);
        let scaled = if img.width() == px && img.height() == px {
            img
        } else {
            img.resize_exact(px, px, image::imageops::FilterType::Lanczos3)
        };

        let mut png = Vec::new();
        scaled
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(ArtError::Encode)?;

        Ok(Thumb {
            size: key.size,
            png,
        })
    }

    /// Copies an icon's colour bitmap out as straight RGBA.
    fn to_rgba(icon: HICON) -> Result<(u32, u32, Vec<u8>), ArtError> {
        let mut info = ICONINFO::default();
        // SAFETY: `icon` is a live HICON and `info` is a valid zeroed ICONINFO.
        if unsafe { GetIconInfo(icon, &mut info) } == 0 {
            return Err(ArtError::Shell("GetIconInfo failed".into()));
        }

        // Both bitmaps belong to the caller once GetIconInfo succeeds.
        let cleanup = |info: &ICONINFO| unsafe {
            if !info.hbmColor.is_null() {
                DeleteObject(info.hbmColor as _);
            }
            if !info.hbmMask.is_null() {
                DeleteObject(info.hbmMask as _);
            }
        };

        if info.hbmColor.is_null() {
            cleanup(&info);
            return Err(ArtError::Shell("icon has no colour bitmap".into()));
        }

        let mut bm = BITMAP {
            bmType: 0,
            bmWidth: 0,
            bmHeight: 0,
            bmWidthBytes: 0,
            bmPlanes: 0,
            bmBitsPixel: 0,
            bmBits: std::ptr::null_mut(),
        };
        // SAFETY: hbmColor is a valid HBITMAP and `bm` is the matching struct.
        let read = unsafe {
            GetObjectW(
                info.hbmColor as _,
                std::mem::size_of::<BITMAP>() as i32,
                &mut bm as *mut _ as *mut _,
            )
        };
        if read == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            cleanup(&info);
            return Err(ArtError::Shell("icon bitmap has no dimensions".into()));
        }

        let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];

        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bm.bmWidth,
            // Negative height requests top-down rows, so the buffer is already in
            // the order image decoders expect and needs no flip afterwards.
            biHeight: -bm.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        // SAFETY: a screen-compatible DC is created and released below; `buf` is
        // sized for w*h 32-bit pixels, matching the header.
        let dc: HDC = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        let lines = unsafe {
            GetDIBits(
                dc,
                info.hbmColor,
                0,
                h,
                buf.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        unsafe { DeleteDC(dc) };
        cleanup(&info);

        if lines == 0 {
            return Err(ArtError::Shell("GetDIBits returned no rows".into()));
        }

        // Windows hands back BGRA; swap into RGBA in place.
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        // Older icons carry no alpha channel at all and come back fully
        // transparent, which would render as nothing. Treat that as opaque rather
        // than showing an invisible icon.
        if buf.chunks_exact(4).all(|p| p[3] == 0) {
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }

        Ok((w, h, buf))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// No shell to ask. The frontend falls back to its own placeholder.
    pub fn locate(_path: &Path, _size: ThumbSize) -> Option<IconKey> {
        None
    }

    pub fn render(_key: &IconKey) -> Result<Thumb, ArtError> {
        Err(ArtError::Shell("shell icons are Windows-only".into()))
    }
}

pub use imp::{locate, render};

/// Renders the shell icon for one path, or `None` where there is no shell to ask.
pub fn folder_icon(path: &Path, size: ThumbSize) -> Option<Thumb> {
    let key = locate(path, size)?;
    render(&key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_real_directory_resolves_to_an_icon_source() {
        let key = locate(Path::new("C:\\Windows"), ThumbSize::Medium);
        let key = key.expect("the shell must name an icon file for C:\\Windows");
        assert!(!key.file.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_folder_icon_renders_square_at_the_requested_size() {
        for size in ThumbSize::all() {
            let Some(thumb) = folder_icon(Path::new("C:\\Windows"), size) else {
                panic!("no folder icon at {size:?}");
            };
            let img = image::load_from_memory(&thumb.png).unwrap();
            assert_eq!((img.width(), img.height()), (size.px(), size.px()));
        }
    }

    /// "Open with" shows each program by its own icon.
    #[cfg(windows)]
    #[test]
    fn a_program_renders_its_own_icon() {
        let exe = Path::new("C:\\Windows\\notepad.exe");
        let key = locate(exe, ThumbSize::Small).expect("notepad must have an icon");
        assert!(
            folder_icon(exe, ThumbSize::Small).is_some(),
            "{key:?} did not render"
        );
    }

    /// Every ordinary folder shares one icon source, so the cache key collapses
    /// a whole library to a single rendered icon per size.
    #[cfg(windows)]
    #[test]
    fn ordinary_folders_share_one_icon_key() {
        let a = locate(Path::new("C:\\Windows"), ThumbSize::Small);
        let b = locate(Path::new("C:\\Users"), ThumbSize::Small);
        assert_eq!(a, b);
    }
}
