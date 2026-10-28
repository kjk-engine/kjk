//! The 2DAs readers look rows up in, each read and parsed once by name (a
//! scene's creatures parse `appearance.2da` once between them). Cells are
//! read with `TwoDa`'s typed readers (`text`, `int`, `float`, `flag`,
//! `hex`, `resref`).

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;

use crate::module::Source;

/// 2DAs by lowercase name; `None` for one that does not read.
#[derive(Debug, Default, Clone)]
pub struct Tables(BTreeMap<String, Option<Arc<TwoDa>>>);

impl Tables {
    /// `name`.2da as `source` resolves it, read the first time.
    pub fn get(&mut self, source: &dyn Source, name: &str) -> Option<Arc<TwoDa>> {
        self.get_with(name, |name| source.read(name, ResType::TWO_DA))
    }

    /// `name`.2da, read by `read` the first time.
    pub fn get_with(
        &mut self,
        name: &str,
        read: impl FnOnce(&str) -> Option<Vec<u8>>,
    ) -> Option<Arc<TwoDa>> {
        let name = name.to_ascii_lowercase();
        if let Some(known) = self.0.get(&name) {
            return known.clone();
        }
        let table = read(&name)
            .and_then(|bytes| TwoDa::parse(&bytes).ok())
            .map(Arc::new);
        self.0.insert(name, table.clone());
        table
    }

    /// Forgets every table (a module entered may override one).
    pub fn clear(&mut self) {
        self.0.clear();
    }
}
