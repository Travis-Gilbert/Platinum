//! How loud what Platinum is playing is, measured where it leaves for the
//! speakers, and when it will be heard.
//!
//! Servo's media goes out through GStreamer: a `<video>` through playbin3's
//! default audio sink, Web Audio through `autoaudiosink`. Both take the
//! highest-ranked "Sink/Audio" element. `install` registers one ranked just
//! above macOS's own: a bin of `audioconvert ! level ! osxaudiosink`. The
//! sound goes out exactly as before; GStreamer's `level` element measures it
//! on the way, and each measurement is kept with the moment its sound will
//! reach the sink's clock, so the face moves with what is heard rather than
//! with what was decoded.
//!
//! What it does not know: the device's own output latency after the sink,
//! and which page a sound came from. Every pipeline made after `install`
//! goes through it, so the level is all of Platinum's sound.
//!
//! SOURCING: GStreamer's `level` element (gst-plugins-good) does the
//! measuring and `osxaudiosink` the playing; the gstreamer crate's subclass
//! API makes the bin. Ours is only the mapping of decibels to the face's
//! 0..1 and the short queue of measurements waiting to be heard.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;

/// How often `level` measures, and so how long one measurement stands for.
const INTERVAL: Duration = Duration::from_millis(50);
/// Quieter than this is silence on the face; 0 dBFS is full.
const FLOOR_DB: f64 = -60.0;
/// How many measurements wait to be heard: a few seconds of buffered sound.
const KEPT: usize = 128;

/// The level of a measurement: the loudest channel's RMS, from `FLOOR_DB`
/// (0) to full scale (1).
pub fn level_of(rms_db: &[f64]) -> f64 {
    let loudest = rms_db
        .iter()
        .copied()
        .filter(|db| db.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    if !loudest.is_finite() {
        return 0.0;
    }
    ((loudest - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Measurements waiting for their sound to be heard.
#[derive(Debug, Default)]
pub struct Levels {
    queue: VecDeque<(f64, Instant)>,
}

impl Levels {
    /// Keeps `level`, whose sound is heard at `heard`.
    pub fn push(&mut self, level: f64, heard: Instant) {
        if self.queue.len() == KEPT {
            self.queue.pop_front();
        }
        self.queue.push_back((level, heard));
    }

    /// The level being heard at `now` and when it began to be heard: the
    /// last measurement whose sound has arrived, while it still stands.
    /// None when nothing is being heard: silence, a pause, or no sound yet.
    pub fn at(&mut self, now: Instant) -> Option<(f64, Instant)> {
        while self.queue.len() > 1 && self.queue[1].1 <= now {
            self.queue.pop_front();
        }
        let &(level, heard) = self.queue.front()?;
        (heard <= now && now.duration_since(heard) <= INTERVAL * 2).then_some((level, heard))
    }
}

fn levels() -> &'static Mutex<Levels> {
    static LEVELS: OnceLock<Mutex<Levels>> = OnceLock::new();
    LEVELS.get_or_init(Mutex::default)
}

/// The level of Platinum's sound being heard now, and since when; None when
/// nothing is, or when the sink is not installed.
pub fn level_now() -> Option<(f64, Instant)> {
    levels().lock().ok()?.at(Instant::now())
}

/// Registers the measuring sink, once. Pipelines made afterwards take it.
pub fn install() {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let installed = (|| -> Result<(), String> {
            gst::init().map_err(|error| error.to_string())?;
            for needed in ["audioconvert", "level", "osxaudiosink"] {
                if gst::ElementFactory::find(needed).is_none() {
                    return Err(format!("GStreamer has no {needed}"));
                }
            }
            gst::Element::register(
                None,
                "platinumlevelsink",
                gst::Rank::PRIMARY + 1,
                LevelSink::static_type(),
            )
            .map_err(|error| error.to_string())
        })();
        match &installed {
            Ok(()) => log::info!("theo: Platinum's sound is measured on the way out"),
            Err(error) => log::warn!("theo: Platinum's sound is not measured: {error}"),
        }
        installed.is_ok()
    });
}

glib::wrapper! {
    pub struct LevelSink(ObjectSubclass<imp::LevelSink>) @extends gst::Bin, gst::Element, gst::Object;
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LevelSink {
        level: OnceLock<gst::Element>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LevelSink {
        const NAME: &'static str = "PlatinumLevelSink";
        type Type = super::LevelSink;
        type ParentType = gst::Bin;
    }

    impl ObjectImpl for LevelSink {
        fn constructed(&self) {
            self.parent_constructed();
            let bin = self.obj();
            let made = (|| -> Result<(), glib::BoolError> {
                let convert = gst::ElementFactory::make("audioconvert").build()?;
                let level = gst::ElementFactory::make("level")
                    .property("interval", INTERVAL.as_nanos() as u64)
                    .property("post-messages", true)
                    .build()?;
                let sink = gst::ElementFactory::make("osxaudiosink").build()?;
                bin.add_many([&convert, &level, &sink])?;
                gst::Element::link_many([&convert, &level, &sink])?;
                let target = convert
                    .static_pad("sink")
                    .ok_or_else(|| glib::bool_error!("audioconvert has no sink pad"))?;
                let templ = bin
                    .pad_template("sink")
                    .ok_or_else(|| glib::bool_error!("no sink template"))?;
                let pad = gst::GhostPad::builder_from_template_with_target(&templ, &target)?
                    .name("sink")
                    .build();
                bin.add_pad(&pad)?;
                let _ = self.level.set(level);
                Ok(())
            })();
            if let Err(error) = made {
                log::warn!("theo: the measuring sink could not be made: {error}");
            }
        }
    }

    impl GstObjectImpl for LevelSink {}

    impl ElementImpl for LevelSink {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static METADATA: OnceLock<gst::subclass::ElementMetadata> = OnceLock::new();
            Some(METADATA.get_or_init(|| {
                gst::subclass::ElementMetadata::new(
                    "Platinum measured audio sink",
                    "Sink/Audio",
                    "Plays to macOS's audio output and measures the level heard",
                    "Platinum",
                )
            }))
        }

        fn pad_templates() -> &'static [gst::PadTemplate] {
            static TEMPLATES: OnceLock<Vec<gst::PadTemplate>> = OnceLock::new();
            TEMPLATES.get_or_init(|| {
                let caps = gst::Caps::builder("audio/x-raw").build();
                vec![
                    gst::PadTemplate::new(
                        "sink",
                        gst::PadDirection::Sink,
                        gst::PadPresence::Always,
                        &caps,
                    )
                    .expect("a sink template"),
                ]
            })
        }
    }

    impl BinImpl for LevelSink {
        fn handle_message(&self, message: gst::Message) {
            let ours = self
                .level
                .get()
                .is_some_and(|level| message.src() == Some(level.upcast_ref()));
            if let (true, gst::MessageView::Element(element)) = (ours, message.view())
                && let Some(structure) = element.structure()
                && structure.name() == "level"
            {
                self.measured(structure);
                return;
            }
            self.parent_handle_message(message);
        }
    }

    impl LevelSink {
        /// One of `level`'s measurements: kept with the moment its sound
        /// reaches the clock, which is its running time less the clock's now.
        fn measured(&self, structure: &gst::StructureRef) {
            let Ok(rms) = structure.get::<glib::ValueArray>("rms") else {
                return;
            };
            let rms: Vec<f64> = rms.iter().filter_map(|v| v.get::<f64>().ok()).collect();
            let at = structure.get::<u64>("running-time").ok();
            let clock = self.obj().current_running_time().map(|t| t.nseconds());
            let ahead = match (at, clock) {
                (Some(at), Some(clock)) => Duration::from_nanos(at.saturating_sub(clock)),
                _ => Duration::ZERO,
            };
            if let Ok(mut levels) = levels().lock() {
                levels.push(level_of(&rms), Instant::now() + ahead);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_map_onto_the_face_s_range() {
        assert_eq!(level_of(&[0.0]), 1.0);
        assert_eq!(level_of(&[-60.0]), 0.0);
        assert_eq!(level_of(&[-90.0]), 0.0);
        assert!((level_of(&[-30.0]) - 0.5).abs() < 1e-9);
        // The loudest channel counts; silence (-inf) and nothing are 0.
        assert!((level_of(&[-45.0, -15.0]) - 0.75).abs() < 1e-9);
        assert_eq!(level_of(&[f64::NEG_INFINITY]), 0.0);
        assert_eq!(level_of(&[]), 0.0);
    }

    #[test]
    fn a_level_is_given_only_once_its_sound_is_heard() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.push(0.2, start + Duration::from_millis(100));
        levels.push(0.8, start + Duration::from_millis(150));
        assert_eq!(levels.at(start), None, "decoded, not yet heard");
        let heard = levels.at(start + Duration::from_millis(120));
        assert_eq!(heard.map(|(l, _)| l), Some(0.2));
        let heard = levels.at(start + Duration::from_millis(160));
        assert_eq!(heard.map(|(l, _)| l), Some(0.8));
    }

    #[test]
    fn a_level_goes_stale_when_the_sound_stops() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.push(0.5, start);
        assert!(levels.at(start + INTERVAL).is_some());
        assert_eq!(levels.at(start + INTERVAL * 3), None);
    }

    #[test]
    fn the_queue_is_bounded() {
        let start = Instant::now();
        let mut levels = Levels::default();
        for i in 0..(KEPT as u64 * 2) {
            levels.push(0.1, start + Duration::from_millis(i));
        }
        assert_eq!(levels.queue.len(), KEPT);
    }
}
