//! `kjk script stores <game> [module] [--opens SCRIPT]`: stores checked end
//! to end (I19, `sim::script::stores`). The module (Larrim's on Taris by
//! default) is installed with its item templates, the game's own script
//! that opens its store (`k_ptar_larrimsto`, run from Larrim's
//! conversation) and two made here (`probe`):
//!
//! 1. Every store has its UTM's terms, and on the first visit its stock:
//!    each entry an item, one that never runs out an item of its own.
//! 2. The game's script opens the store for the player: a trade on the
//!    store's terms in the scripts' state and the snapshot's goods, the
//!    stock with it, and the store's screen told to the player.
//! 3. Without gold, buying is refused; with 5000 (`GiveGoldToCreature`),
//!    one of an item that never runs out is bought at its value by the
//!    markup (the stock keeps it), then a single item (it leaves the stock).
//! 4. That item sold back comes to its value by the markdown; a plot item
//!    is not for sale.
//! 5. Leaving ends the trade (buying is then refused); opened again, a
//!    save carries the trade and the goods.

use std::process::ExitCode;

use serde_json::json;
use sim::action::Action;
use sim::inventory::{Goods, PARTY, Trade, price};
use sim::module::ObjectKind;
use sim::notice::{Notice, Screen};
use vfs::Discovery;

use crate::probe::{Call, Probe, int, me, program, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, ncs_type, open};

/// The most steps the game's script is given to open the store (it waits
/// 0.1 s first).
const OPENING_STEPS: u64 = 30;

/// Who holds item `id` in `goods`, if they show it.
fn holder(goods: &Goods, id: u32) -> Option<u32> {
    goods
        .items
        .iter()
        .find(|(of, _)| *of == id)
        .map(|(_, item)| item.holder)
}

pub fn stores(discovery: &Discovery, game: &str, module: &str, opens: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, index, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let resolver = base
            .with_module(&index, module)
            .map_err(|error| error.to_string())?;
        let opener = resolver
            .find(opens, ncs_type())
            .ok_or(format!("{opens}.ncs: not in {module}"))?
            .location
            .read()
            .map_err(|error| error.to_string())?;
        let opener = fmt_odyssey::ncs::Ncs::parse(&opener).map_err(|e| format!("{opens}: {e}"))?;
        let mut templates = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("the module has no scripts")?;
            let items = own.items().clone();
            let plot = items
                .iter()
                .find(|facts| facts.cost > 0 && !facts.plot)
                .ok_or("no item with a value")?
                .template
                .clone();
            let gold = vec![int(5000), me(), Call::Routine("GiveGoldToCreature", 2)];
            let made_plot = vec![
                int(1),
                int(1),
                me(),
                text(&plot),
                Call::Routine("CreateItemOnObject", 3),
                Call::Routine("SetPlotFlag", 2),
            ];
            let mut scripts = std::collections::BTreeMap::new();
            scripts.insert(opens.to_string(), opener);
            scripts.insert("kjk_gold".to_string(), program(&routines, &[gold])?);
            scripts.insert("kjk_plot".to_string(), program(&routines, &[made_plot])?);
            templates = Some((items.clone(), plot.clone()));
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(items),
            )
        })?;
        let (templates, plot_template) = templates.ok_or("no templates")?;
        let level = probe.world.level().clone();
        let snapshot = probe.world.snapshot();
        let objects = sim::Instances::in_snapshot(&level, &snapshot);
        let state = probe.world.script_state().clone();

        // 1. Every store's terms and first stock.
        let stores: Vec<(u32, &sim::module::ObjectData)> = snapshot
            .entities
            .iter()
            .filter_map(|entity| match entity.kind {
                sim::Kind::Object(ObjectKind::Store, index) => {
                    Some((entity.id.0, objects.get(index)?))
                }
                _ => None,
            })
            .collect();
        let mut facts = Vec::new();
        let mut stocked = !stores.is_empty();
        for (id, store) in &stores {
            let terms = store.store.ok_or(format!("{}: no terms", store.tag))?;
            let held = |infinite: bool| {
                store
                    .inventory
                    .iter()
                    .filter(move |h| h.infinite == infinite)
            };
            let lasting: Vec<&str> = held(true).map(|h| h.template.as_str()).collect();
            let units: u32 = held(false).map(|h| u32::from(h.stack)).sum();
            let stock: Vec<&sim::inventory::Item> = state
                .items
                .values()
                .filter(|item| item.holder == *id)
                .collect();
            let have = |infinite: bool| stock.iter().filter(move |item| item.infinite == infinite);
            stocked &= have(false).map(|item| u32::from(item.stack)).sum::<u32>() == units
                && have(true).count() == {
                    let mut once = lasting.clone();
                    once.sort();
                    once.dedup();
                    once.len()
                };
            facts.push(json!({
                "tag": store.tag,
                "markup": terms.markup,
                "markdown": terms.markdown,
                "sells": terms.sells,
                "buys": terms.buys,
                "entries": store.inventory.len(),
                "items": stock.len(),
                "never_run_out": lasting,
            }));
        }
        report.check(
            stocked,
            format!(
                "{} stores stocked on the first visit as their UTMs list: {}",
                stores.len(),
                facts
                    .iter()
                    .map(|f| format!(
                        "{} (markup {}, markdown {}, {} entries, {} never run out)",
                        f["tag"].as_str().unwrap_or(""),
                        f["markup"],
                        f["markdown"],
                        f["entries"],
                        f["never_run_out"].as_array().map_or(0, Vec::len)
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        );

        // 2. The game's own script opens a store.
        probe.queue(opens, probe.me);
        let mut told = Vec::new();
        let mut opened = None;
        for _ in 0..OPENING_STEPS {
            let (snapshot, notices) = probe.step_told(1);
            told.extend(notices);
            if let Some(trade) = snapshot.goods.trade(probe.me.0) {
                opened = Some((trade, snapshot));
                break;
            }
        }
        let (trade, snapshot) = opened.ok_or(format!("{opens} opened no trade"))?;
        let store = stores
            .iter()
            .find(|(id, _)| *id == trade.store)
            .map(|(_, store)| *store)
            .ok_or("the trade is not with a store of the module")?;
        let terms = store.store.ok_or("no terms")?;
        let state = probe.world.script_state().clone();
        let shown = told.iter().any(|notice| {
            matches!(notice, Notice::Show(Screen::Store { store, .. }) if *store == trade.store)
        });
        let stock_seen = snapshot.goods.carried(trade.store).count();
        report.check(
            trade.terms == terms
                && state.trades.get(&probe.me.0) == Some(&trade)
                && shown
                && stock_seen > 0
                && stock_seen == state.carried(trade.store).len(),
            format!(
                "{opens} opens {} for the player on its terms (markup {}, markdown {}): its screen told, {stock_seen} wares in the snapshot",
                store.tag, trade.terms.markup, trade.terms.markdown
            ),
        );

        // 3. Buying: one of a ware that never runs out, and a single one.
        let value = |template: &str| templates.get(template).map_or(0, |facts| facts.cost);
        let wares: Vec<(u32, sim::inventory::Item)> = snapshot
            .goods
            .carried(trade.store)
            .map(|(id, item)| (id, item.clone()))
            .collect();
        let worth = |item: &sim::inventory::Item| !item.plot && value(&item.template) > 0;
        let lasting = wares
            .iter()
            .find(|(_, item)| item.infinite && worth(item))
            .cloned();
        let single = wares
            .iter()
            .find(|(_, item)| {
                !item.infinite
                    && item.stack == 1
                    && worth(item)
                    && lasting
                        .as_ref()
                        .is_none_or(|(_, l)| l.template != item.template)
            })
            .cloned();
        let (first, first_item) = single
            .clone()
            .or(lasting.clone())
            .ok_or("no ware with a value")?;
        let buy = |item| Action::Buy {
            store: trade.store,
            item,
        };
        let (_, refused) = probe.act(buy(first));
        report.check(
            refused.contains(&Notice::CannotAfford) && probe.world.script_state().gold == 0,
            format!("with no gold, buying {} is refused", first_item.template),
        );
        probe.queue("kjk_gold", probe.me);
        let mut gold = probe.step(1).goods.gold;
        if let Some((lasting, item)) = &lasting {
            let (after, told) = probe.act(buy(*lasting));
            let paid = price(value(&item.template), terms.markup);
            let carried: u32 = after
                .goods
                .carried(PARTY)
                .filter(|(_, carried)| carried.template == item.template)
                .map(|(_, carried)| u32::from(carried.stack))
                .sum();
            report.check(
                told.is_empty()
                    && after.goods.gold == gold - paid
                    && carried == 1
                    && after.goods.items.iter().any(|(id, _)| id == lasting),
                format!(
                    "given {gold}, one {} bought for {paid} (value {} at {}%): gold {}, the stock keeps it",
                    item.template,
                    value(&item.template),
                    terms.markup,
                    after.goods.gold
                ),
            );
            gold = after.goods.gold;
        }
        if let Some((single, item)) = &single {
            let (after, told) = probe.act(buy(*single));
            let paid = price(value(&item.template), terms.markup);
            report.check(
                told.is_empty()
                    && after.goods.gold == gold - paid
                    && holder(&after.goods, *single) == Some(PARTY),
                format!(
                    "{} bought for {paid}: the item itself the party's now, gone from the stock",
                    item.template
                ),
            );
            gold = after.goods.gold;
        }

        // 4. Selling what was bought: the single item, else the copy.
        let sold = match &single {
            Some((id, _)) => *id,
            None => probe
                .world
                .snapshot()
                .goods
                .carried(PARTY)
                .find(|(_, item)| item.template == first_item.template)
                .map(|(id, _)| id)
                .ok_or("nothing bought to sell")?,
        };
        let (after, told) = probe.act(Action::Sell {
            store: trade.store,
            item: sold,
        });
        let got = price(value(&first_item.template), terms.markdown);
        if terms.buys {
            report.check(
                told.is_empty()
                    && after.goods.gold == gold + got
                    && holder(&after.goods, sold) == Some(trade.store),
                format!(
                    "{} sold back for {got} (at {}%): gold {}, in the stock",
                    first_item.template, terms.markdown, after.goods.gold
                ),
            );
        } else {
            report.check(
                told.contains(&Notice::NotForSale)
                    && after.goods.gold == gold
                    && holder(&after.goods, sold) == Some(PARTY),
                "the store buys nothing: selling it back is refused".to_string(),
            );
        }
        probe.queue("kjk_plot", probe.me);
        let snapshot = probe.step(1);
        let plot = snapshot
            .goods
            .carried(PARTY)
            .find(|(_, item)| item.plot && item.template == plot_template)
            .map(|(id, item)| (id, item.template.clone()))
            .ok_or("no plot item made")?;
        let (after, refused) = probe.act(Action::Sell {
            store: trade.store,
            item: plot.0,
        });
        report.check(
            refused.contains(&Notice::NotForSale)
                && after.goods.gold == snapshot.goods.gold
                && after
                    .goods
                    .items
                    .iter()
                    .any(|(id, item)| *id == plot.0 && item.holder == PARTY),
            format!("a plot {} is not for sale", plot.1),
        );

        // 5. Leaving, and a save.
        probe.act(Action::Leave(trade.store));
        let (after, refused) = probe.act(buy(first));
        let left = probe.world.script_state().trades.is_empty() && after.goods.trades.is_empty();
        report.check(
            left && refused.contains(&Notice::NoTrade),
            "leaving ends the trade: buying after is refused".to_string(),
        );
        probe.queue(opens, probe.me);
        let mut again: Option<Trade> = None;
        for _ in 0..OPENING_STEPS {
            if let Some(trade) = probe.step(1).goods.trade(probe.me.0) {
                again = Some(trade);
                break;
            }
        }
        let snapshot = probe.world.snapshot();
        let state = probe.world.script_state().clone();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: snapshot.clone(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            again.is_some()
                && loaded.script.trades == state.trades
                && loaded.snapshot.goods == snapshot.goods,
            format!(
                "opened again, a save carries the trade and the goods ({} items, {} gold)",
                snapshot.goods.items.len(),
                snapshot.goods.gold
            ),
        );
        let faults = probe.world.script_report().faults.clone();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "opens": opens, "stores": facts }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
