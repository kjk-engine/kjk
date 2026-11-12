//! TSL's workbench as its player sees it (`ui::vm::WorkbenchView`, render
//! side only): the bench the host told (`sim::notice::Screen::Workbench`),
//! read with the game's own tables (`session::workshop`, the rules
//! `sim::inventory::workshop`'s) and the presented snapshot: what the table
//! lists for the maker (its skill, level and alignment), each item's price
//! and whether the skill reaches it, the skill asked ("Repair: 5"), how
//! many the party carries; what the party carries that the bench breaks
//! down and what each yields by the maker's Repair (Treat Injury at the lab
//! station). The maker's ranks are its totals with what it wears and what
//! runs on it, as the host's rules read them.

use bevy::prelude::*;
use sim::inventory::workshop::{Piece, Workshop, yield_skill};
use sim::inventory::{Item, PARTY};
use sim::notice::Bench;
use sim::script::Library;
use ui::assets::GuiAssets;
use ui::vm::{BreakdownView, RecipeView, WorkbenchView};

/// The bench a script opened for this player, while its screens are open,
/// and the game's tables read for it.
#[derive(Resource, Default)]
pub struct OpenBench(pub Option<(Bench, Workshop)>);

impl OpenBench {
    /// `bench` told: a script's open reads the game's tables; one told back
    /// after a breakdown or an item made moves only the party's stock.
    pub fn told(&mut self, assets: &GuiAssets, bench: &Bench, again: bool) {
        match &mut self.0 {
            Some((open, _)) if again => open.stock = bench.stock,
            _ => {
                let source = crate::viewer::ResolverSource(assets.resolver());
                self.0 = Some((bench.clone(), session::workshop::workshop(&source)));
            }
        }
    }
}

/// The bench as `snapshot` leaves it.
pub fn view(
    assets: &mut GuiAssets,
    bench: (&Bench, &Workshop),
    snapshot: &sim::Snapshot,
    library: Option<&Library>,
) -> WorkbenchView {
    let (worn, sheet) = snapshot
        .entities
        .iter()
        .find(|entity| entity.id.0 == bench.0.maker)
        .and_then(|entity| {
            let (sheet, fighter) = (entity.sheet?, entity.fighter.as_ref()?);
            let now = sim::combat::effects::sheet_now(sheet, fighter, snapshot.time_ms);
            Some((sheet, now))
        })
        .unwrap_or_else(|| {
            (
                sim::combat::Sheet::commoner(),
                sim::combat::Sheet::commoner(),
            )
        });
    let carried: Vec<(u32, &Item)> = snapshot.goods.carried(PARTY).collect();
    let met = snapshot.screens.bao_dur_met;
    of(assets, bench, (&worn, &sheet), library, (&carried, met))
}

/// The bench for a maker whose sheet without effects is `worn` and with
/// them `sheet`, the party carrying `carried` (`bao_dur_met`:
/// `character::described`). The maker's ranks are read as the host reads
/// them (`sim::script::workbench::total`), the key abilities from
/// `library`.
pub fn of(
    assets: &mut GuiAssets,
    (bench, shop): (&Bench, &Workshop),
    (worn, sheet): (&sim::combat::Sheet, &sim::combat::Sheet),
    library: Option<&Library>,
    (carried, bao_dur_met): (&[(u32, &Item)], bool),
) -> WorkbenchView {
    let total = |skill| sim::script::workbench::total(library, worn, sheet, skill);
    let held = |template: &str| -> u32 {
        carried
            .iter()
            .filter(|(_, item)| item.template == template)
            .map(|(_, item)| u32::from(item.stack))
            .sum()
    };
    let recipes = shop
        .table(&bench.list)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter_map(|(row, recipe)| {
            let rank = total(recipe.base_skill);
            recipe
                .listed(rank, sheet.level, sheet.good_evil)
                .then_some(())?;
            let look = assets.item(&recipe.template)?;
            let piece = Piece {
                template: &recipe.template,
                cost: look.cost,
                base_item: look.base_item,
                plot: look.plot,
            };
            let mut skill = assets
                .cell_text("skills", usize::from(recipe.base_skill), "name")
                .unwrap_or(Vec::new());
            skill.extend(format!(": {rank}").bytes());
            Some(RecipeView {
                row: u16::try_from(row).ok()?,
                entry: ui::vm::Entry {
                    name: look.name.clone(),
                    icon: Some(look.icon.clone()),
                    description: crate::character::described(assets, &look, None, bao_dur_met),
                    kind: Some(look.kind),
                    ..ui::vm::Entry::default()
                },
                group: recipe.group,
                price: shop.price(piece),
                open: recipe.open(rank),
                skill,
                held: held(&recipe.template),
            })
        })
        .collect();
    let rank = total(yield_skill(bench.lab));
    let breakdowns = carried
        .iter()
        .filter_map(|&(id, item)| {
            let look = assets.item(&item.template)?;
            let piece = Piece {
                template: &item.template,
                cost: look.cost,
                base_item: look.base_item,
                plot: look.plot || item.plot,
            };
            shop.breaks_down(piece, &bench.list, bench.lab)
                .then_some(())?;
            Some(BreakdownView {
                item: id,
                entry: crate::character::owned(assets, id, item, bao_dur_met).entry,
                yields: shop.yields(piece, rank),
            })
        })
        .collect();
    WorkbenchView {
        bench: ui::intent::BenchDesc {
            list: bench.list.clone(),
            lab: bench.lab,
            maker: bench.maker,
        },
        upgrade: bench.upgrade,
        stock: bench.stock,
        recipes,
        breakdowns,
    }
}
