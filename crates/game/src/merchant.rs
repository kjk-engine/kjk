//! The store's side of the game (render side only: it reads the presented
//! snapshots' goods, `sim::inventory::Goods`, and the items' templates):
//!
//! - while the snapshot has a trade for this player, the store's screen
//!   shows its stock at its prices (an item's value by the markup) and
//!   what the party carries, not plot items, at what it pays (by the
//!   markdown), and the party's credits; it opens when a trade begins
//!   (this owns the screen: a script's `OpenStore` notice opens nothing);
//! - it closes when the trade ends; closed any other way (the fall, `ui
//!   close`), the store is left; a store this player just left does not
//!   open again while the snapshot still has the trade (a joined player's
//!   lags by a round trip), for at most `LEAVING_FRAMES`;
//! - buying, selling and leaving go to the host as the loot window's
//!   actions do (`loot::send`, which notes a leave here).

use bevy::prelude::*;
use sim::action::Action;
use sim::inventory::{Goods, Item, PARTY, Trade, price};
use ui::assets::GuiAssets;
use ui::vm::{StoreView, Ware};

use crate::sim_host::{OwnSeat, PresentedSnapshot};
use crate::ui_view::Ui;

/// How long a leave is waited for before the snapshot is believed again.
pub const LEAVING_FRAMES: u32 = 120;

/// The store this player left, not yet left in the snapshot, with the
/// frames it is still waited for.
#[derive(Resource, Default)]
pub struct Leaving(pub Option<(u32, u32)>);

/// The store whose trade fills the open screen.
#[derive(Resource, Default)]
struct Trading(Option<u32>);

pub struct MerchantPlugin;

impl Plugin for MerchantPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Leaving>()
            .init_resource::<Trading>()
            .add_systems(Update, show.before(crate::ui_view::Stepped));
    }
}

/// `item` (object `id`) as the screen lists it, at `percent` of its value
/// (`bao_dur_met`: `character::described`).
fn ware(assets: &mut GuiAssets, (id, item): (u32, &Item), percent: i32, bao_dur_met: bool) -> Ware {
    let cost = assets.item(&item.template).map_or(0, |look| look.cost);
    Ware {
        item: id,
        entry: crate::character::owned(assets, id, item, bao_dur_met).entry,
        price: price(cost, percent),
    }
}

/// The trade as the screen shows it.
fn view(assets: &mut GuiAssets, goods: &Goods, trade: Trade, bao_dur_met: bool) -> StoreView {
    let terms = trade.terms;
    StoreView {
        store: trade.store,
        stock: goods
            .carried(trade.store)
            .map(|held| ware(assets, held, terms.markup, bao_dur_met))
            .collect(),
        offers: goods
            .carried(PARTY)
            .filter(|(_, item)| !item.plot)
            .map(|held| ware(assets, held, terms.markdown, bao_dur_met))
            .collect(),
        credits: goods.gold,
        sells: terms.sells,
        buys: terms.buys,
    }
}

fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    mut ui: ResMut<Ui>,
    mut leaving: ResMut<Leaving>,
    mut trading: ResMut<Trading>,
    mut outgoing: ResMut<crate::loot::Outgoing>,
) {
    let (Some(presented), Some(own)) = (presented, own) else {
        return;
    };
    let ui = &mut *ui;
    let snapshot = presented.0.current();
    let trade = snapshot
        .body(own.0)
        .and_then(|me| snapshot.goods.trade(me.id.0));
    // A leave waited for ends when the snapshot has it, or in time.
    if let Some((store, frames)) = &mut leaving.0 {
        *frames = frames.saturating_sub(1);
        if *frames == 0 || trade.is_none_or(|trade| trade.store != *store) {
            leaving.0 = None;
        }
    }
    let trade = trade.filter(|trade| leaving.0.is_none_or(|(store, _)| store != trade.store));
    let showing = ui.stack.has("store");
    match (trade, ui.assets.as_mut()) {
        // Closed by anything but leaving (the fall, `ui close`): left.
        (Some(trade), Some(_)) if trading.0 == Some(trade.store) && !showing => {
            outgoing.0.push(Action::Leave(trade.store));
            leaving.0 = Some((trade.store, LEAVING_FRAMES));
            trading.0 = None;
            ui.vm.store = None;
        }
        (Some(trade), Some(assets)) => {
            let met = snapshot.screens.bao_dur_met;
            ui.vm.store = Some(view(assets, &snapshot.goods, trade, met));
            if trading.0.replace(trade.store) != Some(trade.store) && !showing {
                ui.stack.open("store", &ui.vm);
            }
        }
        _ if trading.0.take().is_some() => {
            ui.vm.store = None;
            ui.stack.close("store");
        }
        _ => {}
    }
}
