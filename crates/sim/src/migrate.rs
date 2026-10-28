//! What a save from before a change needs beyond reading its old layout:
//! a value whose meaning changed, converted once the whole save is decoded,
//! so the rest of the sim sees only this build's (`bumps/README.md`).

use crate::save::Save;
use crate::wire::DecodeError;

/// `save`, decoded from any version read, made this build's: the
/// companions' templates kept in its party table (`Npc::template_data`),
/// written in `pack` of that format, re-encoded in this build's.
pub(crate) fn save(save: &mut Save, pack: u32) -> Result<(), DecodeError> {
    if pack > crate::pack::VERSION {
        return Err(DecodeError::Version {
            format: "a kept companion",
            found: pack,
        });
    }
    for npc in save.script.party.available.values_mut() {
        let Some(bytes) = &npc.template_data else {
            continue;
        };
        let facts = crate::pack::creature_of(bytes, pack)?;
        if pack < crate::pack::VERSION {
            npc.template_data = Some(crate::pack::creature_bytes(&facts).into());
        }
    }
    Ok(())
}
