//! A store (`store`, opened by a script's `OpenStore`), as KOTOR's: one
//! list at a time, the store's stock to buy or (Show Sell List) what the
//! party carries to sell, at the store's prices; the chosen item's
//! description, price and count, and the party's credits. Accept, or a
//! double click on a row, buys or sells one of the chosen item (greyed when
//! the store does not trade so); the host's goods then show the result.
//! As K1's executable: an item dearer than the credits is not bought ("You
//! do not have enough credits"); one costing more than 50 credits a level
//! of the party's leader asks first ("This item costs more than …"), one
//! worth more than that, at most 250, asks before it is sold; a purchase
//! sounds `Inventory_Add`, a sale `Inventory_Drop`. Close or Back leaves
//! the store. KOTOR 2's filters show the items of a kind (weapons, armour,
//! the rest); its "on hand" is hidden.

use fmt_odyssey::gui::{Control, Gui};

use crate::input::Key;
use crate::intent::{GameAction, GuiSound, Intent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::filter::{self, Filter};
use crate::screens::pages::description;
use crate::screens::tab::{hidden, rows, text};
use crate::strings;
use crate::vm::{Entry, StoreView, ViewModel, Ware};

const STOCK: &str = "LB_SHOPITEMS";
const CARRIED: &str = "LB_INVITEMS";
const ACCEPT: &str = "BTN_Accept";

/// "You do not have enough credits to purchase this item."
const SHORT: u32 = 41950;
/// "This item costs more than <CUSTOM0> credits. Do you still wish to buy?"
const COSTS_MORE: u32 = 42020;
/// "This item is worth more than <CUSTOM0> credits. Do you still wish to
/// sell?"
const WORTH_MORE: u32 = 41985;
/// What a trade may come to unasked: so many credits a level of the
/// party's leader (none: one level's), a sale at most `SELL_ASKED`.
const ASKED_A_LEVEL: i32 = 50;
const SELL_ASKED: i32 = 250;
const CONFIRM_SCREEN: &str = "store_confirm";

pub struct Store {
    /// Showing what the party may sell, else the store's stock.
    selling: bool,
    filter: Filter,
    /// The row chosen in the list shown.
    chosen: Option<usize>,
}

impl Default for Store {
    fn default() -> Store {
        Store {
            selling: false,
            filter: Filter::new(filter::STORE),
            chosen: None,
        }
    }
}

impl Store {
    fn list(&self) -> &'static str {
        if self.selling { CARRIED } else { STOCK }
    }

    /// The wares of the list shown, of the filter's kinds.
    fn wares<'a>(&self, store: &'a StoreView) -> Vec<&'a Ware> {
        let all = if self.selling {
            &store.offers
        } else {
            &store.stock
        };
        self.filter.shown(all, |ware| &ware.entry)
    }

    fn ware<'a>(&self, store: &'a StoreView) -> Option<&'a Ware> {
        self.wares(store).get(self.chosen?).copied()
    }

    /// The chosen ware, if the store trades it so.
    fn tradeable<'a>(&self, store: &'a StoreView) -> Option<&'a Ware> {
        let open = if self.selling {
            store.buys
        } else {
            store.sells
        };
        self.ware(store).filter(|_| open)
    }

    /// Accept on the chosen ware: K1's checks, then the trade.
    fn accept(&self, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(store) = &vm.store else { return };
        let Some(ware) = self.tradeable(store) else {
            return;
        };
        let asked = asked(self.selling, vm);
        if !self.selling && ware.price > store.credits {
            out.push(Intent::OpenFor("message_box", SHORT.to_string()));
        } else if ware.price > asked {
            let deal = Deal {
                selling: self.selling,
                store: store.store,
                item: ware.item,
                asked,
            };
            out.push(Intent::OpenFor(CONFIRM_SCREEN, deal.to_string()));
        } else {
            trade(self.selling, store.store, ware.item, out);
        }
    }

    /// Whether the other list can be traded from.
    fn other_open(&self, store: &StoreView) -> bool {
        if self.selling {
            store.sells
        } else {
            store.buys
        }
    }

    fn leave(vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(store) = &vm.store {
            out.push(Intent::Game(GameAction::Leave(store.store)));
        }
        out.push(Intent::Close);
    }
}

/// The selling side's words over the layout's own (which are the buying
/// side's).
fn selling_text(selling: bool, words: &str) -> ControlState {
    if selling {
        text(words)
    } else {
        ControlState::default()
    }
}

impl Screen for Store {
    fn id(&self) -> ScreenId {
        ScreenId("store")
    }

    fn layout(&self) -> &str {
        "store"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        if let Some(state) = self.filter.state(&control.tag) {
            return state;
        }
        let store = vm.store.clone().unwrap_or(StoreView::default());
        let ware = self.ware(&store);
        match control.tag.as_str() {
            list @ (STOCK | CARRIED) if list == self.list() => {
                let entries: Vec<Entry> = self
                    .wares(&store)
                    .iter()
                    .map(|ware| ware.entry.clone())
                    .collect();
                rows(&entries, self.chosen)
            }
            STOCK | CARRIED => hidden(),
            "LB_DESCRIPTION" => description(ware.map_or(&[][..], |ware| &ware.entry.description)),
            "LBL_CREDITS_VALUE" => text(store.credits),
            "LBL_COST_VALUE" => text(ware.map_or(String::new(), |ware| ware.price.to_string())),
            "LBL_STOCK_VALUE" => text(
                ware.and_then(|ware| ware.entry.count)
                    .map_or(String::new(), |count| count.to_string()),
            ),
            "LBL_BUYSELL" => selling_text(self.selling, strings::SELL),
            "LBL_ONHAND" | "LBL_ONHAND_VALUE" => hidden(),
            // The game names the button by what it does.
            ACCEPT => ControlState {
                disabled: self.tradeable(&store).is_none(),
                ..text(if self.selling {
                    strings::SELL
                } else {
                    strings::BUY
                })
            },
            "BTN_Examine" => ControlState {
                disabled: !self.other_open(&store),
                ..selling_text(self.selling, strings::SHOW_BUY_LIST)
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.filter.press(tag) {
            self.chosen = None;
            return;
        }
        match tag {
            ACCEPT => self.accept(vm, out),
            "BTN_Examine" => {
                if vm
                    .store
                    .as_ref()
                    .is_some_and(|store| self.other_open(store))
                {
                    self.selling = !self.selling;
                    self.chosen = None;
                }
            }
            "BTN_Cancel" => Store::leave(vm, out),
            _ => {}
        }
    }

    fn row(&mut self, list: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        if list == self.list() {
            self.chosen = Some(index);
        }
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == self.list()).then_some(ACCEPT)
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key == Key::Back {
            Store::leave(vm, out);
            return true;
        }
        false
    }

    fn first_focus(&self) -> Option<&str> {
        Some(STOCK)
    }
}

/// What a trade may come to before K1 asks (`ASKED_A_LEVEL`).
fn asked(selling: bool, vm: &ViewModel) -> i32 {
    let level = vm.party.first().map_or(1, |leader| i32::from(leader.level));
    let most = ASKED_A_LEVEL * level;
    if selling { most.min(SELL_ASKED) } else { most }
}

/// One of `item` bought from or sold to `store`, heard as K1's.
fn trade(selling: bool, store: u32, item: u32, out: &mut Vec<Intent>) {
    out.push(Intent::Sound(if selling {
        GuiSound::InventoryDrop
    } else {
        GuiSound::InventoryAdd
    }));
    out.push(Intent::Game(if selling {
        GameAction::Sell { store, item }
    } else {
        GameAction::Buy { store, item }
    }));
}

/// A trade waiting on its question, as the screen's name carries it
/// (`buy:STORE:ITEM:ASKED`, `sell:…`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Deal {
    selling: bool,
    store: u32,
    item: u32,
    asked: i32,
}

impl std::fmt::Display for Deal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let side = if self.selling { "sell" } else { "buy" };
        write!(f, "{side}:{}:{}:{}", self.store, self.item, self.asked)
    }
}

impl std::str::FromStr for Deal {
    type Err = ();

    fn from_str(said: &str) -> Result<Deal, ()> {
        let mut parts = said.split(':');
        let selling = match parts.next() {
            Some("sell") => true,
            Some("buy") => false,
            _ => return Err(()),
        };
        let mut number = || {
            parts
                .next()
                .and_then(|part| part.parse::<i64>().ok())
                .ok_or(())
        };
        Ok(Deal {
            selling,
            store: u32::try_from(number()?).map_err(|_| ())?,
            item: u32::try_from(number()?).map_err(|_| ())?,
            asked: i32::try_from(number()?).map_err(|_| ())?,
        })
    }
}

/// K1's question before a dear trade (`confirm`): OK makes it, Cancel
/// goes back to the store.
pub struct Confirm {
    deal: Deal,
}

impl Confirm {
    pub fn new(said: &str) -> Option<Confirm> {
        Some(Confirm {
            deal: said.parse().ok()?,
        })
    }
}

impl Screen for Confirm {
    fn id(&self) -> ScreenId {
        ScreenId(CONFIRM_SCREEN)
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => ControlState {
                strref: Some(if self.deal.selling {
                    WORTH_MORE
                } else {
                    COSTS_MORE
                }),
                tokens: vec![("CUSTOM0", self.deal.asked.to_string().into_bytes())],
                ..ControlState::default()
            },
            "BTN_OK" => text(strings::OK),
            "BTN_CANCEL" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => {
                out.push(Intent::Close);
                let Deal {
                    selling,
                    store,
                    item,
                    ..
                } = self.deal;
                trade(selling, store, item, out);
            }
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        super::message::grow(gui);
    }
}
