//! A local convenience implementation of figment::Provider
// (c) 2025 Ross Younger

use figment::{Error, Metadata, Profile, Provider, value::Dict, value::Map, value::Value};

#[derive(Debug, Clone)]
pub(crate) struct LocalConfigSource {
    source: String,
    data: Dict,
}

impl LocalConfigSource {
    pub(crate) fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            data: Dict::new(),
        }
    }

    pub(crate) fn add(&mut self, key: &str, val: Value) {
        let _ = self.data.insert(key.into(), val);
    }

    pub(crate) fn borrow(&mut self) -> &mut Dict {
        &mut self.data
    }
}

impl Provider for LocalConfigSource {
    fn metadata(&self) -> Metadata {
        Metadata::named(self.source.clone())
    }
    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        let mut profile_map = Map::new();
        let _ = profile_map.insert(Profile::Global, self.data.clone());
        Ok(profile_map)
    }
}
