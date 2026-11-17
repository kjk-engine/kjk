//! Rows by label (`rules::rows`, `vfs::rows`, `kjk sim probes --only "rows
//! by label"`): the sim's checks with made-up tables
//! (`sim::rows_by_label_probes`), then checks on both games' data where
//! this machine has them.
//!
//! The made-up mod on KOTOR 1's data appends rows to `spells.2da` the way
//! a patcher does: a copy of KOTOR 2's Master Heal with a script of the
//! mod's own, Force Scream twice (the second by mistake, as the mod "K2
//! Force Powers for K1" labels two rows alike), and a power of the mod's
//! own. It appends KOTOR 2's Dual Strike and a feat of the mod's own to
//! `feat.2da`. KOTOR 2's rows are then lent.

use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};
use fmt_odyssey::tlk::LENT_FLAG;
use fmt_odyssey::twoda::TwoDa;
use rules::rows::Table;
use sim::probe::{Checks, check};
use vfs::resolver::{Layer, Location, Rank, Resolver};

use crate::kotor2_powers_probes::{Case, Data, run};
use crate::modules::ResolverSource;

/// The checks on the installs, by name.
const CASES: [(&str, Case); 9] = [
    ("the identity without mods", identity),
    ("named rows at their numbers", named),
    ("rows lent after a mod's rows", lent_after),
    ("prerequisites", prerequisites),
    ("a template's install rows", template),
    ("the action bar's icon", bar_icon),
    ("the same map on every machine", deterministic),
    ("lent scripts", lent_scripts),
    ("a mod's own feat", own_feat),
];

pub fn probes() -> Checks {
    let mut checks = sim::rows_by_label_probes::probes();
    checks.extend(run(&CASES));
    checks
}

/// A row the made-up mod appends: KOTOR 2's row `from` with `label` and
/// the cells `cells` (column, value) of its own.
struct Appended {
    from: &'static str,
    label: &'static str,
    cells: &'static [(&'static str, &'static str)],
}

const SPELL_ROWS: [Appended; 4] = [
    Appended {
        from: "FORCE_POWER_MASTER_HEAL",
        label: "FORCE_POWER_MASTER_HEAL",
        cells: &[("impactscript", "k_fp_masterheal"), ("iconresref", "ip_modheal")],
    },
    Appended {
        from: "FORCE_POWER_FORCE_SCREAM",
        label: "FORCE_POWER_FORCE_SCREAM",
        cells: &[("impactscript", "k_fp_scream")],
    },
    Appended {
        from: "FORCE_POWER_FORCE_SCREAM",
        label: "FORCE_POWER_FORCE_SCREAM",
        cells: &[("impactscript", "k_fp_scream2")],
    },
    Appended {
        from: "FORCE_POWER_MASTER_HEAL",
        label: "FORCE_POWER_A_MODS_OWN",
        cells: &[("impactscript", "k_fp_own")],
    },
];

const FEAT_ROWS: [Appended; 2] = [
    Appended {
        from: "DUAL_STRIKE",
        label: "DUAL_STRIKE",
        cells: &[("icon", "ip_moddual")],
    },
    Appended {
        from: "DUAL_STRIKE",
        label: "A_MODS_OWN_FEAT",
        cells: &[],
    },
];

/// The columns whose cells are talk table lines and icons: a mod brings
/// its own, which KOTOR 1's data has.
const OWN_CELLS: [&str; 5] = ["name", "description", "spelldesc", "icon", "iconresref"];

/// The made-up mod's tables: each table of KOTOR 1's (without the loan)
/// with `rows` appended, copied from KOTOR 2's table under KOTOR 1's
/// columns. A row's name, description and icon are KOTOR 1's first named
/// row's, as a mod brings lines and icons that KOTOR 1's data then has.
fn modded_table(data: &Data, table: Table, rows: &[Appended]) -> Result<TwoDa, String> {
    let mut parsed = data.plain.two_da(table.name()).ok_or("no table")?;
    let named = (0..parsed.cells.len())
        .find(|&row| parsed.int(row, "name").is_some_and(|strref| strref > 0))
        .ok_or("no row with a name")?;
    let first = parsed.cells[named].clone();
    let tsl = data.tsl.two_da(table.name()).ok_or("no KOTOR 2 table")?;
    for row in rows {
        let from = tsl.find("label", row.from).ok_or(format!("KOTOR 2 has no {}", row.from))?;
        let cells = parsed
            .columns
            .iter()
            .enumerate()
            .map(|(at, column)| {
                let name = String::from_utf8_lossy(column).to_ascii_lowercase();
                let own = row.cells.iter().find(|(cell, _)| *cell == name);
                match (name.as_str(), own) {
                    ("label", _) => row.label.as_bytes().to_vec(),
                    (_, Some((_, value))) => value.as_bytes().to_vec(),
                    (name, None) if OWN_CELLS.contains(&name) => first[at].clone(),
                    _ => tsl.text(from, &name).unwrap_or_default().into_bytes(),
                }
            })
            .collect();
        parsed.row_labels.push(parsed.cells.len().to_string().into_bytes());
        parsed.cells.push(cells);
    }
    Ok(parsed)
}

/// `resolver` with `files` in an override layer of their own, its rows
/// layer made again over them.
fn with_override(resolver: &Resolver, files: Vec<(String, ResType, Vec<u8>)>) -> Resolver {
    let resources = files
        .into_iter()
        .map(|(name, restype, bytes)| {
            let bytes: Arc<[u8]> = bytes.into();
            let location = Location {
                path: format!("override/{name}.{}", restype.0).into(),
                offset: 0,
                size: bytes.len() as u64,
                held: Some(bytes),
            };
            ((name, restype), location)
        })
        .collect();
    resolver.with_layer(Layer {
        name: "override".into(),
        rank: Rank::Directory,
        resources,
    })
}

/// KOTOR 1's data with the made-up mod, KOTOR 2's rows lent to it, and the
/// install rows of the mod's rows of each table.
struct Modded {
    resolver: Resolver,
    spells: Vec<u16>,
    feats: Vec<u16>,
}

fn modded(data: &Data) -> Result<Modded, String> {
    let spells = modded_table(data, Table::Spells, &SPELL_ROWS)?;
    let feats = modded_table(data, Table::Feats, &FEAT_ROWS)?;
    let appended = |table: &TwoDa, count: usize| -> Vec<u16> {
        (table.cells.len() - count..table.cells.len())
            .filter_map(|row| u16::try_from(row).ok())
            .collect()
    };
    let (spell_rows, feat_rows) = (
        appended(&spells, SPELL_ROWS.len()),
        appended(&feats, FEAT_ROWS.len()),
    );
    let mut resolver = with_override(
        &data.plain,
        vec![
            ("spells".into(), ResType::TWO_DA, spells.write()),
            ("feat".into(), ResType::TWO_DA, feats.write()),
        ],
    );
    resolver.lend(&data.tsl)?;
    Ok(Modded {
        resolver,
        spells: spell_rows,
        feats: feat_rows,
    })
}

/// The canonical number of `resolver`'s install row `row` of `table`.
fn canonical(resolver: &Resolver, table: Table, row: u16) -> Option<u16> {
    resolver.rows.get(table).canonical(row)
}

/// KOTOR 1 without the loan, KOTOR 1 with it, KOTOR 2, and KOTOR 2 with a
/// mod's row after its own in each table each have the identity, and the
/// mod's KOTOR 2 rows keep their numbers.
fn identity(data: &Data) -> Result<String, String> {
    let own = |table: Table| Appended {
        from: if table == Table::Spells { "FORCE_POWER_FORCE_SCREAM" } else { "DUAL_STRIKE" },
        label: "A_MODS_OWN",
        cells: &[],
    };
    let mut tsl_files = Vec::new();
    for table in Table::ALL {
        let mut parsed = data.tsl.two_da(table.name()).ok_or("no KOTOR 2 table")?;
        let from = parsed.find("label", own(table).from).ok_or("no row to copy")?;
        let mut row = parsed.cells[from].clone();
        if let Some(column) = parsed.column("label") {
            row[column] = own(table).label.as_bytes().to_vec();
        }
        parsed.row_labels.push(parsed.cells.len().to_string().into_bytes());
        parsed.cells.push(row);
        tsl_files.push((table.name().to_string(), ResType::TWO_DA, parsed.write()));
    }
    let tsl_modded = with_override(&data.tsl, tsl_files);
    let resolvers = [
        ("KOTOR 1", &data.plain),
        ("KOTOR 1 lent to", &data.lent),
        ("KOTOR 2", &data.tsl),
        ("KOTOR 2 with a mod's rows after its own", &tsl_modded),
    ];
    let not: Vec<&str> = resolvers
        .iter()
        .filter(|(_, resolver)| !resolver.rows.is_identity())
        .map(|(name, _)| *name)
        .collect();
    let served = Table::ALL.iter().all(|table| {
        tsl_modded
            .two_da(table.name())
            .and_then(|parsed| parsed.text(usize::from(table.kotor2_rows()), "label"))
            .as_deref()
            == Some("A_MODS_OWN")
    });
    check(
        not.is_empty() && served,
        format!("maps other than the identity: {not:?}; KOTOR 2's mod rows served at their own numbers {served}"),
    )
}

/// The mod's Master Heal, first Force Scream and Dual Strike are served at
/// the engine's numbers with the mod's own cells; the second Force Scream
/// and the mod's own power and feat are served past KOTOR 2's own rows.
fn named(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let resolver = &modded.resolver;
    let spells = resolver.two_da("spells").ok_or("no spells.2da")?;
    let feats = resolver.two_da("feat").ok_or("no feat.2da")?;
    let numbers: Vec<Option<u16>> = modded
        .spells
        .iter()
        .map(|&row| canonical(resolver, Table::Spells, row))
        .collect();
    let feat_numbers: Vec<Option<u16>> = modded
        .feats
        .iter()
        .map(|&row| canonical(resolver, Table::Feats, row))
        .collect();
    let (spell_past, feat_past) = (Table::Spells.kotor2_rows(), Table::Feats.kotor2_rows());
    let cell = |table: &TwoDa, number: Option<u16>, column: &str| {
        number.and_then(|number| table.text(usize::from(number), column))
    };
    let wanted = [
        (cell(&spells, numbers[0], "impactscript"), "k_fp_masterheal"),
        (cell(&spells, numbers[1], "impactscript"), "k_fp_scream"),
        (cell(&spells, numbers[2], "impactscript"), "k_fp_scream2"),
        (cell(&spells, numbers[3], "label"), "FORCE_POWER_A_MODS_OWN"),
        (cell(&feats, feat_numbers[0], "icon"), "ip_moddual"),
        (cell(&feats, feat_numbers[1], "label"), "A_MODS_OWN_FEAT"),
    ];
    let cells_right = wanted.iter().all(|(got, want)| got.as_deref() == Some(*want));
    use sim::combat::spells::{FORCE_POWER_FORCE_SCREAM, FORCE_POWER_MASTER_HEAL};
    let numbers_right = numbers[0] == Some(FORCE_POWER_MASTER_HEAL)
        && numbers[1] == Some(FORCE_POWER_FORCE_SCREAM)
        && numbers[2..].iter().all(|n| n.is_some_and(|n| n >= spell_past))
        && feat_numbers[0] == Some(sim::combat::sheet::feats::DUAL_STRIKE[0])
        && feat_numbers[1].is_some_and(|n| n >= feat_past);
    let max = sim::combat::sheet::Powers::ROWS;
    let under = spells.cells.len() <= usize::from(max) && feats.cells.len() <= usize::from(max);
    check(
        cells_right && numbers_right && under,
        format!(
            "spells {:?} at {numbers:?}, feats {:?} at {feat_numbers:?}; cells {:?}; {} and {} rows served",
            modded.spells,
            modded.feats,
            wanted.iter().map(|(got, _)| got.clone()).collect::<Vec<_>>(),
            spells.cells.len(),
            feats.cells.len()
        ),
    )
}

/// KOTOR 2's rows are lent after the mod's rows, each at its KOTOR 2
/// number as its canonical number, except the rows whose labels the mod
/// already has (Master Heal, Force Scream, Dual Strike).
fn lent_after(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let lent = modded.resolver.lent.as_ref().ok_or("nothing lent")?;
    let plain = data.lent.lent.as_ref().ok_or("nothing lent without the mod")?;
    use sim::combat::spells::{FORCE_POWER_FORCE_SCREAM, FORCE_POWER_MASTER_HEAL};
    let skipped_spells = [FORCE_POWER_MASTER_HEAL, FORCE_POWER_FORCE_SCREAM];
    let skipped_feats = [sim::combat::sheet::feats::DUAL_STRIKE[0]];
    let wanted = |all: Vec<u16>, skipped: &[u16]| -> Vec<u16> {
        all.into_iter().filter(|row| !skipped.contains(row)).collect()
    };
    let spell_numbers: Vec<u16> = lent.spell_numbers.values().copied().collect();
    let feat_numbers: Vec<u16> = lent.feat_numbers.values().copied().collect();
    let after = |installed: &[u16], placed: Vec<u16>| {
        let last = installed.iter().max().copied().unwrap_or(0);
        placed.first().is_some_and(|&first| first > last)
    };
    let canonical_right = Table::ALL.iter().all(|&table| {
        let numbers = match table {
            Table::Spells => &lent.spell_numbers,
            Table::Feats => &lent.feat_numbers,
        };
        numbers.iter().all(|(&at, &number)| {
            u16::try_from(at).ok().and_then(|at| canonical(&modded.resolver, table, at)) == Some(number)
        })
    });
    let ok = spell_numbers == wanted(plain.rows(), &skipped_spells)
        && feat_numbers == wanted(plain.feat_rows(), &skipped_feats)
        && after(&modded.spells, lent.rows())
        && after(&modded.feats, lent.feat_rows())
        && canonical_right
        && lent.refused.is_empty();
    check(
        ok,
        format!(
            "powers' KOTOR 2 numbers {spell_numbers:?} at {:?}; feats' {feat_numbers:?} at {:?}; \
             canonical numbers right {canonical_right}; refused {:?}",
            lent.rows(),
            lent.feat_rows(),
            lent.refused
        ),
    )
}

/// The columns whose cells hold rows, by table.
const PREREQUISITES: [(Table, &[&str]); 2] = [
    (Table::Spells, &["prerequisites"]),
    (Table::Feats, &["prereqfeat1", "prereqfeat2", "successor"]),
];

/// Every lent row's prerequisites, served as canonical numbers, name the
/// rows of the labels KOTOR 2's own cells name. Improved Force Scream
/// needs the mod's Force Scream, and Improved Dual Strike the mod's Dual
/// Strike.
fn prerequisites(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let lent = modded.resolver.lent.as_ref().ok_or("nothing lent")?;
    let mut wrong = Vec::new();
    let mut seen = 0;
    for (table, columns) in PREREQUISITES {
        let served = modded.resolver.two_da(table.name()).ok_or("no table")?;
        let tsl = data.tsl.two_da(table.name()).ok_or("no KOTOR 2 table")?;
        let numbers = match table {
            Table::Spells => &lent.spell_numbers,
            Table::Feats => &lent.feat_numbers,
        };
        for &number in numbers.values() {
            let number = usize::from(number);
            for column in columns {
                let labels = |parsed: &TwoDa, row: usize| -> Vec<Option<String>> {
                    parsed
                        .text(row, column)
                        .unwrap_or_default()
                        .split('_')
                        .filter_map(|n| n.trim().parse::<usize>().ok())
                        .map(|n| parsed.text(n, "label"))
                        .collect()
                };
                let (here, there) = (labels(&served, number), labels(&tsl, number));
                seen += there.len();
                if here != there {
                    wrong.push(format!("{} {number} {column}: {here:?}, KOTOR 2 {there:?}", table.name()));
                }
            }
        }
    }
    // The two the mod's rows answer.
    let spells = modded.resolver.two_da("spells").ok_or("no spells.2da")?;
    let feats = modded.resolver.two_da("feat").ok_or("no feat.2da")?;
    use sim::combat::spells::{FORCE_POWER_FORCE_SCREAM, FORCE_POWER_IMPROVED_FORCE_SCREAM};
    let dual = sim::combat::sheet::feats::DUAL_STRIKE;
    let scream = spells.text(usize::from(FORCE_POWER_IMPROVED_FORCE_SCREAM), "prerequisites");
    let dual_needs = feats.text(usize::from(dual[1]), "prereqfeat1");
    let mods = scream.as_deref() == Some(&FORCE_POWER_FORCE_SCREAM.to_string()[..])
        && spells.text(usize::from(FORCE_POWER_FORCE_SCREAM), "impactscript").as_deref() == Some("k_fp_scream")
        && dual_needs.as_deref() == Some(&dual[0].to_string()[..])
        && feats.text(usize::from(dual[0]), "icon").as_deref() == Some("ip_moddual");
    check(
        wrong.is_empty() && seen > 0 && mods,
        format!(
            "{seen} prerequisites checked; wrong {wrong:?}; Improved Force Scream needs {scream:?}, \
             Improved Dual Strike {dual_needs:?}, both the mod's rows {mods}"
        ),
    )
}

/// A creature template naming the mod's rows by their install numbers
/// gives a sheet that knows them by their canonical numbers.
fn template(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let name = modded
        .resolver
        .names(ResType::UTC)
        .into_iter()
        .find(|name| {
            modded
                .resolver
                .read(name, ResType::UTC)
                .and_then(|bytes| Gff::parse(&bytes).ok())
                .is_some_and(|gff| !gff.root.list("ClassList").is_empty())
        })
        .ok_or("no creature template with a class")?;
    let mut gff = Gff::parse(&modded.resolver.read(&name, ResType::UTC).ok_or("no template")?)
        .map_err(|e| e.to_string())?;
    let word = |label: &str, value: u16| GffField {
        label: label.as_bytes().to_vec(),
        value: GffValue::Word(value),
    };
    let entries = |label: &str, rows: &[u16]| -> Vec<GffStruct> {
        rows.iter()
            .map(|&row| GffStruct {
                id: 3,
                fields: vec![word(label, row)],
            })
            .collect()
    };
    let powers = [modded.spells[0], modded.spells[3]];
    let feats = [modded.feats[0], modded.feats[1]];
    set_list(&mut gff.root, "FeatList", entries("Feat", &feats));
    for field in &mut gff.root.fields {
        if field.label == b"ClassList"
            && let GffValue::List(classes) = &mut field.value
            && let Some(class) = classes.first_mut()
        {
            set_list(class, "KnownList0", entries("Spell", &powers));
        }
    }
    let resolver = with_override(&modded.resolver, vec![(name.clone(), ResType::UTC, gff.write())]);
    let facts = session::creature_facts(&ResolverSource(&resolver), &name).ok_or("the template does not read")?;
    let sheet = facts.sheet.ok_or("no sheet")?;
    let want = |table: Table, rows: &[u16]| -> Option<Vec<u16>> {
        rows.iter().map(|&row| canonical(&modded.resolver, table, row)).collect()
    };
    let (want_powers, want_feats) = (want(Table::Spells, &powers), want(Table::Feats, &feats));
    let (known_powers, known_feats): (Vec<u16>, Vec<u16>) =
        (sheet.powers.rows().collect(), sheet.feats.rows().collect());
    check(
        want_powers.as_ref() == Some(&known_powers) && want_feats.as_ref() == Some(&known_feats),
        format!(
            "{name} names powers {powers:?} and feats {feats:?}; its sheet knows {known_powers:?} and \
             {known_feats:?}, wanted {want_powers:?} and {want_feats:?}"
        ),
    )
}

/// `owner`'s list `label` made `entries`, added when it has none.
fn set_list(owner: &mut GffStruct, label: &str, entries: Vec<GffStruct>) {
    match owner.fields.iter_mut().find(|field| field.label == label.as_bytes()) {
        Some(field) => field.value = GffValue::List(entries),
        None => owner.fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value: GffValue::List(entries),
        }),
    }
}

/// The action bar and the power wheel show a power's icon by its canonical
/// number (`hud`): Master Heal's slot shows the mod's icon, and a lent
/// row's slot its KOTOR 2 icon with its name from KOTOR 2's talk table.
fn bar_icon(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let spells = modded.resolver.two_da("spells").ok_or("no spells.2da")?;
    use sim::combat::spells::{FORCE_POWER_FORCE_BARRIER, FORCE_POWER_MASTER_HEAL};
    let icon = spells.text(usize::from(FORCE_POWER_MASTER_HEAL), "iconresref");
    let tsl_spells = data.tsl.two_da("spells").ok_or("no KOTOR 2 spells.2da")?;
    let barrier = usize::from(FORCE_POWER_FORCE_BARRIER);
    let lent_icon = spells.text(barrier, "iconresref");
    let name = spells.int(barrier, "name").map(|strref| strref as u32);
    let tsl_name = tsl_spells.int(barrier, "name").map(|strref| strref as u32 | LENT_FLAG);
    check(
        icon.as_deref() == Some("ip_modheal")
            && lent_icon.is_some()
            && lent_icon == tsl_spells.text(barrier, "iconresref")
            && name.is_some()
            && name == tsl_name,
        format!(
            "Master Heal's slot shows {icon:?}; Force Barrier's {lent_icon:?} named {name:?} (KOTOR 2's {tsl_name:?})"
        ),
    )
}

/// The made-up mod opened twice gives the same maps and the same digest,
/// which differs from the unmodded loan's (co-op compares it).
fn deterministic(data: &Data) -> Result<String, String> {
    let (first, second) = (modded(data)?, modded(data)?);
    let same = first.resolver.rows == second.resolver.rows
        && first.resolver.lent_digest() == second.resolver.lent_digest();
    let differs = first.resolver.lent_digest() != data.lent.lent_digest();
    check(
        same && differs && !first.resolver.rows.is_identity(),
        format!("the same maps and digest twice {same}; a digest of its own {differs}"),
    )
}

/// The resolver tells a lent impact script from the game's own: KOTOR 2's
/// `k2_sp1_generic` speaks canonical numbers, KOTOR 1's `k_sp1_generic`
/// install numbers.
fn lent_scripts(data: &Data) -> Result<String, String> {
    let modded = modded(data)?;
    let lent = vfs::lent::lent_script(&modded.resolver, "k2_sp1_generic");
    let own = vfs::lent::lent_script(&modded.resolver, "k_sp1_generic");
    check(lent && !own, format!("k2_sp1_generic lent {lent}; k_sp1_generic lent {own}"))
}

/// A feat of a mod's own, appended past KOTOR 1's rows and moved past
/// KOTOR 2's (as feat mods add dummy feats): a class may pick it at
/// level-up, it is shown with its name and icon, and an item's Bonus Feat
/// property and Use Limitation Feat property that name its install row give
/// and need it by its canonical number. A sheet that knows it survives a
/// snapshot, and GetHasFeat answers it (`sim::rows_by_label_probes`).
fn own_feat(data: &Data) -> Result<String, String> {
    use sim::inventory::properties::{Limit, Property};
    let modded = modded(data)?;
    let install = modded.feats[1];
    let number = canonical(&modded.resolver, Table::Feats, install).ok_or("no number")?;
    let feats = modded.resolver.two_da("feat").ok_or("no feat.2da")?;
    let talk = modded.resolver.talk.get().ok_or("no talk table")?;
    let row = usize::from(number);
    let name = feats.int(row, "name").and_then(|strref| talk.text(strref as u32)).map(<[u8]>::to_vec);
    let icon = feats.resref(row, "icon");
    let shown = name.as_ref().is_some_and(|name| !name.is_empty())
        && icon.as_ref().is_some_and(|icon| modded.resolver.texture(icon).is_some());
    let classes = session::classes::classes(&ResolverSource(&modded.resolver));
    let picked = classes.classes.iter().filter(|class| class.feats.contains(&number)).count();
    // An item template with both properties naming the install row.
    let name_of = modded
        .resolver
        .names(ResType::UTI)
        .into_iter()
        .find(|name| {
            modded
                .resolver
                .read(name, ResType::UTI)
                .and_then(|bytes| Gff::parse(&bytes).ok())
                .is_some_and(|gff| !gff.root.list("PropertiesList").is_empty())
        })
        .ok_or("no item template with properties")?;
    let mut gff = Gff::parse(&modded.resolver.read(&name_of, ResType::UTI).ok_or("no item")?)
        .map_err(|e| e.to_string())?;
    let first = gff.root.list("PropertiesList").first().cloned().ok_or("no property")?;
    let property = |kind: u16| {
        let mut property = first.clone();
        for field in &mut property.fields {
            match field.label.as_slice() {
                b"PropertyName" => field.value = GffValue::Word(kind),
                b"Subtype" => field.value = GffValue::Word(install),
                _ => {}
            }
        }
        property
    };
    // Bonus Feat (9) and Use Limitation Feat (57).
    set_list(&mut gff.root, "PropertiesList", vec![property(9), property(57)]);
    let resolver = with_override(&modded.resolver, vec![(name_of.clone(), ResType::UTI, gff.write())]);
    let facts = session::item_facts(&ResolverSource(&resolver), &name_of).ok_or("the item does not read")?;
    let gives = facts.properties.contains(&Property::Feat(number));
    let needs = facts.limits.contains(&Limit::Feat(number));
    check(
        number >= Table::Feats.kotor2_rows() && shown && picked > 0 && gives && needs,
        format!(
            "install row {install} is {number}; name {:?}, icon {icon:?}, shown {shown}; {picked} classes may pick it; \
             {name_of} gives it {gives} and needs it {needs}",
            name.map(|name| String::from_utf8_lossy(&name).into_owned())
        ),
    )
}
