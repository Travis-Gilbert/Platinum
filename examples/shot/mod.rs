//! A window captured from GPUI's own render, shared by the live examples'
//! `--shots`. It needs no screen recording permission, and it sees what the
//! window painted, the GPUI-drawn menus included.
use std::path::Path;

use gpui_kit::{AnyWindowHandle, AsyncApp};

/// Render the window to a PNG.
///
/// The frame is drawn here, not left to the display's clock: macOS stops
/// asking a window for frames while it is occluded (a locked screen, a display
/// asleep), and the last frame such a window holds is empty. A frame in which
/// almost nothing differs from its top-left pixel is reported and not saved, so
/// a blank render is never kept as evidence.
pub fn capture(window: AnyWindowHandle, cx: &mut AsyncApp, path: &Path) -> bool {
    let saved = window.update(cx, |_, window, cx| {
        window.draw(cx).clear(cx);
        println!(
            "        window visible to the system: {}; quads painted: {}",
            window.is_visible(),
            window.painted_quads().len()
        );
        let image = window.render_to_image()?;
        let ground = *image.get_pixel(0, 0);
        let marked = image.pixels().filter(|pixel| **pixel != ground).count();
        let total = image.width() as usize * image.height() as usize;
        if marked * 1000 < total {
            anyhow::bail!("{marked} of {total} pixels differ from the ground: nothing was painted");
        }
        image.save(path).map_err(anyhow::Error::from)
    });
    match saved {
        Ok(Ok(())) => {
            println!("        screenshot: {}", path.display());
            true
        }
        Ok(Err(error)) | Err(error) => {
            println!("        screenshot not taken: {error}");
            false
        }
    }
}
