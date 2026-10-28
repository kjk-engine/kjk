//! What a save from before a change needs beyond reading its old layout:
//! a value whose meaning changed, converted once the whole save is decoded,
//! so the rest of the sim sees only this build's (`bumps/README.md`).

use crate::save::Save;
use crate::wire::{DecodeError, Versions};

/// `save`, decoded from `versions` of the stored formats, made this
/// build's: the companions' templates kept in its party table
/// (`Npc::template_data`), written in its pack version, re-encoded in this
/// build's.
pub(crate) fn save(save: &mut Save, versions: Versions) -> Result<(), DecodeError> {
    let pack = versions.pack;
    if !(crate::pack::OLDEST..=crate::pack::VERSION).contains(&pack) {
        return Err(DecodeError::Version {
            format: "a save's templates",
            found: pack,
        });
    }
    for npc in save.script.party.available.values_mut() {
        let Some(bytes) = &npc.template_data else {
            continue;
        };
        let facts = crate::pack::creature_of(bytes, versions)?;
        if pack < crate::pack::VERSION {
            npc.template_data = Some(crate::pack::creature_bytes(&facts).into());
        }
    }
    Ok(())
}
