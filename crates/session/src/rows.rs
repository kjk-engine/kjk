//! The rows-by-label maps of the tables a `Source` serves (`rules::rows`).
//!
//! The resolver serves `spells.2da` and `feat.2da` in canonical order when
//! a mod's rows make the map other than the identity (`vfs::rows`), with
//! each row's install number in `rules::rows::INSTALL_COLUMN`. The maps are
//! read back from that column here. Whatever reads install numbers through
//! the source (a creature template's feats and powers, an item's Cast Spell
//! property, a script's routine) translates them with these maps.

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;
use rules::rows::{INSTALL_COLUMN, RowMap, RowMaps, Table};

/// The maps `source`'s tables are served by; the identity for a table
/// served in its install order.
pub fn row_maps(source: &dyn Source) -> RowMaps {
    let mut maps = RowMaps::default();
    for table in Table::ALL {
        let parsed = source
            .read(table.name(), ResType::TWO_DA)
            .and_then(|bytes| TwoDa::parse(&bytes).ok());
        let Some(parsed) = parsed.filter(|parsed| parsed.column(INSTALL_COLUMN).is_some()) else {
            continue;
        };
        let install = (0..parsed.cells.len())
            .map(|number| u16::try_from(parsed.int(number, INSTALL_COLUMN)?).ok())
            .collect();
        *maps.get_mut(table) = RowMap::from_install_rows(install);
    }
    maps
}
