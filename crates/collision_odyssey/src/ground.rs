//! What a floor looks like underfoot ([`collision::Ground`]), from the
//! texture drawn on it and the walkmesh material under it. KOTOR's
//! `surfacemat.2da` has no sand, snow or gravel: Tatooine's dunes and
//! Korriban's valleys are marked dirt, the Unknown World's wet sand mud, and
//! Telos's polar snow carpet. The texture tells them apart
//! (`kjk collision build` lists each module's floor textures by material).

use collision::Ground;

/// `surfacemat.2da`'s rows.
const DIRT: u32 = 1;
const GRASS: u32 = 3;
const STONE: u32 = 4;
const WOOD: u32 = 5;
const WATER: u32 = 6;
const CARPET: u32 = 9;
const METAL: u32 = 10;
const PUDDLES: u32 = 11;
const SWAMP: u32 = 12;
const MUD: u32 = 13;
const LEAVES: u32 = 14;
const DEEP_WATER: u32 = 17;
const NON_WALK_GRASS: u32 = 19;

/// Words in a texture's name that say its ground, whatever its material
/// (Anchorhead's deck plates and grates are marked dirt, Manaan's sea floor
/// dirt, Nar Shaddaa's plates carpet).
const NAMED: [(&str, Ground); 9] = [
    ("snw", Ground::Snow),
    ("snow", Ground::Snow),
    ("sand", Ground::Sand),
    ("mud", Ground::Mud),
    ("gravel", Ground::Gravel),
    ("water", Ground::Water),
    ("metal", Ground::Metal),
    ("_met", Ground::Metal),
    ("grate", Ground::Metal),
];

/// Planets' textures whose dirt is sand: Tatooine's (`lta_`), Korriban's
/// in K1 (`lko_`, the tombs' `m36_`) and TSL (`kor_`). The Valley of the
/// Dark Lords' gravel floor (`lrk_flr03`, both games) steps as dirt.
const SANDY: [&str; 4] = ["lta_", "lko_", "m36_", "kor_"];

/// The ground of a floor of `material` drawn with `texture` (lowercase; ""
/// for none). A planet's texture prefix alone never makes a floor metal:
/// Taris's streets are marked metal already, and its Undercity's dirt,
/// grass and stone (`lts_un…`) step as KOTOR steps them.
pub fn of(material: u32, texture: &str) -> Ground {
    if let Some(&(_, ground)) = NAMED.iter().find(|(word, _)| texture.contains(word)) {
        return ground;
    }
    if material == DIRT && SANDY.iter().any(|prefix| texture.starts_with(prefix)) {
        return Ground::Sand;
    }
    match material {
        DIRT => Ground::Dirt,
        GRASS | LEAVES | NON_WALK_GRASS => Ground::Grass,
        STONE => Ground::Stone,
        WOOD => Ground::Wood,
        WATER | PUDDLES | SWAMP | DEEP_WATER => Ground::Water,
        CARPET => Ground::Rug,
        METAL => Ground::Metal,
        MUD => Ground::Mud,
        _ => Ground::Unknown,
    }
}
