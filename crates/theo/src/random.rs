//! mulberry32, as web Theo draws: a face replayed from a seed must draw the
//! same numbers in both.

/// A seeded generator of numbers in [0, 1).
#[derive(Clone, Debug)]
pub struct Random {
    state: u32,
}

impl Random {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    pub fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    /// A number between two bounds.
    pub fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }

    /// The place of one of a list by weight; `None` for an empty list or no weight.
    pub fn weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().map(|weight| weight.max(0.0)).sum();
        // `!(total > 0)` also turns away a weight that is no number.
        if !(total > 0.0) {
            return None;
        }
        let mut at = self.next() * total;
        for (index, weight) in weights.iter().enumerate() {
            at -= weight.max(0.0);
            if at < 0.0 {
                return Some(index);
            }
        }
        Some(weights.len() - 1)
    }
}

/// What a JavaScript number becomes as an unsigned 32-bit seed (`>>> 0`).
pub fn to_u32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}

/// A seed from a string, so a name can stand for one (FNV-1a over UTF-16 units).
pub fn seed_of(text: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

/// `Math.round`: halves go up.
pub fn round(value: f64) -> f64 {
    (value + 0.5).floor()
}
