//! The action bar setup screen's view (`ui::screens::pages::ActionBar`).
//! While the screen is open, the view holds the bar of the party member the
//! menu shows, and everything that member can put on it
//! (`sim::combat::bar::assignable`). The icons, names and counts are the
//! ones the HUD and the power wheel show (`hud::setup_slots`,
//! `hud::bar_entry`).
//!
//! The screen's edits go to the host as `sim::action::Action::Bar`
//! (`edit_of`). The host refuses an edit for a member this player does not
//! dress, as it refuses equipping one.

use bevy::prelude::*;
use sim::Snapshot;
use sim::combat::bar::{Content, FeatLine, Rules};
use sim::combat::items::Shelf;
use ui::intent::{BarContent, BarEdit};
use ui::screens::tab::Page;
use ui::vm::BarSetupView;

use crate::bar_view::Means;
use crate::pad::Prompts;
use crate::pick_view::Picking;
use crate::settings::BAR;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

pub struct BarSetupViewPlugin;

impl Plugin for BarSetupViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.before(crate::ui_view::Stepped));
    }
}

/// The screen's name for `content`.
fn named(content: Content) -> BarContent {
    match content {
        Content::Power(row) => BarContent::Power(row),
        Content::Item(shelf) => BarContent::Item(shelf as u8),
        Content::Mine => BarContent::Mine,
        Content::Feat(line) => BarContent::Feat(line as u8),
        Content::Stealth => BarContent::Stealth,
    }
}

/// The content that the screen's `content` names. There is none for an item
/// kind or a feat attack line past the end of the game's lists.
fn content_of(content: BarContent) -> Option<Content> {
    Some(match content {
        BarContent::Power(row) => Content::Power(row),
        BarContent::Item(at) => Content::Item(*Shelf::ALL.get(usize::from(at))?),
        BarContent::Mine => Content::Mine,
        BarContent::Feat(at) => Content::Feat(*FeatLine::ALL.get(usize::from(at))?),
        BarContent::Stealth => Content::Stealth,
    })
}

/// The sim's edit for the screen's `edit`.
pub(crate) fn edit_of(edit: BarEdit) -> Option<sim::action::BarEdit> {
    Some(match edit {
        BarEdit::Put { slot, content } => sim::action::BarEdit::Put {
            slot,
            content: content_of(content)?,
        },
        BarEdit::Clear { slot } => sim::action::BarEdit::Clear { slot },
        BarEdit::Hide { row, hide } => sim::action::BarEdit::Hide { row, hide },
    })
}

/// The screen's view of the character `means` describes. `picking` holds
/// the item picks this player asked for that the host has not taken yet.
fn view(
    ui: &mut Ui,
    (snapshot, library): (&Snapshot, Option<&std::sync::Arc<sim::script::Library>>),
    (means, rules): (&Means, &Rules),
    picking: Option<&Picking>,
    prompts: &Prompts,
) -> BarSetupView {
    let identity = (means.me, &means.sheet, means.fighter);
    let chosen = crate::hud::chosen_items(snapshot, identity, library, picking);
    let kits = library.map_or(0, |library| {
        sim::traps::party_kits(&snapshot.goods, library.items())
    });
    let labels = BAR
        .iter()
        .map(|key| {
            prompts
                .bound_label(*key)
                .unwrap_or(ui::vm::KeyLabel::default())
        })
        .collect();
    let mut view = BarSetupView {
        who: means.me.0,
        slots: crate::hud::setup_slots(ui, means, &chosen, labels),
        keys: BAR.iter().map(|key| prompts.raw_key(*key)).collect(),
        ..BarSetupView::default()
    };
    for content in &means.assignable {
        let entry = crate::hud::bar_entry(ui, *content, means, &chosen, kits);
        let name = named(*content);
        match content {
            Content::Power(row) => {
                let hidden = means.sheet.bar.hidden(&means.sheet, rules, *row);
                view.powers.push((name, entry, hidden));
            }
            Content::Feat(_) => view.feats.push((name, entry)),
            _ => view.items.push((name, entry)),
        }
    }
    view
}

/// Fills the view-model's bar setup while the screen is open, for the party
/// member the menu shows. The dump's `bar_setup` fact holds what it shows.
#[allow(clippy::type_complexity)]
fn show(
    ui: Option<ResMut<Ui>>,
    (presented, own, swap, picking): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
        Option<Res<Picking>>,
    ),
    prompts: Prompts,
    mut facts: ResMut<console::DumpFacts>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let ui = &mut *ui;
    let open = ui.stack.has(ui::screens::pages::ActionBar::NAME);
    let seen = presented.zip(own).zip(swap).filter(|_| open);
    let shown = seen.and_then(|((presented, own), swap)| {
        let snapshot = presented.0.current();
        let who = ui.vm.party.get(ui.vm.viewed)?.id;
        let body = snapshot.entities.iter().find(|entity| entity.id.0 == who)?;
        // Only the player's own body has picks the host has not taken yet.
        let own_body = snapshot.body(own.0).is_some_and(|mine| mine.id == body.id);
        let picking = picking.as_deref().filter(|_| own_body);
        let library = swap.level.scripts();
        crate::bar_view::with_bar_of(snapshot, body, &swap.level, |means, rules| {
            view(ui, (snapshot, library), (means, rules), picking, &prompts)
        })
    });
    ui.vm.bar_setup = shown;
    if facts.wanted() {
        let fact = ui.vm.bar_setup.as_ref().map(|view| {
            let entry = |content: &BarContent, entry: &ui::vm::WheelEntry| {
                let name = String::from_utf8_lossy(&entry.name).into_owned();
                (
                    format!("{content:?}"),
                    entry.icon.clone(),
                    name,
                    entry.count,
                )
            };
            let powers: Vec<_> = (view.powers.iter())
                .map(|(content, shown, hidden)| (entry(content, shown), hidden))
                .collect();
            let others = |list: &[(BarContent, ui::vm::WheelEntry)]| {
                let entries = list.iter().map(|(content, shown)| entry(content, shown));
                entries.collect::<Vec<_>>()
            };
            serde_json::json!({
                "who": view.who,
                "slots": view.slots.iter().map(|slot| slot.icon.clone()).collect::<Vec<_>>(),
                "keys": view.keys,
                "powers": powers,
                "feats": others(&view.feats),
                "items": others(&view.items),
            })
        });
        facts
            .0
            .insert("bar_setup".into(), fact.unwrap_or(serde_json::Value::Null));
    }
}
