use super::*;
use gpui_kit::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div};
use std::cell::Cell;
use std::hash::{Hash, Hasher};

struct Cards {
    pages: Vec<Entity<page::Page>>,
    boards: Vec<Entity<canvas::SpatialCanvas>>,
    hidden: bool,
}
impl Render for Cards {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        div().size_full().flex().flex_wrap().children(
            self.boards.iter().filter(|_| !self.hidden).map(|p| {
                div()
                    .w(viewport.width / 4.)
                    .h(viewport.height / 4.)
                    .overflow_hidden()
                    .child(p.clone())
            }),
        )
    }
}

fn resident_bytes() -> anyhow::Result<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()?;
    Ok(String::from_utf8(output.stdout)?.trim().parse::<u64>()? * 1024)
}

async fn memory_snapshot(
    dir: &std::path::Path,
    name: &str,
    cx: &AsyncApp,
) -> anyhow::Result<Value> {
    let started = Instant::now();
    let rss = resident_bytes()?;
    let pid = std::process::id().to_string();
    let output = cx
        .background_executor()
        .spawn(async move {
            std::process::Command::new("vmmap")
                .args(["--summary", &pid])
                .output()
        })
        .await?;
    let text = String::from_utf8(output.stdout)?;
    std::fs::write(dir.join(format!("vmmap-{name}.txt")), &text)?;
    anyhow::ensure!(
        output.status.success(),
        "vmmap failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let footprint = text
        .lines()
        .find_map(|line| line.strip_prefix("Physical footprint:"))
        .ok_or_else(|| anyhow::anyhow!("vmmap physical footprint missing"))?
        .trim();
    let (number, factor) = match footprint.chars().last() {
        Some('K') => (&footprint[..footprint.len() - 1], 1024.),
        Some('M') => (&footprint[..footprint.len() - 1], 1024. * 1024.),
        Some('G') => (&footprint[..footprint.len() - 1], 1024. * 1024. * 1024.),
        Some('B') => (&footprint[..footprint.len() - 1], 1.),
        _ => (footprint, 1.),
    };
    let footprint_bytes = (number.trim().parse::<f64>()? * factor).round() as u64;
    let regions = text
        .lines()
        .filter(|line| line.to_ascii_lowercase().contains("iosurface"))
        .collect::<Vec<_>>();
    let result = json!({"rss_bytes":rss,"physical_footprint_bytes":footprint_bytes,
        "physical_footprint_reported":footprint,"iosurface_regions":regions,
        "iosurface_region_reported":!regions.is_empty(),"vmmap_file":format!("vmmap-{name}.txt"),
        "elapsed_seconds":started.elapsed().as_secs_f64(),
        "precision":"Physical footprint rounded by vmmap; IOSurface lines retain all original columns. Missing region is not zero."});
    record(dir, &format!("memory-{name}.json"), &result)?;
    Ok(result)
}

fn surface_id(page: &page::Page) -> Option<u32> {
    let frame = page.context.frame()?;
    objc2_core_video::CVPixelBufferGetIOSurface(Some(&frame)).map(|surface| surface.id())
}

fn signature(page: &page::Page) -> Option<(usize, usize, u64)> {
    let frame = page.context.frame()?;
    frame_signature(&frame)
}

fn frame_signature(frame: &objc2_core_video::CVPixelBuffer) -> Option<(usize, usize, u64)> {
    use objc2_core_video::*;
    unsafe {
        let flags = CVPixelBufferLockFlags::ReadOnly;
        if CVPixelBufferLockBaseAddress(&frame, flags) != kCVReturnSuccess {
            return None;
        }
        let width = CVPixelBufferGetWidth(&frame);
        let height = CVPixelBufferGetHeight(&frame);
        let address = CVPixelBufferGetBaseAddress(&frame).cast::<u8>();
        let result = if address.is_null() {
            None
        } else {
            let bytes =
                std::slice::from_raw_parts(address, CVPixelBufferGetBytesPerRow(&frame) * height);
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hash);
            Some((width, height, hash.finish()))
        };
        CVPixelBufferUnlockBaseAddress(&frame, flags);
        result
    }
}

fn tick(window: &Window, count: Rc<Cell<u64>>, running: Rc<Cell<bool>>) {
    window.on_next_frame(move |window, _| {
        if running.get() {
            count.set(count.get() + 1);
            window.refresh();
            tick(window, count, running);
        }
    });
}

async fn ready(cards: &Entity<Cards>, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let started = Instant::now();
    loop {
        if cards.read_with(cx, |cards, cx| {
            cards.pages.iter().all(|p| {
                let p = p.read(cx);
                p.webview.load_status() == LoadStatus::Complete && p.delegate.frames.get() > 0
            })
        }) {
            break;
        }
        anyhow::ensure!(
            started.elapsed().as_secs() < 45,
            "measurement pages did not render"
        );
        pause(cx, 30).await;
    }
    Ok(())
}

fn add(
    cards: &Entity<Cards>,
    window: NativeWindow,
    url: &url::Url,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    window.update(cx, |_, w, cx| {
        let page = page::Page::open(url.clone(), w, cx)?;
        let board = cx.new(|cx| canvas::SpatialCanvas::new(page.clone(), cx));
        let viewport = w.viewport_size();
        let zoom = (f32::from(viewport.width) / 4. / 1376.)
            .min(f32::from(viewport.height) / 4. / 928.)
            .min(0.25);
        board.update(cx, |b, cx| b.zoom_to(zoom as f64, cx));
        cards.update(cx, |cards, cx| {
            cards.pages.push(page);
            cards.boards.push(board);
            cx.notify();
        });
        Ok::<_, anyhow::Error>(())
    })??;
    Ok(())
}

async fn measure(
    args: Args,
    cards: Entity<Cards>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    let conditions = std::process::Command::new("ps")
        .args(["-A", "-o", "comm=,pcpu="])
        .output()?;
    let compilers = String::from_utf8_lossy(&conditions.stdout)
        .lines()
        .filter(|line| line.contains("rustc") || line.contains("cargo"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let layout = window.update(cx, |_, w, _| json!({
        "viewport_points":[f32::from(w.viewport_size().width), f32::from(w.viewport_size().height)],
        "display_scale":w.scale_factor(),"grid_columns":4,"grid_rows":4
    }))?;
    let baseline_memory = memory_snapshot(&args.evidence, "baseline", cx).await?;
    let baseline = baseline_memory["rss_bytes"].as_u64().unwrap();
    let refresh = window.update(cx, |_, w, cx| {
        w.refresh_interval(cx).map(|d| 1. / d.as_secs_f64())
    })?;
    record(
        &args.evidence,
        "conditions.json",
        &json!({
            "display_refresh_hz":refresh,"layout":layout,"baseline_rss_bytes":baseline,"baseline_memory":baseline_memory,
            "concurrent_compilers_at_start":compilers,"window_kind":"Normal",
            "active_at_start":window.update(cx, |_, w, _| w.is_window_active())?,
            "criterion":"Every card and GPUI >=95% display refresh in each of three two-second samples",
            "frame_pacing":"Unmodified GPUI macOS display link; no timer or refresh override"
        }),
    )?;
    let mut rows = Vec::new();
    for n in 1..=16 {
        while cards.read_with(cx, |c, _| c.pages.len()) < n {
            add(&cards, window, &args.url, cx)?;
        }
        ready(&cards, cx).await?;
        pause(cx, 1000).await;
        let mut samples = Vec::new();
        for _ in 0..3 {
            anyhow::ensure!(
                window.update(cx, |_, w, _| w.is_visible() && w.is_window_active())?,
                "needs the foreground: measurement window inactive or occluded; frame-rate sample rejected"
            );
            let before = cards.read_with(cx, |c, cx| {
                c.pages
                    .iter()
                    .map(|p| p.read(cx).delegate.frames.get())
                    .collect::<Vec<_>>()
            });
            let frames = Rc::new(Cell::new(0));
            let active = Rc::new(Cell::new(true));
            window.update(cx, |_, w, _| {
                tick(w, frames.clone(), active.clone());
                w.refresh();
            })?;
            let start = Instant::now();
            pause(cx, 2000).await;
            let seconds = start.elapsed().as_secs_f64();
            active.set(false);
            anyhow::ensure!(
                window.update(cx, |_, w, _| w.is_visible() && w.is_window_active())?,
                "needs the foreground: measurement window inactive or occluded during sample"
            );
            let rates = cards.read_with(cx, |c, cx| {
                c.pages
                    .iter()
                    .zip(&before)
                    .map(|(p, before)| (p.read(cx).delegate.frames.get() - before) as f64 / seconds)
                    .collect::<Vec<_>>()
            });
            samples.push(json!({"seconds":seconds,"presented_fps_per_card":rates,"gpui_fps":frames.get() as f64 / seconds,"window_active":window.update(cx, |_, w, _|w.is_window_active())?}));
        }
        let rss = resident_bytes()?;
        let holds = refresh.map(|hz| {
            samples.iter().all(|s| {
                s["gpui_fps"].as_f64().unwrap_or(0.) >= hz * 0.95
                    && s["presented_fps_per_card"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|v| v.as_f64().unwrap_or(0.) >= hz * 0.95)
            })
        });
        rows.push(json!({"live_cards":n,"rss_bytes":rss,"rss_delta_per_card":(rss as i64-baseline as i64)/n as i64,"holds_display_rate":holds,"samples":samples}));
        record(&args.evidence, "capacity-progress.json", &json!(rows))?;
    }
    window.update(cx, |_, w, _| {
        w.render_to_image()?
            .save(args.evidence.join("live-16.png"))
            .map_err(anyhow::Error::from)
    })??;
    let live_memory = memory_snapshot(&args.evidence, "live", cx).await?;
    let live_rss = live_memory["rss_bytes"].as_u64().unwrap();
    let pages = cards.read_with(cx, |c, _| c.pages.clone());
    for p in &pages {
        let view = p.read_with(cx, |p, _| p.webview.clone());
        evaluate(&view, "(canvasBench.paused=true)", cx).await?;
    }
    pause(cx, 500).await;
    let before = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| signature(p)))
        .collect::<Vec<_>>();
    let surface_ids_before = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| surface_id(p)))
        .collect::<Vec<_>>();
    let presents_before = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| p.delegate.frames.get()))
        .collect::<Vec<_>>();
    cards.update(cx, |c, cx| {
        c.hidden = true;
        for p in &c.pages {
            p.update(cx, |p, _| p.set_visible(false));
        }
        cx.notify();
    });
    pause(cx, 2000).await;
    let hidden_memory = memory_snapshot(&args.evidence, "hidden", cx).await?;
    let hidden_rss = hidden_memory["rss_bytes"].as_u64().unwrap();
    let after = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| signature(p)))
        .collect::<Vec<_>>();
    let surface_ids_after = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| surface_id(p)))
        .collect::<Vec<_>>();
    let presents_after = pages
        .iter()
        .map(|p| p.read_with(cx, |p, _| p.delegate.frames.get()))
        .collect::<Vec<_>>();
    let frame_valid = surface_ids_before.iter().all(Option::is_some)
        && surface_ids_before == surface_ids_after
        && presents_before == presents_after
        && before
            .iter()
            .zip(&after)
            .all(|(a, b)| a.is_some() && a == b);
    let mut activation = Vec::new();
    cards.update(cx, |c, cx| {
        c.hidden = false;
        cx.notify();
    });
    pause(cx, 250).await;
    window.update(cx, |_, w, _| {
        w.render_to_image()?
            .save(args.evidence.join("hidden-retained-16.png"))
            .map_err(anyhow::Error::from)
    })??;
    for p in &pages {
        let (view, count) = p.read_with(cx, |p, _| (p.webview.clone(), p.delegate.frames.get()));
        let start = Instant::now();
        p.update(cx, |p, _| p.set_visible(true));
        evaluate(&view, "(canvasBench.paused=false)", cx).await?;
        while p.read_with(cx, |p, _| p.delegate.frames.get()) == count {
            anyhow::ensure!(
                start.elapsed().as_secs() < 10,
                "hidden page did not produce a new frame"
            );
            pause(cx, 5).await;
        }
        activation.push(start.elapsed().as_secs_f64() * 1000.);
        // The lifecycle never has more cards live than its budget, so each
        // card is timed with one page animating, not with all sixteen.
        evaluate(&view, "(canvasBench.paused=true)", cx).await?;
    }
    let weak = pages.iter().map(|p| p.downgrade()).collect::<Vec<_>>();
    drop(pages);
    cards.update(cx, |c, cx| {
        c.boards.clear();
        c.pages.clear();
        cx.notify();
    });
    pause(cx, 2000).await;
    let disposed_memory = memory_snapshot(&args.evidence, "disposed", cx).await?;
    let disposed_rss = disposed_memory["rss_bytes"].as_u64().unwrap();
    let released = weak.iter().all(|p| p.upgrade().is_none());
    let mut restored = Vec::new();
    for _ in 0..16 {
        let start = Instant::now();
        add(&cards, window, &args.url, cx)?;
        ready(&cards, cx).await?;
        restored.push(start.elapsed().as_secs_f64() * 1000.);
        let view = cards.read_with(cx, |c, cx| c.pages.last().unwrap().read(cx).webview.clone());
        evaluate(&view, "(canvasBench.paused=true)", cx).await?;
    }
    let capacity = refresh.map(|_| {
        rows.iter()
            .filter(|r| r["holds_display_rate"] == true)
            .filter_map(|r| r["live_cards"].as_u64())
            .max()
            .unwrap_or(0)
    });
    record(
        &args.evidence,
        "measurements.json",
        &json!({
            "concurrent_compilers_at_start":compilers,"display_refresh_hz":refresh,"live_card_capacity":capacity,"tested_maximum":16,"capacity_is_lower_bound":capacity==Some(16),
            "criterion":"Every card and GPUI >=95% display refresh in each of three two-second samples",
            "layout":layout,"workload":"Animated Canvas2D, 1280x800 CSS per card, 4x4 viewport grid at zoom <=0.25, production SpatialCanvas, Page and IOSurface renderer; one M1 board per tile",
            "memory_metric":"ps RSS and vmmap physical footprint/IOSurface regions, includes allocator retention; deltas from empty warmed engine",
            "memory":{"baseline":baseline_memory,"live":live_memory,"hidden":hidden_memory,"disposed":disposed_memory},
            "physical_footprint_bytes_per_card":{
                "live":(live_memory["physical_footprint_bytes"].as_i64().unwrap()-baseline_memory["physical_footprint_bytes"].as_i64().unwrap())/16,
                "hidden":(hidden_memory["physical_footprint_bytes"].as_i64().unwrap()-baseline_memory["physical_footprint_bytes"].as_i64().unwrap())/16,
                "disposed":(disposed_memory["physical_footprint_bytes"].as_i64().unwrap()-baseline_memory["physical_footprint_bytes"].as_i64().unwrap())/16
            },
            "hidden_copy_bytes":0,"hidden_surface_ids_before":surface_ids_before,"hidden_surface_ids_after":surface_ids_after,
            "hidden_presents_before":presents_before,"hidden_presents_after":presents_after,
            "baseline_rss_bytes":baseline,"live_rss_bytes":live_rss,"hidden_rss_bytes":hidden_rss,"disposed_rss_bytes":disposed_rss,
            "live_bytes_per_card":(live_rss as i64-baseline as i64)/16,
            "hidden_bytes_per_card":(hidden_rss as i64-baseline as i64)/16,
            "disposed_bytes_per_card":(disposed_rss as i64-baseline as i64)/16,
            "hidden_to_live_ms":activation,"disposed_restore_ms":restored,"all_entities_released":released,
            "lifecycle_load":"Each shown or restored card is paused again after its first frame, so one page animates while it is timed",
            "hidden_newest_frame_valid":frame_valid,"hidden_frame_signatures_before":before,"hidden_frame_signatures_after":after,"samples":rows
        }),
    )?;
    Ok(released && frame_valid)
}

pub fn start(args: Args, result: Rc<RefCell<Option<bool>>>, cx: &mut gpui_kit::App) {
    let mut cards = None;
    let window: NativeWindow = cx
        .open_window(
            WindowOptions {
                kind: gpui_kit::WindowKind::Normal,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(50.), px(50.)),
                    size(px(900.), px(650.)),
                ))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| Cards {
                    pages: Vec::new(),
                    boards: Vec::new(),
                    hidden: false,
                });
                cards = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("measurement window")
        .into();
    cx.activate(true);
    cx.spawn(async move |cx| {
        let evidence = args.evidence.clone();
        let checked = async {
            visible_window(window, &evidence, cx).await?;
            let guard = foreground::Guard::new(window, evidence.clone(), false, cx)?;
            let mut monitor_cx = cx.clone();
            let checked = {
                let work = measure(args, cards.unwrap(), window, cx).fuse();
                let monitor = guard.watch(window, &mut monitor_cx).fuse();
                futures::pin_mut!(work, monitor);
                futures::select_biased! { stopped = monitor => stopped, result = work => result }
            };
            guard.finish()?;
            checked
        }
        .await;
        write_outcome(&evidence, &checked);
        if let Err(error) = &checked {
            eprintln!("measurements: {error:#}");
        }
        *result.borrow_mut() = Some(matches!(checked, Ok(true)));
        cx.update(|cx| cx.quit());
    })
    .detach();
}

pub async fn diagnose_surface(
    dir: &std::path::Path,
    page: &Entity<page::Page>,
    window: NativeWindow,
    cx: &mut AsyncApp,
) -> anyhow::Result<bool> {
    let view = page.read_with(cx, |p, _| p.webview.clone());
    evaluate(&view, "(canvasBench.paused=true)", cx).await?;
    pause(cx, 500).await;
    let held = page
        .read_with(cx, |p, _| p.context.frame())
        .expect("live frame");
    let identity = |frame: &objc2_core_video::CVPixelBuffer| {
        objc2_core_video::CVPixelBufferGetIOSurface(Some(frame)).map(|surface| surface.id())
    };
    let before = json!({"surface":identity(&held),"signature":frame_signature(&held),"presents":page.read_with(cx, |p,_|p.delegate.frames.get())});
    window.update(cx, |_, w, _| {
        w.render_to_image()?
            .save(dir.join("live.png"))
            .map_err(anyhow::Error::from)
    })??;
    page.update(cx, |p, _| p.set_visible(false));
    pause(cx, 2000).await;
    anyhow::ensure!(
        window.update(cx, |_, w, _| w.is_visible() && w.is_window_active())?,
        "surface diagnosis window inactive or occluded"
    );
    let newest = page
        .read_with(cx, |p, _| p.context.frame())
        .expect("hidden frame");
    let after = json!({"surface":identity(&newest),"signature":frame_signature(&newest),"presents":page.read_with(cx, |p,_|p.delegate.frames.get()),"held_surface":identity(&held),"held_signature":frame_signature(&held)});
    let retained = before["surface"].is_number()
        && before["signature"].is_array()
        && before["surface"] == after["surface"]
        && before["signature"] == after["signature"]
        && before["signature"] == after["held_signature"]
        && before["presents"] == after["presents"];
    record(
        dir,
        "surface-diagnosis.json",
        &json!({"before":before,"after":after,"retained":retained,"hidden_copy_bytes":0}),
    )?;
    window.update(cx, |_, w, _| {
        w.render_to_image()?
            .save(dir.join("hidden.png"))
            .map_err(anyhow::Error::from)
    })??;
    if !retained {
        return Ok(false);
    }
    let count = page.read_with(cx, |p, _| p.delegate.frames.get());
    page.update(cx, |p, _| p.set_visible(true));
    evaluate(&view, "(canvasBench.paused=false)", cx).await?;
    let start = Instant::now();
    while page.read_with(cx, |p, _| p.delegate.frames.get()) == count {
        anyhow::ensure!(
            start.elapsed().as_secs() < 10,
            "shown page did not resume presenting"
        );
        pause(cx, 5).await;
    }
    record(
        dir,
        "surface-resume.json",
        &json!({"resumed":true,"elapsed_ms":start.elapsed().as_secs_f64()*1000.}),
    )?;
    Ok(true)
}
