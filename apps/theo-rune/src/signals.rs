//! Measurements a capability offers to authored behavior, by namespaced
//! name: `composer.typing`, `theo.form`. Any capability can register a new
//! one without a change to Theo's engine; the engine's own fixed signals
//! (`theo::signals`) are fed through as well when a name is one of them.
//!
//! A name belongs to the owner that registered it, so two capabilities cannot
//! write each other's measurement, and a value nobody has refreshed within
//! its lifetime reads as absent, so a behavior never acts on a stale input.

use std::collections::BTreeMap;

/// Who registered a signal, and so who may set it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum SignalError {
    /// A name needs a namespace and a name, `capability.measure`.
    NotNamespaced(String),
    /// Another owner registered it first.
    Taken { name: String, owner: String },
    /// Set by an owner that did not register it, or never registered.
    NotYours(String),
}

#[derive(Clone, Debug)]
struct Entry {
    owner: Owner,
    /// How long a value stays true without being refreshed, in milliseconds.
    lifetime: f64,
    value: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
}

fn namespaced(name: &str) -> bool {
    let mut parts = name.split('.');
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    };
    matches!((parts.next(), parts.next()), (Some(a), Some(b)) if valid(a) && valid(b))
        && parts.all(valid)
}

impl Registry {
    /// Registers `name` for `owner`. Registering again by the same owner is
    /// a no-op that keeps the value, so a capability may repeat it on restart.
    pub fn register(
        &mut self,
        name: &str,
        owner: &Owner,
        lifetime_ms: f64,
    ) -> Result<(), SignalError> {
        if !namespaced(name) {
            return Err(SignalError::NotNamespaced(name.to_owned()));
        }
        match self.entries.get(name) {
            Some(entry) if entry.owner != *owner => Err(SignalError::Taken {
                name: name.to_owned(),
                owner: entry.owner.0.clone(),
            }),
            Some(_) => Ok(()),
            None => {
                let entry = Entry {
                    owner: owner.clone(),
                    lifetime: lifetime_ms,
                    value: None,
                };
                self.entries.insert(name.to_owned(), entry);
                Ok(())
            }
        }
    }

    /// Takes a measurement at `now`.
    pub fn set(
        &mut self,
        name: &str,
        owner: &Owner,
        value: f64,
        now: f64,
    ) -> Result<(), SignalError> {
        match self.entries.get_mut(name) {
            Some(entry) if entry.owner == *owner => {
                entry.value = Some((value, now));
                Ok(())
            }
            _ => Err(SignalError::NotYours(name.to_owned())),
        }
    }

    /// The current value, or `None` when unregistered, never set, or stale.
    pub fn read(&self, name: &str, now: f64) -> Option<f64> {
        let entry = self.entries.get(name)?;
        let (value, at) = entry.value?;
        (now - at <= entry.lifetime).then_some(value)
    }

    /// Every value that is current at `now`, as behaviors read them.
    pub fn current(&self, now: f64) -> BTreeMap<String, f64> {
        self.entries
            .keys()
            .filter_map(|name| self.read(name, now).map(|v| (name.clone(), v)))
            .collect()
    }

    /// Every registered name, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composer() -> Owner {
        Owner("composer".into())
    }

    #[test]
    fn a_new_capability_registers_a_measurement_and_a_behavior_reads_it() {
        let mut signals = Registry::default();
        signals
            .register("composer.typing", &composer(), 1_000.0)
            .unwrap();
        assert_eq!(
            signals.read("composer.typing", 0.0),
            None,
            "nothing measured yet"
        );
        signals
            .set("composer.typing", &composer(), 4.5, 100.0)
            .unwrap();
        assert_eq!(signals.read("composer.typing", 200.0), Some(4.5));
        assert_eq!(signals.names().collect::<Vec<_>>(), ["composer.typing"]);
    }

    #[test]
    fn a_value_nobody_refreshes_goes_stale() {
        let mut signals = Registry::default();
        signals
            .register("composer.typing", &composer(), 1_000.0)
            .unwrap();
        signals
            .set("composer.typing", &composer(), 4.5, 100.0)
            .unwrap();
        assert_eq!(signals.read("composer.typing", 1_100.0), Some(4.5));
        assert_eq!(signals.read("composer.typing", 1_101.0), None);
    }

    #[test]
    fn names_belong_to_the_owner_that_registered_them() {
        let mut signals = Registry::default();
        let other = Owner("capture".into());
        signals
            .register("composer.typing", &composer(), 1_000.0)
            .unwrap();
        signals
            .register("composer.typing", &composer(), 1_000.0)
            .unwrap();
        assert_eq!(
            signals.register("composer.typing", &other, 1_000.0),
            Err(SignalError::Taken {
                name: "composer.typing".into(),
                owner: "composer".into()
            })
        );
        assert_eq!(
            signals.set("composer.typing", &other, 1.0, 0.0),
            Err(SignalError::NotYours("composer.typing".into()))
        );
        assert_eq!(
            signals.set("capture.level", &other, 1.0, 0.0),
            Err(SignalError::NotYours("capture.level".into())),
            "an unregistered name cannot be set"
        );
    }

    #[test]
    fn a_name_needs_a_namespace() {
        let mut signals = Registry::default();
        for bad in [
            "typing",
            ".typing",
            "composer.",
            "Composer.typing",
            "composer typing",
        ] {
            assert_eq!(
                signals.register(bad, &composer(), 1.0),
                Err(SignalError::NotNamespaced(bad.into())),
                "{bad}"
            );
        }
        signals
            .register("capture.level.left", &composer(), 1.0)
            .unwrap();
    }
}
