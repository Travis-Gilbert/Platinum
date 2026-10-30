//! Bounded, serial history writer. Rendering owns the copy; SQLite owns the
//! encoded visit. Capture never extends the lifetime of a Servo frame.
use frame_profile::ProfileStore;
use gpui_kit::{App, Task};
use std::{sync::mpsc, time::Duration};

pub struct Writer {
    send: Option<mpsc::SyncSender<Capture>>,
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
        // At most four owned frames (135 MiB maximum), compressed off the UI thread.
        let (send, receive) = mpsc::sync_channel::<Capture>(4);
        let (errors, reports) = mpsc::sync_channel(8);
        let worker = std::thread::Builder::new()
            .name("platinum-visits".into())
            .spawn(move || {
                while let Ok(capture) = receive.recv() {
                    let result = (|| -> anyhow::Result<()> {
                        let preview = capture
                            .pixels
                            .map(|pixels| {
                                let mut bytes = std::io::Cursor::new(Vec::new());
                                image::DynamicImage::ImageRgba8(pixels)
                                    .thumbnail(480, 300)
                                    .write_to(&mut bytes, image::ImageFormat::Png)?;
                                Ok::<_, image::ImageError>(bytes.into_inner())
                            })
                            .transpose()?;
                        store.record_visit_at(
                            &capture.tab,
                            &capture.url,
                            &capture.title,
                            preview.as_deref(),
                            capture.visited,
                        )?;
                        Ok(())
                    })();
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
        self.send
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Visit writer has closed"))?
            .try_send(Capture {
                visited,
                tab,
                url: url.into(),
                title: page.title().to_string(),
                pixels,
            })
            .map_err(|_| anyhow::anyhow!("History storage is busy; this visit could not be saved"))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(worker) = self.drain() {
            let _ = worker.join();
        }
    }
}
