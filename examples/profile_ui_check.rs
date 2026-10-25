//! Render the production profile library with synthetic data in a real window.
extern crate gpui_kit as gpui;
#[allow(dead_code)]
#[path = "../src/icons.rs"]
mod icons;
#[allow(dead_code)]
#[path = "../src/profile_ui.rs"]
mod profile_ui;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;
use gpui_kit::component::Root;
use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};
fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Supply a PNG output path"))?;
    let temp = tempfile::tempdir()?;
    let store = frame_profile::ProfileStore::open(temp.path())?;
    store.imported_profile("synthetic-work", "Work", "work@example.test")?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::new(
                            gpui_kit::point(px(100.), px(100.)),
                            size(px(540.), px(760.)),
                        ))),
                        ..Default::default()
                    },
                    |window, cx| {
                        let panel = cx.new(|cx| {
                            profile_ui::ProfilePanel::new(
                                store,
                                profile_ui::Section::Profiles,
                                window,
                                cx,
                            )
                        });
                        cx.new(|cx| Root::new(panel, window, cx))
                    },
                )
                .expect("native window");
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let saved = window.update(cx, |_, window, _| {
                    window
                        .render_to_image()
                        .and_then(|image| image.save(&output).map_err(Into::into))
                });
                match saved {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("Could not capture the profile panel: {error:#}"),
                    Err(error) => eprintln!("Could not reach the profile window: {error:#}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}
