//! Stores (I19): KOTOR's merchants. A store is a module object from the
//! GIT's `StoreList` (a UTM; `ObjectData::store` its terms); its stock is
//! its items in play, those a UTM entry marks `Infinite` never running
//! out. `OpenStore` opens a trade between a player and a store on the
//! store's terms with the call's bonuses (and runs its `OnOpenStore`); the
//! player buys and sells through it one of an item at a time, paid from and
//! to the party's gold, until it leaves or its seat readies for an exit
//! (R11); while more than one seat plays a body, also until it is hurt or a
//! party conversation gathers it (R8). A price is the item's value (its
//! UTI's `Cost`) by the markup to buy and by the markdown to sell
//! (`inventory::price`). Plot items and what the party wears are not for
//! sale. Components and chemicals bought are the party's counts
//! (`State::pool_carried`). What players see of it rides in the snapshot
//! (`inventory::Goods`).

use std::collections::BTreeMap;

use super::state::State;
use super::{Library, workbench};
use crate::action::Action;
use crate::inventory::{Goods, Item, PARTY, Terms, Trade, price, slots};
use crate::notice::Notice;

impl State {
    /// `player` trades with `store` from now on, on `terms` with `bonus`
    /// (markup, markdown) added.
    pub(crate) fn open_trade(&mut self, player: u32, store: u32, terms: Terms, bonus: (i32, i32)) {
        let terms = Terms {
            markup: terms.markup.saturating_add(bonus.0),
            markdown: terms.markdown.saturating_add(bonus.1),
            ..terms
        };
        self.trades.insert(player, Trade { store, terms });
    }

    /// `player`'s trade ends, as its Leave ends it: the store's screen
    /// closes as the goods no longer list it. Whether it had one.
    pub(crate) fn end_trade(&mut self, player: u32) -> bool {
        self.trades.remove(&player).is_some()
    }

    /// What `player`'s trading action comes to (what the party's pools
    /// gained); a refusal is for its player. Other actions are not the
    /// stores'.
    pub(crate) fn trade(
        &mut self,
        library: &Library,
        player: u32,
        action: Action,
    ) -> Result<workbench::Acquired, Notice> {
        let catalog = library.items();
        let (store, item, buying) = match action {
            Action::Buy { store, item } => (store, item, true),
            Action::Sell { store, item } => (store, item, false),
            Action::Leave(store) => {
                if self
                    .trades
                    .get(&player)
                    .is_some_and(|trade| trade.store == store)
                {
                    self.end_trade(player);
                }
                return Ok(workbench::Acquired::default());
            }
            _ => return Ok(workbench::Acquired::default()),
        };
        let terms = self
            .trades
            .get(&player)
            .filter(|trade| trade.store == store)
            .ok_or(Notice::NoTrade)?
            .terms;
        let (from, to, open, percent) = if buying {
            (store, PARTY, terms.sells, terms.markup)
        } else {
            (PARTY, store, terms.buys, terms.markdown)
        };
        let value = self
            .items
            .get(&item)
            .filter(|offered| {
                open && offered.holder == from
                    && offered.slot.is_none()
                    && (buying || !offered.plot)
            })
            .and_then(|offered| self.item_facts(catalog, &offered.template))
            .ok_or(Notice::NotForSale)?
            .cost;
        let paid = price(value, percent);
        if buying {
            if self.gold < paid {
                return Err(Notice::CannotAfford);
            }
            self.gold -= paid;
        } else {
            self.gold = self.gold.saturating_add(paid);
        }
        self.hand_over(catalog, item, to);
        Ok(self.pool_carried(library, PARTY))
    }

    /// What players see of the items in play: the gold, the items the
    /// party carries and its characters (`members`) wear, the weapons any
    /// character in the module (`here`) holds in its hands and the armour on
    /// its body (drawn on it: its body is dressed by it),
    /// the trades under way and their stores' stock, what the containers
    /// held `open` hold that may be taken (the loot window's, `loot`), the
    /// items lying on `module`'s floor (drawn there); in id order.
    pub fn goods(
        &self,
        members: &[u32],
        here: impl IntoIterator<Item = u32>,
        open: &[u32],
        module: &str,
    ) -> Goods {
        let stores = self.trades.values().map(|trade| trade.store);
        let whole = [PARTY]
            .into_iter()
            .chain(members.iter().copied())
            .chain(stores);
        let mut items: BTreeMap<u32, &Item> = whole
            .flat_map(|holder| self.items.held_by(holder))
            .collect();
        let drawn = |item: &Item| {
            item.slot.is_some_and(|slot| {
                matches!(slot, slots::RIGHT_WEAPON | slots::LEFT_WEAPON | slots::BODY)
            })
        };
        for holder in here {
            items.extend(self.items.held_by(holder).filter(|(_, item)| drawn(item)));
        }
        for &container in open {
            items.extend(self.loot(container));
        }
        let floor: Vec<_> = self.lying_in(module).collect();
        items.extend(
            floor
                .iter()
                .filter_map(|(id, _)| Some((*id, self.items.get(id)?))),
        );
        Goods {
            gold: self.gold,
            items: items
                .into_iter()
                .map(|(id, item)| (id, item.clone()))
                .collect(),
            trades: self
                .trades
                .iter()
                .map(|(player, trade)| (*player, *trade))
                .collect(),
            floor,
        }
    }
}
