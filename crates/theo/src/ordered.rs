//! A map that is walked in the order its keys were first set, as a
//! JavaScript `Map` is: which feature comes first decides which grains it
//! owns, so the order is part of the state.
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Ordered<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> Ordered<V> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&at| &self.entries[at].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        match self.index.get(key) {
            Some(&at) => Some(&mut self.entries[at].1),
            None => None,
        }
    }

    /// Sets a key. A key already there keeps its place.
    pub fn set(&mut self, key: &str, value: V) {
        match self.index.get(key) {
            Some(&at) => self.entries[at].1 = value,
            None => {
                self.index.insert(key.to_owned(), self.entries.len());
                self.entries.push((key.to_owned(), value));
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        let at = self.index.remove(key)?;
        let (_, value) = self.entries.remove(at);
        for (later, (name, _)) in self.entries.iter().enumerate().skip(at) {
            self.index.insert(name.clone(), later);
        }
        Some(value)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(key, _)| key.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, value)| value)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, value)| value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut V)> {
        self.entries
            .iter_mut()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }
}
