//! The probes of rows by label in the sim (`rules::rows`, `script::rows`;
//! `kjk sim probes --only "rows by label"`), on the proving ground with
//! made-up tables, no install.
//!
//! The made-up install is a KOTOR 1 install with a mod's rows: `spells.2da`
//! row 132 is the mod's Force Body (canonical 134) and row 133 a power of
//! the mod's own (moved to 282). `feat.2da` row 125 is the mod's Dual Strike
//! (canonical 190) and row 126 a feat of the mod's own (moved to 245).
//!
//! - A creature's script of the mod (install numbers) and a lent script
//!   (canonical numbers) each ask whether the creature knows the power and
//!   the feat, grant the mod's own power and feat, and read a talent's row
//!   back. Each script gets its own numbers, and the sheet gets canonical
//!   ones.
//! - A Jedi casts the canonical Force Body. Its impact script gets the
//!   install row from `GetSpellId` when it is the mod's script, and the
//!   canonical number when it is a lent script.
//! - A sheet that knows the moved power and feat survives a snapshot.

use std::collections::BTreeSet;

use rules::rows::{InstallRow, RowMap, RowMaps, Table};
use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;

use super::SimWorld;
use crate::combat::Sheet;
use crate::combat::sheet::{Rows, factions, flags};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::{Menu, Talent, Talents};
use crate::snapshot::Kind;
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

/// Where the Jedi stands and the foe it casts at (metres, the ground's own
/// axes).
const JEDI_AT: (f32, f32) = (-7.0, 0.0);
const FOE_AT: (f32, f32) = (-4.5, 0.0);

/// The mod's rows of each table: its row of a named label, and its own row.
const SPELLS: (u16, u16) = (132, 133);
const FEATS: (u16, u16) = (125, 126);
/// The labels of those rows.
const SPELL_LABELS: [&str; 2] = ["FORCE_POWER_FORCE_BODY", "FORCE_POWER_A_MODS_OWN"];
const FEAT_LABELS: [&str; 2] = ["DUAL_STRIKE", "A_MODS_OWN_FEAT"];

const CATALOG: &str = r#"
int TALENT_TYPE_FORCE = 0;
int TALENT_TYPE_FEAT = 1;
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
int GetSpellAcquired(int nSpell, object oCreature=OBJECT_SELF);
// 3
int GetHasFeat(int nFeat, object oCreature=OBJECT_SELF);
// 4
void GrantSpell(int nSpell, object oCreature);
// 5
void GrantFeat(int nFeat, object oCreature);
// 6
talent TalentSpell(int nSpell);
// 7
talent TalentFeat(int nFeat);
// 8
int GetIdFromTalent(talent tTalent);
// 9
int GetSpellId();
"#;

/// The made-up install's map of `table`: KOTOR 1's own rows, each named
/// row under its label, then the mod's two rows.
fn map(table: Table, labels: [&str; 2]) -> RowMap {
    let own: Vec<String> = (0..table.kotor1_rows())
        .map(|row| {
            table
                .named()
                .iter()
                .find(|(_, number)| *number == row)
                .map_or_else(|| format!("OWN_ROW_{row}"), |(label, _)| label.to_string())
        })
        .collect();
    let rows: Vec<InstallRow> = own
        .iter()
        .map(String::as_str)
        .chain(labels)
        .map(|label| InstallRow {
            label: Some(label),
            lent: None,
        })
        .collect();
    RowMap::build(table, false, &rows).0
}

fn maps() -> RowMaps {
    RowMaps {
        spells: map(Table::Spells, SPELL_LABELS),
        feats: map(Table::Feats, FEAT_LABELS),
    }
}

/// The canonical numbers of the mod's rows of `table`.
fn canonical(maps: &RowMaps, table: Table, (named, own): (u16, u16)) -> Option<(u16, u16)> {
    let map = maps.get(table);
    Some((map.canonical(named)?, map.canonical(own)?))
}

/// One script's rows (its own numbering) and the first custom token it
/// writes.
#[derive(Clone, Copy)]
struct Asks {
    spells: (u16, u16),
    feats: (u16, u16),
    token: i32,
}

/// The script: tokens `token` to `token + 5` get whether the creature
/// knows the named power and feat, then (after granting them) the own
/// power and feat, and the rows of a talent made of the named power and
/// feat.
fn asking(asks: Asks) -> Result<fmt_odyssey::ncs::Ncs, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let int = |row: u16| Expr::Int(i32::from(row));
    let token = |n: i32, value: Expr| {
        call(
            "SetCustomToken",
            [Expr::Int(asks.token + n), call("IntToString", [value])],
        )
    };
    let id = |kind: &str, row: u16| call("GetIdFromTalent", [call(kind, [int(row)])]);
    script(
        &catalog,
        &[
            token(0, call("GetSpellAcquired", [int(asks.spells.0), Expr::Me])),
            token(1, call("GetHasFeat", [int(asks.feats.0), Expr::Me])),
            call("GrantSpell", [int(asks.spells.1), Expr::Me]),
            call("GrantFeat", [int(asks.feats.1), Expr::Me]),
            token(2, call("GetSpellAcquired", [int(asks.spells.1), Expr::Me])),
            token(3, call("GetHasFeat", [int(asks.feats.1), Expr::Me])),
            token(4, id("TalentSpell", asks.spells.0)),
            token(5, id("TalentFeat", asks.feats.0)),
        ],
    )
}

/// The impact script: token 20 gets `GetSpellId`.
fn impact() -> Result<fmt_odyssey::ncs::Ncs, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    script(
        &catalog,
        &[call(
            "SetCustomToken",
            [Expr::Int(20), call("IntToString", [call("GetSpellId", [])])],
        )],
    )
}

/// The scripts' names: the mod's speak install numbers, the lent one
/// canonical numbers.
const MOD_SCRIPT: &str = "k_mod_rows";
const LENT_SCRIPT: &str = "k2_rows";
const IMPACT: &str = "k_mod_body";

/// A Jedi who knows the canonical Force Body and Dual Strike.
fn knowing(maps: &RowMaps) -> Option<Sheet> {
    let (spell, _) = canonical(maps, Table::Spells, SPELLS)?;
    let (feat, _) = canonical(maps, Table::Feats, FEATS)?;
    let mut sheet = Sheet::dev_jedi();
    sheet.powers = Rows::default().with(spell);
    sheet.feats = Rows::default().with(feat);
    sheet.flags |= flags::TSL | flags::IMMORTAL;
    Some(sheet)
}

/// A world on the proving ground with the made-up maps, two creatures
/// whose heartbeats run the mod's script and the lent script, and a player
/// Jedi whose Force Body runs `IMPACT`. `impact_lent` makes `IMPACT` a
/// lent script.
fn world(impact_lent: bool) -> Result<(SimWorld, RowMaps), String> {
    let maps = maps();
    let sheet = knowing(&maps).ok_or("the made-up map has no number for a mod's row")?;
    let spells = canonical(&maps, Table::Spells, SPELLS).ok_or("no number for a mod's power")?;
    let feats = canonical(&maps, Table::Feats, FEATS).ok_or("no number for a mod's feat")?;
    let body = spells.0;
    let canonical_asks = Asks {
        spells,
        feats,
        token: 10,
    };
    let install_asks = Asks {
        spells: SPELLS,
        feats: FEATS,
        token: 0,
    };
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    // KOTOR 2's rules cast a power only its impact script carries out
    // (`force::scripted`), as `power_rows_probes` does.
    data.game = "tsl".into();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    let size = usize::from(body) + 1;
    data.force_costs = vec![0; size];
    data.force_costs[usize::from(body)] = 10;
    for (n, name) in [MOD_SCRIPT, LENT_SCRIPT].into_iter().enumerate() {
        data.placements.push(Placement::Own(ObjectData {
            sheet: Some(Sheet {
                faction: factions::FRIENDLY_1,
                ..sheet
            }),
            template: format!("asker{n}"),
            scripts: vec![("ScriptHeartbeat".into(), name.into())],
            ..ObjectData::bare(
                ObjectKind::Creature,
                &format!("asker{n}"),
                crate::determinism::ground_point(4.0, 3.0 * n as f32, 0.0),
            )
        }));
    }
    // A foe the Jedi casts at, as `power_rows_probes` casts a power only
    // its impact script carries out.
    data.placements.push(Placement::Own(ObjectData {
        sheet: Some(Sheet {
            faction: factions::HOSTILE_1,
            ..Sheet::dev_soldier()
        }),
        template: "foe".into(),
        ..ObjectData::bare(
            ObjectKind::Creature,
            "foe",
            crate::determinism::ground_point(FOE_AT.0, FOE_AT.1, 0.0),
        )
    }));
    let mut spells = vec![Talent::default(); size];
    spells[usize::from(body)] = Talent {
        menu: Menu::Hostile,
        hostile: true,
        reach_dm: 100,
        maxcr: 3,
        cast_ms: 500,
        conjure_ms: 100,
        ..Talent::default()
    };
    let mut impacts = vec![String::new(); size];
    impacts[usize::from(body)] = IMPACT.into();
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let scripts = [
        (MOD_SCRIPT, asking(install_asks)?),
        (LENT_SCRIPT, asking(canonical_asks)?),
        (IMPACT, impact()?),
    ];
    let mut lent: BTreeSet<String> = [LENT_SCRIPT.to_string()].into();
    if impact_lent {
        lent.insert(IMPACT.into());
    }
    let library =
        crate::probe::library_with(CATALOG, scripts, talents).with_rows(maps.clone(), lent);
    let level = ground.with_module(data).with_scripts(library);
    let config = WorldConfig {
        sheet,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
    let jedi = world.snapshot().body(Seat(0)).ok_or("no player")?.id;
    let at = crate::script::standing(crate::determinism::ground_point(JEDI_AT.0, JEDI_AT.1, 0.0));
    let mut bodies = world.world.query::<(&super::Id, &mut super::Player)>();
    for (id, mut body) in bodies.iter_mut(&mut world.world) {
        if id.0 == jedi {
            body.0.origin[0] = at[0];
            body.0.origin[1] = at[1];
        }
    }
    Ok((world, maps))
}

fn step(world: &mut SimWorld, command: Command) {
    let mut input = TickInput::default();
    input.insert(Seat(0), command);
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority);
}

/// What a run of `ms` saw: every custom token, and each asker's powers
/// and feats.
struct Seen {
    tokens: std::collections::BTreeMap<i32, String>,
    sheets: Vec<(Vec<u16>, Vec<u16>)>,
}

/// Runs the world for `ms` after the player presses Force Body once.
fn run(impact_lent: bool, ms: u32) -> Result<(Seen, RowMaps), String> {
    let (mut world, maps) = world(impact_lent)?;
    let (body, _) = canonical(&maps, Table::Spells, SPELLS).ok_or("no Force Body")?;
    let press = Command {
        force: body,
        buttons: buttons::FORCE_POWER,
        ..Command::default()
    };
    // One step to settle, one press, then the press let go.
    step(&mut world, Command::default());
    step(&mut world, press);
    for _ in 0..ms / MSEC {
        step(&mut world, press.idle());
    }
    let snapshot = world.snapshot();
    let mut askers: Vec<_> = snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .filter(|entity| {
            entity
                .sheet
                .as_ref()
                .is_some_and(|sheet| sheet.faction == factions::FRIENDLY_1)
        })
        .collect();
    askers.sort_by_key(|entity| entity.id);
    let sheets = askers
        .iter()
        .filter_map(|entity| entity.sheet.as_ref())
        .map(|sheet| (sheet.powers.rows().collect(), sheet.feats.rows().collect()))
        .collect();
    let tokens = snapshot
        .talk
        .tokens
        .iter()
        .map(|(k, v)| (i32::from(*k), v.clone()))
        .collect();
    Ok((Seen { tokens, sheets }, maps))
}

/// Each script got its own numbers back, and the sheets hold canonical
/// numbers.
fn dialects() -> Result<String, String> {
    let (seen, maps) = run(false, 14_000)?;
    let (body, own_power) = canonical(&maps, Table::Spells, SPELLS).ok_or("no number")?;
    let (body_feat, own_feat) = canonical(&maps, Table::Feats, FEATS).ok_or("no number")?;
    let wanted = |base: i32, ids: (u16, u16)| {
        let mut tokens: Vec<(i32, String)> = (0..4).map(|n| (base + n, "1".to_string())).collect();
        tokens.push((base + 4, ids.0.to_string()));
        tokens.push((base + 5, ids.1.to_string()));
        tokens
    };
    let missing: Vec<(i32, String)> = wanted(0, (SPELLS.0, FEATS.0))
        .into_iter()
        .chain(wanted(10, (body, body_feat)))
        .filter(|(token, value)| seen.tokens.get(token) != Some(value))
        .collect();
    let granted = seen.sheets.len() == 2
        && seen.sheets.iter().all(|(powers, feats)| {
            powers.contains(&own_power)
                && feats.contains(&own_feat)
                && !powers.contains(&SPELLS.1)
                && !feats.contains(&FEATS.1)
        });
    check(
        missing.is_empty() && granted,
        format!(
            "tokens {:?}; wrong {missing:?}; askers' powers and feats {:?} (the mod's own power \
             is {own_power}, its own feat {own_feat})",
            seen.tokens, seen.sheets
        ),
    )
}

/// The Jedi's Force Body runs `IMPACT`: `GetSpellId` gives the mod's
/// script the install row, and a lent script the canonical number.
fn spell_id(impact_lent: bool) -> Result<String, String> {
    let (seen, maps) = run(impact_lent, 3000)?;
    let wanted = match impact_lent {
        true => maps.spells.canonical(SPELLS.0),
        false => Some(SPELLS.0),
    };
    let got = seen.tokens.get(&20);
    check(
        wanted.is_some() && got == wanted.map(|row| row.to_string()).as_ref(),
        format!("GetSpellId gave {got:?}, wanted {wanted:?}"),
    )
}

/// The made-up maps: the named rows at their constants, the mod's own rows
/// past KOTOR 2's tables, and every other row at its own number.
fn made_up_maps() -> Result<String, String> {
    let maps = maps();
    let spells = canonical(&maps, Table::Spells, SPELLS);
    let feats = canonical(&maps, Table::Feats, FEATS);
    let kept = (0..Table::Spells.kotor1_rows()).all(|row| maps.spells.canonical(row) == Some(row))
        && (0..Table::Feats.kotor1_rows()).all(|row| maps.feats.canonical(row) == Some(row));
    let back = maps
        .spells
        .install(crate::combat::spells::FORCE_POWER_FORCE_BODY)
        == Some(SPELLS.0);
    check(
        spells
            == Some((
                crate::combat::spells::FORCE_POWER_FORCE_BODY,
                Table::Spells.kotor2_rows(),
            ))
            && feats
                == Some((
                    crate::combat::sheet::feats::DUAL_STRIKE[0],
                    Table::Feats.kotor2_rows(),
                ))
            && kept
            && back,
        format!("spells {spells:?}, feats {feats:?}, own rows kept {kept}, Force Body back {back}"),
    )
}

/// A sheet that knows the moved power and feat comes back whole from this
/// build's snapshot, by their canonical numbers.
fn snapshot_round_trip() -> Result<String, String> {
    let maps = maps();
    let mut sheet = knowing(&maps).ok_or("no numbers")?;
    let (_, power) = canonical(&maps, Table::Spells, SPELLS).ok_or("no number")?;
    let (_, feat) = canonical(&maps, Table::Feats, FEATS).ok_or("no number")?;
    sheet.powers.insert(power);
    sheet.feats.insert(feat);
    let versions = crate::wire::Versions::THIS_BUILD;
    let mut out = crate::wire::Writer::default();
    sheet.encode_as(&mut out, versions);
    sheet.picks.encode(&mut out);
    sheet.bar.encode(&mut out);
    let mut input = crate::wire::Reader::new("probe", &out.0).with_versions(versions);
    let read = crate::snapshot::decode_sheet(&mut input).map_err(|e| format!("{e:?}"))?;
    input.finish().map_err(|e| format!("{e:?}"))?;
    check(
        read == sheet,
        format!(
            "powers {:?} and feats {:?} read back as {:?} and {:?}",
            sheet.powers.rows().collect::<Vec<_>>(),
            sheet.feats.rows().collect::<Vec<_>>(),
            read.powers.rows().collect::<Vec<_>>(),
            read.feats.rows().collect::<Vec<_>>()
        ),
    )
}

pub fn probes() -> Checks {
    let mut checks: Checks = vec![
        ("rows_by_label_made_up_maps", made_up_maps()),
        ("rows_by_label_script_dialects", dialects()),
    ];
    let ids: [(&'static str, bool); 2] = [
        ("rows_by_label_spell_id_of_a_mods_script", false),
        ("rows_by_label_spell_id_of_a_lent_script", true),
    ];
    checks.extend(ids.map(|(name, lent)| (name, spell_id(lent))));
    checks.push(("rows_by_label_snapshot_round_trip", snapshot_round_trip()));
    checks
}
