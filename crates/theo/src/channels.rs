//! The live set of channels: the fixed ones, and the params of whichever
//! features the face has now. Features come and go, so this is built, not
//! declared.
use crate::ordered::Ordered;
use crate::tables::{ChannelSpec, FeatureDef, PARAM_COUNT, param_name, tables};

/// The most features a face holds at once; matter is finite.
pub const MAX_FEATURES: usize = 8;

/// `value` inside two bounds, as the web clamps: a value that is no number passes through.
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn is_part(part: &str) -> bool {
    let bytes = part.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 16
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// A feature id: a short lowercase path such as `eye.left` or `brow.left`.
pub fn is_feature_id(id: &str) -> bool {
    match id.split_once('.') {
        Some((head, tail)) => is_part(head) && is_part(tail),
        None => is_part(id),
    }
}

#[derive(Clone, Debug)]
pub struct Channels {
    specs: Ordered<ChannelSpec>,
    defs: Ordered<FeatureDef>,
}

impl Channels {
    pub fn new(features: &[FeatureDef]) -> Self {
        let mut channels = Self {
            specs: Ordered::new(),
            defs: Ordered::new(),
        };
        for (name, spec) in &tables().face_channels {
            channels.specs.set(name, spec.clone());
        }
        for feature in features {
            // The identity's own features are well formed; a refusal here would be the data's fault.
            let _ = channels.add_feature(feature);
        }
        channels
    }

    /// Adds a feature's channels. Refuses a bad id, a reserved one, or one too many; a taken id is left as it is.
    pub fn add_feature(&mut self, def: &FeatureDef) -> Result<(), String> {
        let id = def.id.as_str();
        if !is_feature_id(id) {
            return Err(format!("\"{id}\" is not a feature id."));
        }
        if ["face", "gaze", "matter", "auto", "emit"]
            .iter()
            .any(|reserved| id.starts_with(reserved))
            || id == "eyes"
        {
            return Err(format!("\"{id}\" is a reserved name."));
        }
        if self.defs.has(id) {
            return Ok(());
        }
        if self.defs.len() >= MAX_FEATURES {
            return Err(format!(
                "The face holds at most {MAX_FEATURES} features; remove one first."
            ));
        }
        self.defs.set(id, def.clone());
        for index in 0..PARAM_COUNT {
            let base = &tables().feature_params[index];
            let rest = def
                .rest
                .iter()
                .find(|(param, _)| *param == index)
                .map_or(base.rest, |(_, rest)| clamp(*rest, base.min, base.max));
            self.specs.set(
                &format!("{id}.{}", param_name(index)),
                ChannelSpec {
                    rest,
                    ..base.clone()
                },
            );
        }
        Ok(())
    }

    pub fn remove_feature(&mut self, id: &str) -> bool {
        if self.defs.remove(id).is_none() {
            return false;
        }
        for index in 0..PARAM_COUNT {
            self.specs.remove(&format!("{id}.{}", param_name(index)));
        }
        true
    }

    pub fn features(&self) -> impl Iterator<Item = &FeatureDef> {
        self.defs.values()
    }

    pub fn feature_count(&self) -> usize {
        self.defs.len()
    }

    pub fn feature(&self, id: &str) -> Option<&FeatureDef> {
        self.defs.get(id)
    }

    pub fn spec(&self, name: &str) -> Option<&ChannelSpec> {
        self.specs.get(name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.specs.has(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.specs.keys()
    }

    /// The channels a written name means. `eyes.w` is both eyes' `w`; any other name is itself. An unknown name gives nothing.
    pub fn expand(&self, name: &str) -> Vec<String> {
        if let Some(param) = name.strip_prefix("eyes.") {
            let both = vec![format!("eye.left.{param}"), format!("eye.right.{param}")];
            return if both.iter().all(|channel| self.specs.has(channel)) {
                both
            } else {
                Vec::new()
            };
        }
        if self.specs.has(name) {
            vec![name.to_owned()]
        } else {
            Vec::new()
        }
    }

    /// The known name closest to a wrong one, for an error a model can act on.
    pub fn nearest(&self, name: &str) -> Option<String> {
        let wrong: Vec<u16> = name.encode_utf16().collect();
        let mut best: Option<String> = None;
        let mut best_score = usize::MAX;
        let eyes = (0..PARAM_COUNT).map(|index| format!("eyes.{}", param_name(index)));
        for candidate in self.specs.keys().map(str::to_owned).chain(eyes) {
            let score = distance(&wrong, &candidate.encode_utf16().collect::<Vec<_>>());
            if score < best_score {
                best_score = score;
                best = Some(candidate);
            }
        }
        if best_score as f64 <= (wrong.len() as f64 / 2.0).max(3.0) {
            best
        } else {
            None
        }
    }
}

/// Edit distance, small strings only.
fn distance(a: &[u16], b: &[u16]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut previous = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let above = row[j];
            row[j] = (above + 1)
                .min(row[j - 1] + 1)
                .min(previous + usize::from(a[i - 1] != b[j - 1]));
            previous = above;
        }
    }
    row[b.len()]
}
