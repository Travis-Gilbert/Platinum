//! Opt-in measurements, compiled out of ordinary builds.
use std::{collections::BTreeMap, sync::Mutex, time::Instant};
static SAMPLES: Mutex<BTreeMap<&'static str, Vec<f64>>> = Mutex::new(BTreeMap::new());
static LAST_FRAME: Mutex<BTreeMap<&'static str, Instant>> = Mutex::new(BTreeMap::new());
pub struct Span(&'static str, Instant);
impl Span {
    pub fn new(name: &'static str) -> Self {
        let now = Instant::now();
        let interval = match name {
            "servo_frame" => Some("servo_frame_interval"),
            "page_render" => Some("page_render_interval"),
            "surface_present" => Some("surface_present_interval"),
            _ => None,
        };
        if let Some(interval) = interval
            && let Some(last) = LAST_FRAME.lock().unwrap().insert(name, now)
        {
            let mut samples = SAMPLES.lock().unwrap();
            let values = samples.entry(interval).or_default();
            if values.len() < 100_000 {
                values.push(now.duration_since(last).as_secs_f64() * 1000.);
            }
        }
        Self(name, now)
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        let mut samples = SAMPLES.lock().unwrap();
        let values = samples.entry(self.0).or_default();
        if values.len() < 100_000 {
            values.push(self.1.elapsed().as_secs_f64() * 1000.);
        }
    }
}
pub fn reset() {
    SAMPLES.lock().unwrap().clear();
    LAST_FRAME.lock().unwrap().clear();
}
pub fn report() -> serde_json::Value {
    let samples = SAMPLES.lock().unwrap();
    let values: serde_json::Map<_, _> = samples
        .iter()
        .map(|(name, values)| {
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            let n = sorted.len();
            (
                (*name).into(),
                serde_json::json!({"count":n,"total_ms":sorted.iter().sum::<f64>(),
            "p50_ms":sorted[n/2], "p95_ms":sorted[(n-1)*95/100],"max_ms":sorted[n-1]}),
            )
        })
        .collect();
    values.into()
}
