//! What the import step made once per install (`docs/IMPORT.md`) for plans
//! to take instead of reading the install: every item and creature template
//! of the shared data (`sim::pack::Templates`) and every voiced line's
//! length. A plan then reads only what its module's own archives add.

use fmt_odyssey::ResType;
use std::collections::BTreeMap;

use asset_odyssey::module::Source;
use fmt_odyssey::gff::Gff;
use sim::pack::Templates;

use crate::sheets::{self, Tables};

/// The import's products a plan uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    pub templates: Templates,
    /// Each voiced line's length (ms), by lowercase resref.
    pub voices: BTreeMap<String, u32>,
}

/// Bump when what `templates` reads changes (with `sim::pack::VERSION`).
pub const TEMPLATES_FORMAT: &str = "templates-v16";
/// Bump when the voice lengths' encoding or reading changes.
pub const VOICES_FORMAT: &str = "voices-v1";

/// Every item and creature template `source` lists (the install's shared
/// data, read without a module).
pub fn templates(source: &dyn Source) -> Templates {
    let mut tables = Tables::new(source);
    let creatures = sheets::creature_catalog(&mut tables, std::iter::empty());
    let items = crate::items::catalog_of(&mut tables, source.names(ResType::UTI));
    let named_items = creatures
        .keys()
        .filter_map(|name| {
            let utc = Gff::parse(&source.read(name, ResType::UTC)?).ok()?;
            Some((name.clone(), crate::items::named(&utc.root)))
        })
        .collect();
    Templates {
        items,
        creatures,
        named_items,
    }
}

/// A voiced line's length (ms), from its file.
pub fn voice_ms(file: &[u8]) -> Option<u32> {
    fmt_odyssey::wav::Wav::parse(file).ok()?.duration_ms()
}

const VOICES_MAGIC: &[u8; 4] = b"KJKW";

pub fn encode_voices(voices: &BTreeMap<String, u32>) -> Vec<u8> {
    let mut out = VOICES_MAGIC.to_vec();
    out.extend((voices.len() as u32).to_le_bytes());
    for (name, ms) in voices {
        out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
        out.extend(name.as_bytes().iter().take(usize::from(u8::MAX)));
        out.extend(ms.to_le_bytes());
    }
    out
}

pub fn decode_voices(bytes: &[u8]) -> Option<BTreeMap<String, u32>> {
    let rest = bytes.strip_prefix(VOICES_MAGIC)?;
    let (count, mut rest) = rest.split_first_chunk::<4>()?;
    let mut voices = BTreeMap::new();
    for _ in 0..u32::from_le_bytes(*count) {
        let (&len, after) = rest.split_first()?;
        let (name, after) = after.split_at_checked(usize::from(len))?;
        let (ms, after) = after.split_first_chunk::<4>()?;
        voices.insert(
            String::from_utf8_lossy(name).into_owned(),
            u32::from_le_bytes(*ms),
        );
        rest = after;
    }
    rest.is_empty().then_some(voices)
}
