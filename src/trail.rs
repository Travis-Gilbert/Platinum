//! Serial history writer with bounded optional previews. Rendering owns the copy; SQLite owns the
//! encoded visit. Capture never extends the lifetime of a Servo frame.
use frame_profile::ProfileStore;
use gpui_kit::{App, Task};
use std::{sync::mpsc, time::Duration};

pub struct Writer {
    send: Option<mpsc::Sender<Capture>>,
    previews: mpsc::SyncSender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
    _reports: Task<()>,
}
struct Capture {
    visited: i64,
    tab: String,
    url: String,
    title: String,
    pixels: Option<image::RgbaImage>,
}
impl Writer {
    pub fn new(
        store: ProfileStore,
        window: gpui_kit::AnyWindowHandle,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        // At most four retained frames (135 MiB maximum), compressed off the UI thread.
        // Small visit records queue independently of the four-frame budget.
        // Preview pressure must never drop history.
        let (send, receive) = mpsc::channel::<Capture>();
        let (previews, slots) = mpsc::sync_channel::<()>(4);
        let (errors, reports) = mpsc::sync_channel(8);
        let worker = std::thread::Builder::new()
            .name("platinum-visits".into())
            .spawn(move || {
                while let Ok(capture) = receive.recv() {
                    let had_preview = capture.pixels.is_some();
                    let result = persist(&store, capture);
                    if had_preview {
                        let _ = slots.recv();
                    }
                    if let Err(error) = result {
                        let _ = errors.try_send(error.to_string());
                    }
                }
            })?;
        let task = cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                match reports.try_recv() {
                    Ok(error) => {
                        use gpui_kit::component::{WindowExt as _, notification::Notification};
                        let _ = window.update(cx, |_, window, cx| {
                            window.push_notification(
                                Notification::error(format!("Visit could not be saved: {error}")),
                                cx,
                            )
                        });
                    }
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        });
        Ok(Self {
            send: Some(send),
            previews,
            worker: Some(worker),
            _reports: task,
        })
    }
    pub fn drain(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.send.take();
        self.worker.take()
    }
    pub fn record(&self, tab: String, page: &crate::page::Page) -> anyhow::Result<()> {
        let visited = frame_profile::now();
        let Some(url) = page
            .url()
            .filter(|u| frame_profile::web_url(u.as_str()).is_some())
        else {
            return Ok(());
        };
        let pixels = (page.frame_navigation == Some(page.delegate.navigation_starts.get()))
            .then(|| {
                page.context
                    .with_pixels(|bytes, width, height, stride| {
                        // Copy only while locked. Servo presents BGRA, bottom row first;
                        // image owns the new RGBA pixels before the lock is released.
                        if width > 8192 || height > 8192 || width.saturating_mul(height) > 8_847_360
                        {
                            return None;
                        }
                        let mut pixels = Vec::with_capacity(width * height * 4);
                        for y in (0..height).rev() {
                            for pixel in bytes[y * stride..y * stride + width * 4].chunks_exact(4) {
                                pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                            }
                        }
                        image::RgbaImage::from_raw(width as u32, height as u32, pixels)
                    })
                    .flatten()
            })
            .flatten();
        enqueue(
            self.send
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Visit writer has closed"))?,
            &self.previews,
            Capture {
                visited,
                tab,
                url: url.into(),
                title: page.title().to_string(),
                pixels,
            },
        )
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(worker) = self.drain() {
            let _ = worker.join();
        }
    }
}

// Only images consume scarce preview slots. Lightweight records preserve FIFO
// predecessor ordering even when every preview slot is occupied.
fn enqueue(
    send: &mpsc::Sender<Capture>,
    previews: &mpsc::SyncSender<()>,
    mut capture: Capture,
) -> anyhow::Result<()> {
    if capture.pixels.is_some() && previews.try_send(()).is_err() {
        capture.pixels = None;
    }
    send.send(capture)
        .map_err(|_| anyhow::anyhow!("Visit writer has closed"))
}

fn persist(store: &ProfileStore, capture: Capture) -> anyhow::Result<()> {
    let preview = capture.pixels.and_then(|pixels| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        match image::DynamicImage::ImageRgba8(pixels)
            .thumbnail(480, 300)
            .write_to(&mut bytes, image::ImageFormat::Png)
        {
            Ok(()) if bytes.get_ref().len() <= 512 * 1024 => Some(bytes.into_inner()),
            _ => {
                log::warn!("Visit preview could not be encoded; saving history without it");
                None
            }
        }
    });
    store.record_visit_at(
        &capture.tab,
        &capture.url,
        &capture.title,
        preview.as_deref(),
        capture.visited,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_preview_queue_retains_every_visit_and_history_update() {
        let folder = tempfile::tempdir().unwrap();
        let store = ProfileStore::open(folder.path()).unwrap();
        let (send, receive) = mpsc::channel();
        let (previews, slots) = mpsc::sync_channel(4);
        // Deliberately stall the consumer until all arrivals are queued.
        for visited in 0..12 {
            enqueue(
                &send,
                &previews,
                Capture {
                    visited,
                    tab: "tab-1".into(),
                    url: "https://example.org/".into(),
                    title: format!("Visit {visited}"),
                    pixels: Some(image::RgbaImage::new(2, 2)),
                },
            )
            .unwrap();
        }
        drop(send);
        for (index, capture) in receive.into_iter().enumerate() {
            assert_eq!(capture.pixels.is_some(), index < 4);
            persist(&store, capture).unwrap();
            if index < 4 {
                slots.recv().unwrap();
            }
        }
        let visits = store.visits(None, 100).unwrap();
        assert_eq!(visits.len(), 12);
        assert_eq!(visits.iter().filter(|v| v.preview.is_some()).count(), 4);
        for pair in visits.windows(2) {
            assert_eq!(pair[0].predecessor, Some(pair[1].id));
        }
        assert!(visits.last().unwrap().predecessor.is_none());
        assert_eq!(
            store.suggestions("", false, 10).unwrap()[0].title,
            "Visit 11"
        );
    }
}
