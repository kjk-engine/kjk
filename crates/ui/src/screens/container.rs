//! A container's contents (`container`), live: what is left in it as the
//! host has it, alike items one stack with its count on the icon, an item
//! asked for shown faded until the host confirms it. Activating an item
//! takes it (a stack of more than one asks how many first, below). OK (or
//! A) takes everything and closes it. Close or Back stops holding it open.
//!
//! Its give mode is KOTOR's (`CSWGuiContainer::SetupGiveMode`, issue 47):
//! `BTN_GIVEITEMS` ("Give Items", 38543) lists what the party carries but
//! its plot items under "Items Available to Place in Container" (392), and
//! activating one puts its whole stack in (`GiveItem`: `PerformGiveItem`
//! with the entry's count); the same button, now "Get Items" (38542), goes
//! back to the contents. OK takes nothing there, as KOTOR's.
//!
//! kjk's number box (`HowMany`, not in either game): activating a stack of
//! more than one, in either mode, asks how many to move first. KOTOR 2's
//! pazaak wager box shows the number, with less and more (or Left and
//! Right) to change it by one. The number starts at the whole stack when
//! taking, so OK takes what KOTOR takes, and at one when giving.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{enabled, skin_line, text};
use crate::strings;
use crate::vm::{LootRow, ViewModel};

/// KOTOR's words for the give mode: its list's title, the button into it
/// and the one back.
const GIVE_TITLE: u32 = 392;
const GIVE_ITEMS: u32 = 38543;
const GET_ITEMS: u32 = 38542;

#[derive(Default)]
pub struct Container {
    /// In give mode: the party's items listed, to put in.
    giving: bool,
    /// The mode changed since the list was last asked: its cursor starts
    /// at the top (`lists_anew`).
    switched: bool,
}

/// kjk's number box, by its screen's name, and the modes it is opened in
/// (`HowMany::new`'s words: `give:ITEM`, `take:ITEM`, the stack's first
/// item).
pub const HOW_MANY: &str = "how_many";
const GIVE: &str = "give";
const TAKE: &str = "take";

/// The moves that take (or, `giving`, put) `count` of `row` (from its
/// first item on): each item whole while the count holds it, the last one
/// asked for only in part.
fn moves(container: u32, row: &LootRow, giving: bool, count: u32, out: &mut Vec<Intent>) {
    let mut left = count;
    for (at, &item) in row.items.iter().enumerate() {
        if left == 0 {
            break;
        }
        let stack = u32::from(row.stacks.get(at).copied().unwrap_or(1).max(1));
        let count = (left < stack).then_some(left as u16);
        left = left.saturating_sub(stack);
        out.push(Intent::Game(if giving {
            GameAction::Put {
                container,
                item,
                count,
            }
        } else {
            GameAction::Take {
                container,
                item,
                count,
            }
        }));
    }
}

/// kjk's number box over the container (module docs): how many of one of
/// its stacks to take, or of one of the party's to give.
pub struct HowMany {
    giving: bool,
    /// The stack's first item, which finds its row.
    first: u32,
    /// The number asked; none for where it starts.
    asked: Option<u32>,
}

impl HowMany {
    /// The box for `said` (`give:ITEM` or `take:ITEM`).
    pub fn new(said: &str) -> Option<HowMany> {
        let (mode, first) = said.split_once(':')?;
        Some(HowMany {
            giving: match mode {
                GIVE => true,
                TAKE => false,
                _ => return None,
            },
            first: first.parse().ok()?,
            asked: None,
        })
    }

    /// The stack it asks about, while the container still shows it.
    fn asked_row<'a>(&self, vm: &'a ViewModel) -> Option<(u32, &'a LootRow)> {
        let loot = vm.loot.as_ref()?;
        let rows = if self.giving { &loot.give } else { &loot.rows };
        rows.iter()
            .find(|row| row.items.contains(&self.first) && !row.pending)
            .map(|row| (loot.container, row))
    }

    fn count(&self, row: &LootRow) -> u32 {
        let start = if self.giving { 1 } else { row.count };
        self.asked.unwrap_or(start).clamp(1, row.count.max(1))
    }

    fn step(&mut self, vm: &ViewModel, by: i32) {
        if let Some((_, row)) = self.asked_row(vm) {
            let count = self.count(row).saturating_add_signed(by);
            self.asked = Some(count.clamp(1, row.count.max(1)));
        }
    }
}

impl Screen for HowMany {
    fn id(&self) -> ScreenId {
        ScreenId(HOW_MANY)
    }

    fn layout(&self) -> &str {
        "pazaakwager"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let Some((_, row)) = self.asked_row(vm) else {
            return ControlState::default();
        };
        let count = self.count(row);
        match control.tag.as_str() {
            "LBL_TITLE" if self.giving => skin_line(GIVE_ITEMS),
            "LBL_TITLE" => skin_line(GET_ITEMS),
            "LBL_MAXIMUM" => ControlState {
                text: Some(
                    [
                        row.name.as_slice(),
                        &strings::bytes(&format!(" ({})", row.count)),
                    ]
                    .concat(),
                ),
                ..ControlState::default()
            },
            "LBL_WAGERVAL" => text(count),
            "BTN_LESS" => enabled(count > 1),
            "BTN_MORE" => enabled(count < row.count),
            "BTN_WAGER" => text(strings::OK),
            "BTN_QUIT" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_LESS" => self.step(vm, -1),
            "BTN_MORE" => self.step(vm, 1),
            "BTN_WAGER" => {
                out.push(Intent::Close);
                if let Some((container, row)) = self.asked_row(vm) {
                    moves(container, row, self.giving, self.count(row), out);
                }
            }
            "BTN_QUIT" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn key(&mut self, key: crate::input::Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        use crate::input::Key;
        match key {
            Key::Left => self.step(vm, -1),
            Key::Right => self.step(vm, 1),
            Key::Back => self.press("BTN_QUIT", vm, out),
            _ => return false,
        }
        true
    }

    fn back(&mut self, _: &mut Vec<Intent>) {}

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_WAGER")
    }
}

/// The key that takes everything (`KeyA`).
const TAKE_ALL_KEY: &str = "KeyA";

impl Container {
    /// The rows the list shows: the container's, or in give mode the
    /// party's.
    fn rows<'a>(&self, vm: &'a ViewModel) -> &'a [LootRow] {
        vm.loot.as_ref().map_or(
            &[],
            |loot| {
                if self.giving { &loot.give } else { &loot.rows }
            },
        )
    }

    fn take_all(vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(loot) = &vm.loot {
            out.push(Intent::Game(GameAction::TakeAll(loot.container)));
        }
        Container::close(vm, out);
    }

    fn close(vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(loot) = &vm.loot {
            out.push(Intent::Game(GameAction::Close(loot.container)));
        }
        out.push(Intent::Close);
    }
}

impl Screen for Container {
    fn id(&self) -> ScreenId {
        ScreenId("container")
    }

    /// A pad's A takes the item chosen, X takes all.
    fn pad_x(&self) -> Option<&str> {
        (!self.giving).then_some("BTN_OK")
    }

    /// Its Menu closes (Take all is X's).
    fn accept(&self) -> Option<&str> {
        Some("BTN_CANCEL")
    }

    /// KOTOR 2's box sits at the frame's top left corner, and the stack
    /// centres it as KOTOR 2 does (`screen::CENTRED`).
    fn layout(&self) -> &str {
        "container"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let text = |words: &str| ControlState {
            text: Some(strings::bytes(words)),
            ..ControlState::default()
        };
        match control.tag.as_str() {
            "LBL_MESSAGE" if self.giving => skin_line(GIVE_TITLE),
            "LBL_MESSAGE" => ControlState {
                text: vm.loot.as_ref().map(|loot| loot.title.clone()),
                ..ControlState::default()
            },
            "LB_ITEMS" => ControlState {
                list: Some(ListView {
                    rows: self
                        .rows(vm)
                        .iter()
                        .map(|row| Row {
                            text: row.name.clone(),
                            icon: row.icon.clone(),
                            count: Some(row.count),
                            dimmed: row.pending,
                            ..Row::default()
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "BTN_OK" if self.giving => ControlState {
                disabled: true,
                ..skin_line(GIVE_ITEMS)
            },
            "BTN_OK" if vm.pad_playing => text(strings::TAKE_ALL_PAD),
            "BTN_OK" => text(strings::TAKE_ALL),
            "BTN_CANCEL" => text(strings::CLOSE),
            "BTN_GIVEITEMS" if self.giving => skin_line(GET_ITEMS),
            "BTN_GIVEITEMS" => skin_line(GIVE_ITEMS),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" if !self.giving => Container::take_all(vm, out),
            "BTN_CANCEL" => Container::close(vm, out),
            "BTN_GIVEITEMS" => {
                self.giving = !self.giving;
                self.switched = true;
            }
            _ => {}
        }
    }

    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let take = key == TAKE_ALL_KEY && !self.giving;
        if take {
            Container::take_all(vm, out);
        }
        take
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        let (Some(loot), Some(row)) = (&vm.loot, self.rows(vm).get(index)) else {
            return;
        };
        if row.pending {
            return;
        }
        if row.count > 1 {
            // A double click activates the row twice: one box.
            let asking = out
                .iter()
                .any(|intent| matches!(intent, Intent::OpenFor(HOW_MANY, _)));
            if !asking {
                let mode = if self.giving { GIVE } else { TAKE };
                out.push(Intent::OpenFor(
                    HOW_MANY,
                    format!("{mode}:{}", row.items[0]),
                ));
            }
            return;
        }
        moves(loot.container, row, self.giving, row.count, out);
    }

    fn back(&mut self, _out: &mut Vec<Intent>) {}

    fn key(&mut self, key: crate::input::Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        if key == crate::input::Key::Back {
            Container::close(vm, out);
            return true;
        }
        false
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }

    fn lists_anew(&mut self) -> bool {
        std::mem::take(&mut self.switched)
    }
}
