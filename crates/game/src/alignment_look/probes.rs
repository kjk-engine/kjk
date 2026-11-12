//! A character's look follows its alignment (issue 330). The steps are the
//! executables' (`aligned`), and with the installs each game's own
//! textures are read: the player's dark faces, bodies and portraits, KOTOR
//! 2's companions' dark faces and its `alttexture` heads, and a whole
//! creature placed dark wears its dark face.

use asset_odyssey::creature::{self, Aligned};
use asset_odyssey::tables::Tables;

use super::aligned;
use crate::probe_kit::{Checks, check, not_installed};
use crate::viewer::{CreatureLoad, ResolverSource, resolver};

/// KOTOR's `P_MAL_C_SML_04` (the reporter's player: head `PMHC04`), and
/// its plain clothes.
const K1_PLAYER: u32 = 175;
const CLOTHES: &str = "g_a_clothes01";
/// KOTOR 2's `P_MAL_A_SML_01` (head `PMHA01`) and `Party_NPC_Atton`.
const TSL_PLAYER: u32 = 136;
const ATTON: u32 = 452;
/// KOTOR 2's `heads.2da` row of a Twi'lek woman whose texture is its
/// `alttexture` (`twilek_f01`).
const TWILEK: u8 = 61;
/// `portraits.2da` rows: KOTOR's `po_pmhc4`, KOTOR 2's `po_pmha01`.
const K1_PORTRAIT: usize = 26;
const TSL_PORTRAIT: usize = 18;

pub fn probes() -> Checks {
    vec![
        ("dark_steps", steps()),
        ("k1_dark_player", k1_player()),
        ("tsl_dark_looks", tsl_looks()),
        ("dark_creature_placed", placed()),
    ]
}

/// The steps by alignment: none above 40, then one more at each of 40,
/// 30, 20 and 10. The bare body darkens at 30 in KOTOR and at 40 in
/// KOTOR 2, and only KOTOR 2 reads `alttexture`.
fn steps() -> Result<String, String> {
    let at = |tsl, good_evil| aligned(tsl, good_evil);
    let steps: Vec<u8> = [100, 41, 40, 31, 30, 21, 20, 11, 10, 0]
        .into_iter()
        .map(|good_evil| at(false, good_evil).step)
        .collect();
    let bodies = [
        at(false, 31).evil_body,
        at(false, 30).evil_body,
        at(true, 41).evil_body,
        at(true, 40).evil_body,
    ];
    check(
        steps == [0, 0, 1, 1, 2, 2, 3, 3, 4, 4]
            && bodies == [false, true, false, true]
            && !at(false, 50).alt_texture
            && at(true, 50).alt_texture,
        format!("steps {steps:?}, dark bodies at 31, 30 (KOTOR) and 41, 40 (KOTOR 2) {bodies:?}"),
    )
}

/// A look of `appearance` wearing `armour`, dressed for `aligned`: its
/// head's texture, its body's texture.
fn dressed(
    source: &ResolverSource,
    (appearance, armour): (u32, Option<&str>),
    aligned: Aligned,
) -> Result<(Option<String>, Option<String>), String> {
    let mut tables = Tables::default();
    let mut look =
        creature::resolve_appearance(source, appearance, armour.map(str::to_string), &mut tables)
            .map_err(|refusal| format!("appearance {appearance}: {refusal:?}"))?;
    creature::align(source, &mut look, aligned, &mut tables);
    Ok((look.head_texture, look.body_texture))
}

/// KOTOR: the reporter's player, dressed, keeps its face at 41 and takes
/// `PMHC04D1` at 40 and `PMHC04D` at 5, its clothes unchanged; bare at 30
/// its body is `PMBASD01`, at 31 its own. Its portrait goes from `po_pmhc4`
/// to `po_pmhc4D` at 5.
fn k1_player() -> Result<String, String> {
    let discovery = crate::discover();
    let game = vfs::Game::Kotor1;
    if discovery.get(game).is_none() {
        return Ok(not_installed(game));
    }
    let resolver = resolver(&discovery, game, None)?;
    let source = ResolverSource(&resolver);
    let clothed = (K1_PLAYER, Some(CLOTHES));
    let light = dressed(&source, clothed, aligned(false, 41))?;
    let first = dressed(&source, clothed, aligned(false, 40))?;
    let darkest = dressed(&source, clothed, aligned(false, 5))?;
    let bare = dressed(&source, (K1_PLAYER, None), aligned(false, 30))?;
    let bare_light = dressed(&source, (K1_PLAYER, None), aligned(false, 31))?;
    let portraits = resolver.two_da("portraits").ok_or("no portraits.2da")?;
    let portrait = |step| creature::portrait_at(&portraits, K1_PORTRAIT, step);
    check(
        light.0.is_none()
            && first.0.as_deref() == Some("pmhc04d1")
            && darkest.0.as_deref() == Some("pmhc04d")
            && darkest.1 == light.1
            && bare.1.as_deref() == Some("pmbasd01")
            && bare_light.1.as_deref() != Some("pmbasd01")
            && portrait(0).as_deref() == Some("po_pmhc4")
            && portrait(4).as_deref() == Some("po_pmhc4d"),
        format!(
            "clothed at 41 {light:?}, 40 {first:?}, 5 {darkest:?}; bare at 30 {bare:?}, \
             31 {bare_light:?}; portraits {:?} and {:?}",
            portrait(0),
            portrait(4)
        ),
    )
}

/// KOTOR 2: its player's face is its `alttexture` (`PMHA01`) at 50 and
/// `PMHA01D2` at 5, its bare body `PMBAMD01` at 40; Atton's face is
/// `P_AttnH1D1` at 35; a Twi'lek's head wears its `alttexture` whatever
/// its alignment; the player's portrait is `po_pmha01d2` at 5.
fn tsl_looks() -> Result<String, String> {
    let discovery = crate::discover();
    let game = vfs::Game::Kotor2;
    if discovery.get(game).is_none() {
        return Ok(not_installed(game));
    }
    let resolver = resolver(&discovery, game, None)?;
    let source = ResolverSource(&resolver);
    let bare = (TSL_PLAYER, None);
    let neutral = dressed(&source, bare, aligned(true, 50))?;
    let darkest = dressed(&source, bare, aligned(true, 5))?;
    let dark_body = dressed(&source, bare, aligned(true, 40))?;
    let atton = dressed(&source, (ATTON, None), aligned(true, 35))?;
    let mut tables = Tables::default();
    let mut twilek = creature::resolve_appearance(&source, TSL_PLAYER, None, &mut tables)
        .map_err(|refusal| format!("{refusal:?}"))?;
    twilek.head_row = Some(usize::from(TWILEK));
    creature::align(&source, &mut twilek, aligned(true, 0), &mut tables);
    let portraits = resolver.two_da("portraits").ok_or("no portraits.2da")?;
    let portrait = creature::portrait_at(&portraits, TSL_PORTRAIT, aligned(true, 5).step);
    check(
        neutral.0.as_deref() == Some("pmha01")
            && darkest.0.as_deref() == Some("pmha01d2")
            && dark_body.1.as_deref() == Some("pmbamd01")
            && neutral.1.as_deref() != Some("pmbamd01")
            && atton.0.as_deref() == Some("p_attnh1d1")
            && twilek.head_texture.as_deref() == Some("twilek_f01")
            && portrait.as_deref() == Some("po_pmha01d2"),
        format!(
            "player at 50 {neutral:?}, 5 {darkest:?}, 40 {dark_body:?}; Atton at 35 {atton:?}; \
             Twi'lek {:?}; portrait at 5 {portrait:?}",
            twilek.head_texture
        ),
    )
}

/// A creature placed dark (`CreatureLoad::place_appearance`, as a body
/// dressed again is placed) wears its dark face on every textured mesh of
/// its head, and its view says what it was dressed for.
fn placed() -> Result<String, String> {
    let discovery = crate::discover();
    let game = vfs::Game::Kotor1;
    if discovery.get(game).is_none() {
        return Ok(not_installed(game));
    }
    let resolver = resolver(&discovery, game, None)?;
    let dark = aligned(false, 5);
    let mut models = Vec::new();
    let view = CreatureLoad::default()
        .place_appearance(
            (&resolver, game),
            (K1_PLAYER, Some(CLOTHES), None, None),
            dark,
            ([0.0; 3], 0.0),
            &mut models,
        )?
        .ok_or("not placed")?;
    let head: Vec<&String> = models
        .iter()
        .skip(1)
        .take(1)
        .flat_map(|placed| &placed.ir.meshes)
        .filter_map(|mesh| mesh.texture.as_ref())
        .collect();
    check(
        view.aligned == Some(dark)
            && !head.is_empty()
            && head.iter().all(|texture| *texture == "pmhc04d"),
        format!("dressed for {:?}, head textures {head:?}", view.aligned),
    )
}
