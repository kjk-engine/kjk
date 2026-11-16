//! The action bar setup page (`ActionBar`), on the abilities page's KOTOR 2
//! layout. The page shows a party member's action bar slots and, in a grid
//! of small icons, everything that member can put on the bar. While the
//! keyboard plays, the page shows all six slots in a row. While a
//! controller plays, it shows only the first four slots, in a cross like
//! the controller's face buttons. The player drags an icon onto a slot to
//! put it there. The info box at the right tells of the icon or slot under
//! the pointer.

use fmt_odyssey::gui::{Border, Control, Gui, Kind as ControlKind, Rect};

use crate::draw::{DrawList, Quad};
use crate::input::Key;
use crate::intent::{BarContent, BarEdit, GameAction, Intent};
use crate::paint::{ControlState, Painter, rect_of};
use crate::screen::Pointer;
use crate::screens::hud::{CROSS_SLOTS, cross};
use crate::screens::next_id;
use crate::screens::pages::{Abilities, description, dresses};
use crate::screens::tab::{Page, hidden, image, shown, text};
use crate::skin::Source;
use crate::vm::{BarSetupView, BarSlot, ViewModel, WheelEntry};

/// The setup page's groups of icons, in the order they stand on the page,
/// each with its label. Group 0 is the Force powers, group 1 the feat
/// attacks and group 2 the items, mines and stealth mode.
const GROUPS: [&str; 3] = ["Force powers", "Feat attacks", "Items"];

/// The setup page's slot frames and the icons in them. Slot `n` is
/// `BTN_BARSLOTn` and its icon `LBL_BARSLOTn`, from `n` = 0.
const SLOT: &str = "BTN_BARSLOT";
const SLOT_ICON: &str = "LBL_BARSLOT";
/// The grid's icons. Icon `i` of group `g` is the frame `BTN_PICKg_i` with
/// the icon `LBL_PICKg_i` in it, from 0.
const PICK: &str = "BTN_PICK";
const PICK_ICON: &str = "LBL_PICK";
/// Group `g`'s label is `LBL_GROUPg`.
const GROUP_LABEL: &str = "LBL_GROUP";
/// How many slots the setup page shows while the keyboard plays. They stand
/// in a row. While a controller plays, the page shows only the first
/// `CROSS_SLOTS` slots, because a controller uses only those four. They
/// then stand in a cross (`shown_slots`, `Grid::lay_out`).
const SETUP_SLOTS: i32 = 6;
/// KOTOR 2's HUD action slot (`BTN_ACTION0` of its HUD layout), which the
/// setup page's slots copy. The first picture is the frame, and the second
/// is the frame while it has the focus. The size is the frame's width and
/// height. The icon is the icon's left, top and side inside the frame. The
/// sizes are in the HUD layout's pixels, and the setup page scales them.
const HUD_SLOT: (&str, &str) = ("lbl_mibox00", "lbl_mibox00a");
const HUD_SLOT_SIZE: [i32; 2] = [36, 60];
const HUD_SLOT_ICON: [i32; 3] = [3, 15, 30];
/// A setup slot's width, and the gap between two slots of the row (layout
/// pixels).
const SLOT_WIDE: i32 = 38;
const SLOT_GAP: i32 = 12;
/// The gap between two slots of the cross that the page shows while a
/// controller plays (layout pixels). It is a third of the row's gap, so the
/// four slots stand close together, as the face buttons do.
const CROSS_GAP: i32 = SLOT_GAP / 3;
/// KOTOR 2's square item frame (the equip page's `BTN_INV_HEAD`): its
/// corner and edge pictures, its thickness, and its colour at rest and
/// with the focus. Each icon of the grid stands in one.
const PICK_FRAME: (&str, &str, i32) = ("uibit_brdr_16wc", "uibit_brdr_16we", 16);
const PICK_COLOR: [[f32; 3]; 2] = [
    [0.050_980_393, 0.349_019_62, 0.270_588_25],
    [0.698_039_23, 0.698_039_23, 0.6],
];
/// A grid frame's side and the icon's inset in its frame (layout pixels).
/// A frame is about the size of a HUD slot's icon. The frames of a group
/// touch, with no gap between them.
const PICK_SIDE: i32 = 32;
const PICK_INSET: i32 = 2;
/// The room kept free between the grid and the info box (layout pixels).
const DESC_MARGIN: i32 = 8;
/// A group label's height, and the gap above each group (layout pixels).
const LABEL_TALL: i32 = 16;
const GROUP_GAP: i32 = 6;
/// The fill style that stretches a frame's picture over the whole frame,
/// as KOTOR 2's HUD layout sets for its action slots.
const STRETCHED: i32 = 2;

/// The setup page's words. No line of KOTOR 2 fits, so these are kjk's own.
/// A hidden power's description starts with `HIDDEN_FROM_WHEEL`.
const SETUP_TITLE: &str = "Action bar";
const SETUP_HELP: &str =
    "Drag a power, feat attack or item onto a slot to put it on the action bar.";
const SETUP_NOTHING: &str = "This character has nothing to put on the action bar.";
const SETUP_OTHERS: &str = "Another player sets up this character's action bar.";
const HIDDEN_FROM_WHEEL: &str = "This power is hidden from the wheel.";

/// The setup page's contents before the game has filled them in. The page
/// then shows no slots and no icons.
static NO_SETUP: BarSetupView = BarSetupView {
    who: 0,
    slots: Vec::new(),
    keys: Vec::new(),
    contents: Vec::new(),
    powers: Vec::new(),
    feats: Vec::new(),
    items: Vec::new(),
};

/// The abilities page's controls that the setup page does not use. It
/// hides them.
const UNUSED: [&str; 15] = [
    "BTN_SKILLS",
    "BTN_POWERS",
    "BTN_FEATS",
    "LBL_FILTER",
    "LBL_BAR2",
    "LBL_BAR6",
    "LB_ABILITY",
    "LB_DESC_FEATS",
    "LBL_INFOBG",
    "LBL_SKILLRANK",
    "LBL_RANKVAL",
    "LBL_BONUS",
    "LBL_BONUSVAL",
    "LBL_TOTAL",
    "LBL_TOTALVAL",
];

/// What has the focus on the setup page: an icon of the grid by its group
/// and its place, or a slot by its place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupFocus {
    Pick(usize, usize),
    Slot(usize),
}

impl SetupFocus {
    fn of(tag: &str) -> Option<SetupFocus> {
        if let Some((group, at)) = pick_of(tag, PICK) {
            return Some(SetupFocus::Pick(group, at));
        }
        slot_of(tag, SLOT).map(SetupFocus::Slot)
    }
}

/// One icon of the grid: what it puts on the bar, how it shows, and
/// whether it is a power hidden from the power wheel.
type Choice<'v> = (BarContent, &'v WheelEntry, bool);

/// What the pointer drags: an icon of the grid, or what a slot holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dragged {
    Pick(BarContent),
    Slot(usize, BarContent),
}

/// A drag under way: what is dragged, its icon, and where the pointer is
/// (layout pixels).
struct Drag {
    what: Dragged,
    icon: String,
    at: [f32; 2],
}

/// The slot that the control `tag` of the slots named `prefix` is.
fn slot_of(tag: &str, prefix: &str) -> Option<usize> {
    tag.strip_prefix(prefix)?.parse().ok()
}

/// The group and the place of the grid icon `tag`, of the controls named
/// `prefix`.
fn pick_of(tag: &str, prefix: &str) -> Option<(usize, usize)> {
    let (group, at) = tag.strip_prefix(prefix)?.split_once('_')?;
    Some((group.parse().ok()?, at.parse().ok()?))
}

/// How many slots the page shows. While a controller plays (`pad`), it
/// shows the first four. While the keyboard plays, it shows all six.
fn shown_slots(pad: bool) -> usize {
    if pad {
        CROSS_SLOTS
    } else {
        SETUP_SLOTS as usize
    }
}

/// What the page's layout is shaped for. `counts` is how many icons each
/// group has. `pad` is true while a controller plays, and the slots then
/// stand in a cross.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Shape {
    counts: [usize; 3],
    pad: bool,
}

/// The icons of each group, in the order `GROUPS` names them.
fn picks(view: &BarSetupView) -> [Vec<Choice<'_>>; 3] {
    fn plain((content, entry): &(BarContent, WheelEntry)) -> Choice<'_> {
        (*content, entry, false)
    }
    [
        view.powers
            .iter()
            .map(|(content, entry, hidden)| (*content, entry, *hidden))
            .collect(),
        view.feats.iter().map(plain).collect(),
        view.items.iter().map(plain).collect(),
    ]
}

/// The action bar setup page, on the abilities page's KOTOR 2 layout. The
/// member's bar slots stand at the top left. While the keyboard plays, all
/// six slots stand in a row. While a controller plays, only the first four
/// slots show, in a cross like the controller's face buttons. Slot 0 (A)
/// is at the bottom, slot 1 (B) at the right, slot 2 (X) at the left and
/// slot 3 (Y) at the top. Slots 4 and 5 are then not shown, and nothing
/// can be put in them on this page. Under the slots,
/// everything the member can put on the bar stands as small icons in a
/// grid, grouped under the labels "Force powers", "Feat attacks" and
/// "Items". The info box at the right tells of the icon or slot under the
/// pointer: its name, and the game's description of it. When the pointer is
/// over neither, the box holds the help.
///
/// On this page, an icon drawn grey and faded is a power hidden from the
/// power wheel. Nothing else is drawn that way here, so a slot or an icon
/// that the character cannot use now is drawn in full colour.
///
/// - With the mouse, the player drags an icon onto a slot to put it there.
///   A filled slot dragged onto another slot moves there, and the two slots
///   swap when the other was filled. A filled slot dragged anywhere else is
///   cleared. A right click on a power hides it from the power wheel or
///   shows it again.
/// - On the keyboard, a slot's key puts the icon under the pointer (or with
///   the focus) in that slot. With a slot focused, a slot's key or Delete
///   clears it. H hides the focused power or shows it again.
/// - On a controller, A on an icon chooses it, and A on a slot then puts it
///   there. B lets go of the chosen icon. X on a slot clears it. Y hides
///   the focused power or shows it again.
///
/// The grid is laid out for the icons the member has. When their number
/// changes, or when the player switches between the keyboard and a
/// controller, the page shapes its layout again (`Screen::reshapes`). The
/// member shown follows the menu bar's portraits. When the menu shows
/// another member, the page forgets the chosen icon and any drag. Only a
/// player who dresses the member (`dresses`) changes its bar.
///
/// Back (Escape, or B with no icon chosen) returns to the abilities page,
/// as does the menu bar's abilities tab.
#[derive(Default)]
pub struct ActionBar {
    /// What the layout was last shaped for, and what it would be shaped for
    /// now. When the two differ, the page shapes its layout again.
    shaped: Shape,
    now: Shape,
    /// The first icon of the grid, which has the focus when the page opens.
    first: String,
    focus: Option<SetupFocus>,
    /// What the info box tells of. The pointer sets it to what is under it,
    /// and the keys and a controller set it to what they focus.
    named: Option<SetupFocus>,
    /// The keys or a controller moved the focus since the pointer last
    /// moved.
    keyed: bool,
    /// The icon chosen with A or Enter, waiting for a slot.
    held: Option<BarContent>,
    drag: Option<Drag>,
    /// The grid icon or slot under the pointer this frame.
    under: Option<SetupFocus>,
    /// The member whose bar the page showed last (`BarSetupView::who`).
    who: Option<u32>,
}

impl ActionBar {
    /// The page for the view `vm` holds when it opens, its layout shaped
    /// for that view's icons and for what the player plays with.
    pub fn new(vm: &ViewModel) -> ActionBar {
        let mut page = ActionBar::default();
        page.cursor_to(vm);
        page.shaped = page.now;
        page
    }

    /// The grid icon at `group` and `at`.
    fn pick<'v>(view: &'v BarSetupView, (group, at): (usize, usize)) -> Option<Choice<'v>> {
        picks(view)[group].get(at).copied()
    }

    /// The focused grid icon.
    fn chosen<'v>(&self, view: &'v BarSetupView) -> Option<Choice<'v>> {
        ActionBar::icon_at(view, self.focus)
    }

    /// The grid icon that `place` is, if it is one.
    fn icon_at(view: &BarSetupView, place: Option<SetupFocus>) -> Option<Choice<'_>> {
        match place? {
            SetupFocus::Pick(group, at) => ActionBar::pick(view, (group, at)),
            SetupFocus::Slot(_) => None,
        }
    }

    /// Sends `edit` for the member shown, when this player dresses it.
    fn send(vm: &ViewModel, edit: BarEdit, out: &mut Vec<Intent>) {
        if let Some(view) = vm.bar_setup.as_ref().filter(|_| dresses(vm)) {
            out.push(Intent::Game(GameAction::Bar {
                who: view.who,
                edit,
            }));
        }
    }

    /// Puts the focused grid icon in `slot`. A slot that the page does not
    /// show is left alone.
    fn put(&self, slot: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        if slot >= shown_slots(vm.pad_playing) {
            return;
        }
        let chosen = vm.bar_setup.as_ref().and_then(|view| self.chosen(view));
        if let Some((content, ..)) = chosen {
            let slot = slot as u8;
            ActionBar::send(vm, BarEdit::Put { slot, content }, out);
        }
    }

    /// Empties the focused slot.
    fn clear(&self, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Some(SetupFocus::Slot(slot)) = self.focus {
            let slot = slot as u8;
            ActionBar::send(vm, BarEdit::Clear { slot }, out);
        }
    }

    /// Hides the power at `place` from the power wheel, or shows it again.
    /// Anything else at `place` is left alone. True when `place` is a power.
    fn hide(&self, place: Option<SetupFocus>, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let chosen = vm
            .bar_setup
            .as_ref()
            .and_then(|view| ActionBar::icon_at(view, place));
        if let Some((BarContent::Power(row), _, hidden)) = chosen {
            ActionBar::send(vm, BarEdit::Hide { row, hide: !hidden }, out);
            return true;
        }
        false
    }

    /// What a drag of `what` does when it is let go over the control
    /// `under`. A grid icon let go over a slot goes in it. A slot's content
    /// let go over another slot moves there, and the game swaps the two
    /// slots when the other was filled (`BarEdit::Put`). A slot's content
    /// let go anywhere but a slot is cleared from its slot. A slot that the
    /// page does not show counts as no slot.
    ///
    /// When the source slot no longer holds what was picked up, because the
    /// game changed it during the drag, the drop does nothing.
    fn drop(what: Dragged, under: Option<&str>, vm: &ViewModel, out: &mut Vec<Intent>) {
        if let Dragged::Slot(slot, content) = what {
            let now = vm
                .bar_setup
                .as_ref()
                .and_then(|view| view.contents.get(slot).copied().flatten());
            if now != Some(content) {
                return;
            }
        }
        let target = under
            .and_then(|tag| slot_of(tag, SLOT))
            .filter(|slot| *slot < shown_slots(vm.pad_playing));
        let edit = match (what, target) {
            (Dragged::Pick(content), Some(slot)) | (Dragged::Slot(_, content), Some(slot)) => {
                BarEdit::Put {
                    slot: slot as u8,
                    content,
                }
            }
            (Dragged::Slot(slot, _), None) => BarEdit::Clear { slot: slot as u8 },
            (Dragged::Pick(_), None) => return,
        };
        if !matches!((what, edit), (Dragged::Slot(from, _), BarEdit::Put { slot, .. }) if from == usize::from(slot))
        {
            ActionBar::send(vm, edit, out);
        }
    }

    /// The grid icon of `content`.
    fn choice_of(view: &BarSetupView, content: BarContent) -> Option<Choice<'_>> {
        picks(view)
            .into_iter()
            .flatten()
            .find(|(held, ..)| *held == content)
    }

    /// What the info box tells of: what is dragged while a drag goes on,
    /// else the grid icon or the slot that the name follows (`named`). A
    /// slot tells of the grid icon of what it holds.
    fn told<'v>(&self, view: &'v BarSetupView) -> Option<Choice<'v>> {
        if let Some(drag) = &self.drag {
            let (Dragged::Pick(content) | Dragged::Slot(_, content)) = drag.what;
            return ActionBar::choice_of(view, content);
        }
        match self.named? {
            SetupFocus::Pick(group, at) => ActionBar::pick(view, (group, at)),
            SetupFocus::Slot(slot) => ActionBar::choice_of(view, (*view.contents.get(slot)?)?),
        }
    }

    /// The info box's body for `choice`. A power hidden from the wheel
    /// starts with one sentence that says so, so a long description cannot
    /// push it out of view. The game's description follows after a blank
    /// line. Without either, the body is empty and the box shows only the
    /// name.
    fn body((_, entry, hidden_from_wheel): Choice) -> Vec<u8> {
        let hidden = hidden_from_wheel.then_some(HIDDEN_FROM_WHEEL.as_bytes());
        let parts = hidden
            .into_iter()
            .chain([entry.description.as_slice()])
            .filter(|part| !part.is_empty());
        parts.collect::<Vec<_>>().join(&b"\n\n"[..])
    }

    /// The help in the info box: how to use the page, or why the page
    /// offers nothing.
    fn help(vm: &ViewModel, view: &BarSetupView) -> &'static str {
        if !dresses(vm) {
            SETUP_OTHERS
        } else if view.powers.is_empty() && view.feats.is_empty() && view.items.is_empty() {
            SETUP_NOTHING
        } else {
            SETUP_HELP
        }
    }

    /// What the info box tells a player with a controller to press, as the
    /// first paragraph of its body. A controller cannot drag, so the page
    /// says which button chooses an icon and which puts it in a slot. The
    /// buttons are named as the controller in use names them. None while
    /// the keyboard plays, and none on a bar that another player sets up.
    fn pad_hint(&self, vm: &ViewModel) -> Option<String> {
        if !vm.pad_playing || !dresses(vm) {
            return None;
        }
        let button = |name: &str| crate::screens::key_mapping::chord_name(name, vm);
        let (choose, cancel) = (button("South"), button("East"));
        if self.held.is_some() {
            return Some(format!(
                "Move to a slot and press {choose} to put it there. Press {cancel} to cancel."
            ));
        }
        let put = format!(
            "Press {choose} to choose this. Then move to a slot and press {choose} to put it there."
        );
        Some(match self.focus {
            Some(SetupFocus::Pick(0, _)) => format!(
                "{put} Press {} to hide this power from the wheel or to show it again.",
                button("North")
            ),
            Some(SetupFocus::Slot(_)) => format!("Press {} to empty this slot.", button("West")),
            _ => put,
        })
    }
}

impl Page for ActionBar {
    const NAME: &'static str = "action_bar";
    const LAYOUT: &'static str = "abilities";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        let view = vm.bar_setup.as_ref().unwrap_or(&NO_SETUP);
        // Slots 4 and 5 are hidden while a controller plays.
        let slots = &view.slots[..view.slots.len().min(shown_slots(vm.pad_playing))];
        if let Some(slot) = slot_of(tag, SLOT) {
            return shown(slot < slots.len());
        }
        if let Some(slot) = slot_of(tag, SLOT_ICON) {
            return match slots.get(slot) {
                // The setup page draws nothing in the disabled look for
                // being unusable now. That look means hidden from the wheel
                // here, and only a grid icon shows it.
                Some(BarSlot {
                    icon: Some(icon), ..
                }) => image(Some(icon.clone())),
                _ => hidden(),
            };
        }
        if let Some(place) = pick_of(tag, PICK) {
            return match ActionBar::pick(view, place) {
                // The icon chosen with A waits for a slot in the lit look.
                Some((content, ..)) => ControlState {
                    selected: self.held == Some(content),
                    ..ControlState::default()
                },
                None => hidden(),
            };
        }
        if let Some(place) = pick_of(tag, PICK_ICON) {
            return match ActionBar::pick(view, place) {
                Some((_, entry, hidden_from_wheel)) => ControlState {
                    fill: Some(entry.icon.clone()),
                    dimmed: hidden_from_wheel,
                    ..ControlState::default()
                },
                None => hidden(),
            };
        }
        if let Some(group) = slot_of(tag, GROUP_LABEL) {
            let shows = picks(view)
                .get(group)
                .is_some_and(|icons| !icons.is_empty());
            return if shows { text(GROUPS[group]) } else { hidden() };
        }
        if UNUSED.contains(&tag) {
            return hidden();
        }
        match tag {
            "LBL_ABILITIES" => text(SETUP_TITLE),
            "LBL_NAME" => ControlState {
                text: Some(
                    self.told(view)
                        .map_or(Vec::new(), |(_, entry, _)| entry.name.clone()),
                ),
                ..ControlState::default()
            },
            "LB_DESC" => {
                let hint = self.pad_hint(vm).map(String::into_bytes);
                let body = match self.told(view) {
                    Some(choice) => ActionBar::body(choice),
                    // The controller's own instructions stand in for the
                    // help, which tells of dragging.
                    None if hint.is_some() => Vec::new(),
                    None => ActionBar::help(vm, view).as_bytes().to_vec(),
                };
                let parts: Vec<&[u8]> = hint
                    .iter()
                    .map(Vec::as_slice)
                    .chain([body.as_slice()])
                    .filter(|part| !part.is_empty())
                    .collect();
                description(&parts.join(&b"\n\n"[..]))
            }
            _ => ControlState::default(),
        }
    }

    /// A grid icon activated with A or Enter is chosen, to be put in the
    /// slot activated next. A click that starts a drag chooses nothing.
    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if self.drag.is_some() {
            return;
        }
        let view = vm.bar_setup.as_ref();
        match SetupFocus::of(tag) {
            Some(SetupFocus::Pick(group, at)) => {
                self.held = view
                    .and_then(|view| ActionBar::pick(view, (group, at)))
                    .map(|(content, ..)| content);
            }
            Some(SetupFocus::Slot(slot)) => {
                if let Some(content) = self.held.take() {
                    let slot = slot as u8;
                    ActionBar::send(vm, BarEdit::Put { slot, content }, out);
                }
            }
            None => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some(self.first.as_str())
    }

    fn reshapes(&mut self) -> bool {
        let changed = self.shaped != self.now;
        self.shaped = self.now;
        changed
    }

    /// The stack asks this each frame, before the frame's input. The page
    /// uses it to count the icons of each group, to note whether a
    /// controller plays, and to notice that the menu shows another member.
    /// When the member changes, the page forgets the chosen icon and any
    /// drag. It never moves the cursor itself.
    fn cursor_to(&mut self, vm: &ViewModel) -> Option<(&'static str, usize)> {
        let view = vm.bar_setup.as_ref();
        self.now = Shape {
            counts: view.map_or([0; 3], |view| picks(view).map(|icons| icons.len())),
            pad: vm.pad_playing,
        };
        // With no icons yet, the first slot, which every shape has.
        self.first = (self.now.counts.iter().position(|count| *count > 0))
            .map_or(format!("{SLOT}0"), |group| format!("{PICK}{group}_0"));
        let who = view.map(|view| view.who);
        if self.who.is_some() && self.who != who {
            self.held = None;
            self.drag = None;
        }
        self.who = who;
        None
    }

    fn focused(&mut self, tag: &str, _: &ViewModel) {
        self.focus = SetupFocus::of(tag);
        if self.keyed {
            self.named = self.focus;
        }
    }

    /// A press starts a drag of the grid icon or the filled slot under the
    /// pointer. The drag follows the pointer while the button is held, and
    /// letting go drops it (`drop`).
    fn pointer(&mut self, pointer: Pointer<'_>, vm: &ViewModel, out: &mut Vec<Intent>) {
        self.under = pointer.at.and(pointer.under).and_then(SetupFocus::of);
        // A pointer resting in the window leaves the info box to the keys
        // and the controller. Only a pointer that moved takes it back.
        if pointer.moved {
            self.keyed = false;
            self.named = self.under;
        }
        let view = vm.bar_setup.as_ref();
        if pointer.press
            && let (Some(view), Some(at), Some(under)) = (view, pointer.at, pointer.under)
        {
            let what = match SetupFocus::of(under) {
                Some(SetupFocus::Pick(group, place)) => ActionBar::pick(view, (group, place))
                    .map(|(content, entry, _)| (Dragged::Pick(content), entry.icon.clone())),
                Some(SetupFocus::Slot(slot)) => {
                    let content = view.contents.get(slot).copied().flatten();
                    let icon = view.slots.get(slot).and_then(|slot| slot.icon.clone());
                    content
                        .zip(icon)
                        .map(|(content, icon)| (Dragged::Slot(slot, content), icon))
                }
                None => None,
            };
            self.drag = what.map(|(what, icon)| Drag { what, icon, at });
        }
        if let (Some(drag), Some(at)) = (self.drag.as_mut(), pointer.at) {
            drag.at = at;
        }
        if !pointer.held
            && let Some(drag) = self.drag.take()
        {
            ActionBar::drop(drag.what, pointer.under, vm, out);
        }
    }

    /// A controller's X, Y and B on the page, and the moves that make the
    /// info box follow the focus.
    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        match (key, self.focus) {
            (Key::Primary, Some(SetupFocus::Slot(_))) => self.clear(vm, out),
            (Key::Second, Some(SetupFocus::Pick(..))) => {
                self.hide(self.focus, vm, out);
            }
            (Key::Back, _) if self.held.is_some() => self.held = None,
            (Key::Back, _) => out.push(Intent::Swap(<Abilities as Page>::NAME)),
            (Key::Up | Key::Down | Key::Left | Key::Right, _) => {
                self.keyed = true;
                return false;
            }
            _ => return false,
        }
        true
    }

    /// The keyboard's slot keys, Delete, H and the right mouse button on the
    /// page.
    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let keys = vm.bar_setup.as_ref().map_or(&[][..], |view| &view.keys[..]);
        let slot = keys
            .iter()
            .position(|bound| !bound.is_empty() && bound == key);
        match (slot, key, self.focus) {
            (Some(_), _, Some(SetupFocus::Slot(_)))
            | (None, "Delete", Some(SetupFocus::Slot(_))) => {
                self.clear(vm, out);
            }
            (Some(slot), _, Some(SetupFocus::Pick(..))) => self.put(slot, vm, out),
            (None, "KeyH", Some(SetupFocus::Pick(..))) => {
                self.hide(self.focus, vm, out);
            }
            // A right click acts on what is under the pointer this frame,
            // which may not have the focus yet. Over anything but a power,
            // it does nothing and leaves the frame's other input alone.
            (None, "MouseRight", _) => return self.hide(self.under, vm, out),
            _ => return false,
        }
        true
    }

    /// Each slot's key at its top and the uses left at its bottom right, as
    /// the HUD draws them, each item's uses left on its grid icon, and the
    /// dragged icon under the pointer.
    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        let Some(view) = vm.bar_setup.as_ref() else {
            return Vec::new();
        };
        let mut out = DrawList::default();
        // A slot whose kind of item has two or more different items carried
        // has frames stacked behind it, as on the HUD. The setup page never
        // draws a slot dimmed, and the frame keeps its focused look. Slots 4
        // and 5 are not drawn while a controller plays.
        let slots: Vec<_> = (view.slots.iter().enumerate())
            .take(shown_slots(vm.pad_playing))
            .filter_map(|(at, slot)| {
                let frame = gui.find(&format!("{SLOT}{at}"))?;
                let icon = gui.find(&format!("{SLOT_ICON}{at}"))?;
                let shown = BarSlot {
                    dim: false,
                    ..slot.clone()
                };
                let mut framed = self.state(frame, vm);
                framed.focused = self.focus == Some(SetupFocus::Slot(at));
                Some((shown, [(frame, framed), (icon, self.state(icon, vm))]))
            })
            .collect();
        crate::screens::hud::slot_stacks(
            slots.iter().map(|(slot, drawn)| (slot, drawn.clone())),
            painter,
            &mut out,
        );
        let frames = slots
            .iter()
            .map(|(slot, [(frame, _), _])| (slot, rect_of(frame)));
        crate::screens::hud::slot_marks(frames, painter, &mut out);
        let counted: Vec<(BarSlot, [f32; 4])> = picks(view)
            .iter()
            .enumerate()
            .flat_map(|(group, icons)| {
                icons
                    .iter()
                    .enumerate()
                    .map(move |(at, icon)| (group, at, icon))
            })
            .filter_map(|(group, at, (_, entry, _))| {
                let frame = gui.find(&format!("{PICK}{group}_{at}"))?;
                let count = entry.count?;
                let slot = BarSlot {
                    count: Some(count),
                    ..BarSlot::default()
                };
                Some((slot, rect_of(frame)))
            })
            .collect();
        crate::screens::hud::slot_marks(
            counted.iter().map(|(slot, rect)| (slot, *rect)),
            painter,
            &mut out,
        );
        if let Some(drag) = &self.drag {
            // As large as a slot's icon, where it is about to go.
            let side = gui
                .find(&format!("{SLOT_ICON}0"))
                .map_or(PICK_SIDE, |icon| icon.extent.width) as f32;
            let rect = [drag.at[0] - side / 2.0, drag.at[1] - side / 2.0, side, side];
            painter.image(Source::Campaign, &drag.icon, rect, [1.0; 4], &mut out);
        }
        out.quads
    }

    /// KOTOR 2's stretch to a wide window keeps each picture's shape but
    /// moves the controls apart. That opens gaps in the grid and leaves room
    /// unused between the grid and the info box. The page therefore lays its
    /// slots and its grid out again at the size it is drawn. The grid then
    /// holds as many icons in a row as fit before the info box.
    fn fit_drawn(&mut self, gui: &mut Gui) {
        let extent = |tag: &str| gui.find(tag).map(|control| control.extent);
        let (Some(area), Some(desc), Some(slot), Some(slot_icon)) = (
            extent("LB_ABILITY"),
            extent("LB_DESC"),
            extent(&format!("{SLOT}0")),
            extent(&format!("{SLOT_ICON}0")),
        ) else {
            return;
        };
        let square = gui
            .controls
            .iter()
            .find(|control| pick_of(&control.tag, PICK).is_some())
            .map_or(slot_icon.width, |control| control.extent.width);
        let label = (gui.controls.iter())
            .find(|control| slot_of(&control.tag, GROUP_LABEL).is_some())
            .map_or(LABEL_TALL, |control| control.extent.height);
        // The layout's own gaps, at the size the grid's icons are drawn.
        let scaled = |size: i32| size * square / PICK_SIDE;
        let grid = Grid {
            left: area.left,
            top: area.top,
            wide: desc.left - scaled(DESC_MARGIN) - area.left,
            slot: [slot.width, slot.height],
            slot_gap: scaled(SLOT_GAP),
            cross_gap: scaled(CROSS_GAP),
            side: square,
            label,
            gap: scaled(GROUP_GAP),
        };
        let placed = grid.lay_out(self.shaped);
        // An icon moves with its frame. A group label is placed itself.
        let frame_of = |tag: &str| match slot_of(tag, GROUP_LABEL) {
            Some(_) => tag.to_string(),
            None => tag.replacen("LBL_", "BTN_", 1),
        };
        let moves: Vec<(usize, Rect)> = (gui.controls.iter().enumerate())
            .filter_map(|(at, control)| {
                let (_, to) = placed
                    .iter()
                    .find(|(tag, _)| *tag == frame_of(&control.tag))?;
                if control.tag.starts_with("LBL_") && slot_of(&control.tag, GROUP_LABEL).is_none() {
                    // An icon moves with its frame and keeps its size.
                    let from = extent(&frame_of(&control.tag))?;
                    let Rect { left, top, .. } = control.extent;
                    let moved = Rect {
                        left: left + to.left - from.left,
                        top: top + to.top - from.top,
                        ..control.extent
                    };
                    return Some((at, moved));
                }
                Some((at, *to))
            })
            .collect();
        for (at, to) in moves {
            gui.controls[at].extent = to;
        }
    }

    /// Lays the page out on the abilities page's layout.
    ///
    /// - The band where the abilities page's tab buttons stand is closed.
    ///   The slots, the grid and the info box move up under the title's
    ///   line, and the info box grows down to where the feats' description
    ///   box ends.
    /// - The slots stand at the left, in the HUD action slot's shape. While
    ///   the keyboard plays, all six stand in a row. While a controller
    ///   plays, the first four stand in a cross and slots 4 and 5 are left
    ///   out.
    /// - Under the slots stand each group's label and its icons, in a grid
    ///   that shares the slots' left edge. The grid starts below the whole
    ///   cross, not below one slot. The icons touch. A group with no icons
    ///   takes no room.
    /// - The info box (the name and the description) stays at the right.
    fn shape(&self, gui: &mut Gui) {
        let (Some(list), Some(desc), Some(band)) = (
            gui.find("LB_ABILITY").cloned(),
            gui.find("LB_DESC").cloned(),
            gui.find("BTN_SKILLS").cloned(),
        ) else {
            return;
        };
        // How far the content moves up to close the tab band, and how far
        // the description box grows down.
        let lift = match (gui.find("LBL_BAR1"), gui.find("LBL_BAR2")) {
            (Some(title_line), Some(band_line)) => band_line.extent.top - title_line.extent.top,
            _ => 0,
        };
        let grow = gui.find("LB_DESC_FEATS").map_or(0, |feats| {
            let bottom = |rect: Rect| rect.top + rect.height;
            (bottom(feats.extent) - bottom(desc.extent)).max(0)
        });
        for control in &mut gui.controls {
            match control.tag.as_str() {
                "LB_ABILITY" => moved(control, lift, lift),
                "LB_DESC" => moved(control, lift, lift + grow),
                "LBL_NAME" => moved(control, lift, 0),
                _ => {}
            }
        }
        let parent = list.parent;
        let grid = Grid {
            left: list.extent.left,
            top: list.extent.top - lift,
            wide: desc.extent.left - DESC_MARGIN - list.extent.left,
            slot: [SLOT_WIDE, SLOT_WIDE * HUD_SLOT_SIZE[1] / HUD_SLOT_SIZE[0]],
            slot_gap: SLOT_GAP,
            cross_gap: CROSS_GAP,
            side: PICK_SIDE,
            label: LABEL_TALL,
            gap: GROUP_GAP,
        };
        let mut id = next_id(gui);
        let mut push = |gui: &mut Gui, control: Control| {
            gui.controls.push(Control {
                id,
                parent,
                ..control
            });
            id += 1;
        };
        use fmt_odyssey::gui::align;
        let picture = |fill: &str| Border {
            fill: fill.into(),
            fill_style: STRETCHED,
            ..Border::default()
        };
        let square = |color: [f32; 3]| Border {
            corner: PICK_FRAME.0.into(),
            edge: PICK_FRAME.1.into(),
            fill_style: STRETCHED,
            dimension: PICK_FRAME.2,
            color: Some(color),
            ..Border::default()
        };
        let framed = |tag: String, extent: Rect, [border, hilight]: [Border; 2]| Control {
            kind: ControlKind::Button,
            tag,
            extent,
            border: Some(border),
            hilight: Some(hilight),
            ..Control::default()
        };
        let label = |tag: String, extent: Rect| Control {
            kind: ControlKind::Label,
            tag,
            extent,
            text: band.text.clone().map(|text| fmt_odyssey::gui::Text {
                alignment: align::LEFT | align::MIDDLE,
                ..text
            }),
            ..Control::default()
        };
        let [icon_left, icon_top, icon_side] =
            HUD_SLOT_ICON.map(|size| size * SLOT_WIDE / HUD_SLOT_SIZE[0]);
        for (tag, extent) in grid.lay_out(self.shaped) {
            let inside = |left: i32, top: i32, side: i32| Rect {
                left: extent.left + left,
                top: extent.top + top,
                width: side,
                height: side,
            };
            let (icon, at) = if let Some(at) = slot_of(&tag, SLOT) {
                let frames = [picture(HUD_SLOT.0), picture(HUD_SLOT.1)];
                push(gui, framed(tag, extent, frames));
                (
                    format!("{SLOT_ICON}{at}"),
                    inside(icon_left, icon_top, icon_side),
                )
            } else if let Some((group, at)) = pick_of(&tag, PICK) {
                let frames = PICK_COLOR.map(square);
                push(gui, framed(tag, extent, frames));
                let side = PICK_SIDE - 2 * PICK_INSET;
                (
                    format!("{PICK_ICON}{group}_{at}"),
                    inside(PICK_INSET, PICK_INSET, side),
                )
            } else {
                push(gui, label(tag, extent));
                continue;
            };
            push(
                gui,
                Control {
                    kind: ControlKind::Label,
                    tag: icon,
                    extent: at,
                    ..Control::default()
                },
            );
        }
    }
}

/// Moves `control` up by `up` and makes it `grow` taller, with its list's
/// row prototype and scrollbar.
fn moved(control: &mut Control, up: i32, grow: i32) {
    control.extent.top -= up;
    control.extent.height += grow;
    for part in control.proto.iter_mut().chain(control.scrollbar.iter_mut()) {
        part.extent.top -= up;
    }
    if let Some(bar) = control.scrollbar.as_mut() {
        bar.extent.height += grow;
    }
}

/// The room the page lays its slots and its grid out in, and their sizes,
/// in the pixels of the layout as it is shaped or as it is drawn.
struct Grid {
    /// The shared left edge of the slots and the grid, and the slots' top.
    left: i32,
    top: i32,
    /// How wide the grid may be.
    wide: i32,
    /// A slot's width and height. `slot_gap` is the gap between two slots
    /// of the row, and `cross_gap` the gap between two slots of the cross.
    slot: [i32; 2],
    slot_gap: i32,
    cross_gap: i32,
    /// A grid icon's side, a group label's height, and the gap above each
    /// group.
    side: i32,
    label: i32,
    gap: i32,
}

impl Grid {
    /// Where each slot frame, group label and grid frame goes, by its tag,
    /// for the shape `shape`. While a controller plays, the first four
    /// slots stand in a cross and slots 4 and 5 get no place. While the
    /// keyboard plays, all six slots stand in a row. Each group has as many
    /// icons as `shape.counts` says. A row of the grid holds as many icons
    /// as fit in `wide`.
    fn lay_out(&self, shape: Shape) -> Vec<(String, Rect)> {
        let rect = |left: i32, top: i32, width: i32, height: i32| Rect {
            left,
            top,
            width,
            height,
        };
        let [slot_wide, slot_tall] = self.slot;
        // Each slot's left and top from the slots' top left corner, and
        // how tall the slots stand together.
        let (places, slots_tall): (Vec<[i32; 2]>, i32) = if shape.pad {
            let (places, [_, tall]) = cross(self.slot, self.cross_gap);
            (places.to_vec(), tall)
        } else {
            let row = (0..SETUP_SLOTS).map(|at| [at * (slot_wide + self.slot_gap), 0]);
            (row.collect(), slot_tall)
        };
        let mut placed: Vec<(String, Rect)> = (places.iter().enumerate())
            .map(|(at, &[left, top])| {
                (
                    format!("{SLOT}{at}"),
                    rect(self.left + left, self.top + top, slot_wide, slot_tall),
                )
            })
            .collect();
        let columns = (self.wide / self.side).max(1);
        let mut y = self.top + slots_tall + self.gap;
        let counts = shape.counts;
        for (group, count) in counts.iter().enumerate().filter(|(_, count)| **count > 0) {
            let tag = format!("{GROUP_LABEL}{group}");
            placed.push((tag, rect(self.left, y, columns * self.side, self.label)));
            y += self.label;
            for at in 0..*count as i32 {
                let (column, line) = (at % columns, at / columns);
                let left = self.left + column * self.side;
                let frame = rect(left, y + line * self.side, self.side, self.side);
                placed.push((format!("{PICK}{group}_{at}"), frame));
            }
            let lines = (*count as i32 + columns - 1) / columns;
            y += lines * self.side + self.gap;
        }
        placed
    }
}
