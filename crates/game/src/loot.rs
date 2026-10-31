//! The loot window's side of the game (render side only: it reads the
//! presented snapshots and the module's facts):
//!
//! - the container this player holds open (its seat's bit among the
//!   container's tenants) is shown with what it holds as the snapshot's
//!   goods have it (`State::loot`: its template's items and what scripts
//!   gave it), each item by its name and icon, alike stackable items one
//!   stack as KOTOR's containers merge them (`stacks`); an item asked for
//!   shows faded until the snapshot has it taken or the host says why
//!   not;
//! - the window opens when the player holds a container open (using it,
//!   E) and closes when it no longer does (closed, out of reach, down);
//!   one this player just closed does not open again while the snapshot
//!   still has it open (a joined player's lags by a round trip), for at
//!   most `CLOSING_FRAMES`;
//! - in its give mode the party's items but the plot ones are listed
//!   (`LootView::give`), one given faded until the party no longer has it;
//! - the screens' game intents become the sim's actions, sent to the host
//!   (`Outgoing`), as every player's are;
//! - what the host says of them, and what scripts say to the player
//!   (`sim::notice::Notice`), is put in words for the feedback log.

use bevy::prelude::*;
use sim::action::Action;
use sim::module::ObjectKind;
use sim::notice::Notice;
use ui::intent::{GameAction, Intent};
use ui::skin::Source;
use ui::vm::{LootRow, LootView};

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::{Ui, UiIntents};

/// The player's actions waiting to go to the host.
#[derive(Resource, Default)]
pub struct Outgoing(pub Vec<Action>);

/// What the host told this player, waiting for the feedback log.
#[derive(Resource, Default)]
pub struct Told(pub Vec<String>);

/// The items this player asked for and has not seen taken yet, by
/// container.
#[derive(Resource, Default)]
struct Pending(Vec<(u32, u32)>);

/// The party's items this player put in a container and has not seen
/// leave the party yet.
#[derive(Resource, Default)]
struct Given(Vec<u32>);

/// The containers this player closed, not yet closed in the snapshot,
/// with the frames each is still waited for.
#[derive(Resource, Default)]
struct Closing(Vec<(u32, u32)>);

/// How long a close is waited for before the snapshot is believed again.
const CLOSING_FRAMES: u32 = 120;

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Outgoing>()
            .init_resource::<Told>()
            .init_resource::<Pending>()
            .init_resource::<Given>()
            .init_resource::<Closing>()
            .add_systems(
                Update,
                (
                    show.before(crate::ui_view::Stepped),
                    send.after(crate::ui_view::Stepped),
                ),
            );
    }
}

/// The sim's workbench for a screen's.
fn workbench(bench: ui::intent::BenchDesc) -> sim::action::Workbench {
    sim::action::Workbench {
        list: bench.list,
        lab: bench.lab,
        maker: bench.maker,
    }
}

/// The sim's action for a screen's; none for a side deck of a card the
/// sim does not know.
pub(crate) fn action(game: GameAction) -> Option<Action> {
    Some(match game {
        GameAction::Open(container) => Action::Open(container),
        GameAction::Close(container) => Action::Close(container),
        GameAction::Take { container, item } => Action::Take { container, item },
        GameAction::TakeAll(container) => Action::TakeAll(container),
        GameAction::Put { container, item } => Action::Put { container, item },
        GameAction::Buy { store, item } => Action::Buy { store, item },
        GameAction::Sell { store, item } => Action::Sell { store, item },
        GameAction::Leave(store) => Action::Leave(store),
        GameAction::Equip { who, item, slot } => Action::EquipOn { who, item, slot },
        GameAction::Unequip { who, slot } => Action::UnequipFrom { who, slot },
        GameAction::SwitchWeapons { who } => Action::SwitchWeaponsOn { who },
        GameAction::UseItem { who, item } => Action::UseItem {
            who,
            item,
            target: who,
        },
        GameAction::Assemble { item, changes } => Action::Assemble {
            item,
            changes: changes
                .into_iter()
                .map(|change| match change {
                    ui::intent::Fitting::In { upgrade, replace } => {
                        sim::action::Fitting::In { upgrade, replace }
                    }
                    ui::intent::Fitting::Out(row) => sim::action::Fitting::Out(row),
                })
                .collect(),
        },
        GameAction::Breakdown { bench, item } => Action::Breakdown {
            bench: workbench(bench),
            item,
        },
        GameAction::Create { bench, row } => Action::Create {
            bench: workbench(bench),
            row,
        },
        // The feats and powers the screen does not offer, the host picks.
        GameAction::LevelUp {
            who,
            class,
            ability,
            skills,
            feats,
            powers,
        } => Action::LevelUpOn {
            who,
            choices: sim::levels::Choices {
                class,
                ability,
                skills,
                feats,
                powers,
            },
        },
        GameAction::ChooseParty(picks) => Action::ChooseParty(picks),
        GameAction::ShowParty => Action::ShowParty,
        GameAction::ScreenOpened => Action::ScreenOpened,
        GameAction::Travel(planet) => Action::Travel(planet),
        GameAction::Break => Action::Break,
        GameAction::Resume => Action::Resume,
        GameAction::Pazaak(played) => Action::Pazaak(crate::pazaak_view::move_of(played)?),
        GameAction::StopWatching => Action::Watch { player: u32::MAX },
    })
}

/// The screens' game intents, to the host; a take is pending until seen.
fn send(
    mut intents: ResMut<UiIntents>,
    mut outgoing: ResMut<Outgoing>,
    mut pending: ResMut<Pending>,
    (mut closing, mut leaving): (ResMut<Closing>, ResMut<crate::merchant::Leaving>),
    mut given: ResMut<Given>,
) {
    intents.0.retain(|intent| match intent {
        Intent::Game(game) => {
            let Some(action) = action(game.clone()) else {
                return false;
            };
            match action {
                Action::Take { container, item } => pending.0.push((container, item)),
                Action::Put { item, .. } => given.0.push(item),
                Action::Close(container) => closing.0.push((container, CLOSING_FRAMES)),
                Action::Leave(store) => {
                    leaving.0 = Some((store, crate::merchant::LEAVING_FRAMES));
                }
                _ => {}
            }
            outgoing.0.push(action);
            false
        }
        _ => true,
    });
}

/// The open container as the screens show it, and the window opened or
/// closed to match.
fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut ui: ResMut<Ui>,
    (mut pending, mut closing, mut given): (ResMut<Pending>, ResMut<Closing>, ResMut<Given>),
    mut told: ResMut<Told>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let ui = &mut *ui;
    ui.vm.feedback.append(&mut told.0);
    let (Some(presented), Some(own), Some(swap)) = (presented, own, swap) else {
        return;
    };
    let Some(module) = swap.level.module() else {
        return;
    };
    if let Some(assets) = ui.assets.as_mut() {
        assets.enter_module(&module.name);
    }
    let bit = sim::party::bit(own.0);
    let snapshot = presented.0.current();
    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
    let avatar = snapshot.body(own.0);
    let (me, me_id) = (
        avatar.map(|entity| entity.position),
        avatar.map(|entity| entity.id.0),
    );
    let nearest = me.and_then(|me| {
        snapshot
            .entities
            .iter()
            .filter_map(|entity| match (entity.kind, entity.object) {
                (sim::Kind::Object(ObjectKind::Placeable, index), Some(object))
                    if objects.get(index).is_some_and(|data| data.container) =>
                {
                    let at = entity.position;
                    let distance = ((at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2)).sqrt();
                    Some((distance, index, object))
                }
                _ => None,
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    });
    // What a character wears: (slot, template) pairs.
    let worn_by = |who: u32| {
        snapshot
            .goods
            .items
            .iter()
            .filter(|(_, item)| item.holder == who)
            .map(|(_, item)| serde_json::json!([item.slot, item.template]))
            .collect::<Vec<_>>()
    };
    facts.0.insert(
        "loot".into(),
        serde_json::json!({
            "nearest_container": nearest.map(|(distance, index, object)| serde_json::json!({
                "index": index,
                "distance_m": distance,
                "open": object.state,
                "tenants": object.tenants,
            })),
            "party": snapshot
                .goods
                .carried(sim::inventory::PARTY)
                .map(|(_, item)| item.template.clone())
                .collect::<Vec<_>>(),
            "worn": me_id.map(worn_by),
            // The party's characters: level, who drives them, what they wear.
            "members": snapshot
                .entities
                .iter()
                .filter(|entity| snapshot.in_party(entity))
                .map(|entity| serde_json::json!({
                    "id": entity.id.0,
                    "level": crate::cast::sheet(entity, &objects).level,
                    "driver": entity.fighter.and_then(|fighter| fighter.mind.driver).map(|seat| seat.0),
                    "worn": worn_by(entity.id.0),
                }))
                .collect::<Vec<_>>(),
            "gold": snapshot.goods.gold,
            "feedback": ui.vm.feedback.iter().rev().take(5).collect::<Vec<_>>(),
        }),
    );
    let open = presented.0.current().entities.iter().find_map(|entity| {
        let (sim::Kind::Object(ObjectKind::Placeable, index), Some(object)) =
            (entity.kind, entity.object)
        else {
            return None;
        };
        let held = objects.get(index)?.container && object.tenants & bit != 0;
        held.then(|| {
            (
                index,
                snapshot.goods.carried(entity.id.0).collect::<Vec<_>>(),
            )
        })
    });
    // A close waited for ends when the snapshot has it, or in time.
    closing.0.retain_mut(|(container, frames)| {
        *frames = frames.saturating_sub(1);
        *frames > 0 && open.as_ref().is_some_and(|(index, ..)| index == container)
    });
    let open = open.filter(|(index, ..)| !closing.0.iter().any(|(closed, _)| closed == index));
    pending.0.retain(|(container, item)| match &open {
        Some((index, items)) => container == index && items.iter().any(|(id, _)| id == item),
        None => false,
    });
    // Remains are named as their bag's row (`sim::remains`), others as
    // their template.
    let title = open.as_ref().and_then(|(index, ..)| {
        let facts = objects.get(*index)?;
        match sim::remains::bag_row(&facts.template) {
            Some(_) => Some(ui::strings::bytes(&facts.name)),
            None => ui
                .assets
                .as_mut()?
                .template_name(&facts.template, fmt_odyssey::ResType::UTP),
        }
    });
    let catalog = swap.level.scripts().map(|library| library.items());
    let most = |template: &str| {
        catalog
            .and_then(|catalog| catalog.get(template))
            .map_or(1, |facts| facts.stacking)
    };
    // What the party may give (KOTOR's give mode lists the party's items but
    // its plot ones); one given waits until the party no longer has it.
    let carried: Vec<_> = snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter(|(_, item)| !item.plot)
        .collect();
    given
        .0
        .retain(|item| carried.iter().any(|(id, _)| id == item));
    let assets = &mut ui.assets;
    let mut rows = |items: &[(u32, &sim::inventory::Item)], asked: &dyn Fn(u32) -> bool| {
        stacks(items, most)
            .into_iter()
            .map(|(template, items, count)| {
                let look = assets.as_mut().and_then(|assets| assets.item(template));
                LootRow {
                    pending: items.iter().any(|item| asked(*item)),
                    items,
                    count,
                    name: look
                        .as_ref()
                        .map_or_else(|| template.as_bytes().to_vec(), |look| look.name.clone()),
                    icon: look.map(|look| look.icon),
                }
            })
            .collect::<Vec<_>>()
    };
    let loot = open.map(|(index, items)| LootView {
        container: index,
        title: title.unwrap_or_else(|| ui::strings::bytes(ui::strings::CONTAINER)),
        rows: rows(&items, &|item| pending.0.contains(&(index, item))),
        give: rows(&carried, &|item| given.0.contains(&item)),
    });
    ui.vm.loot = loot;
    ui.stack.sync("container", ui.vm.loot.is_some(), &ui.vm);
}

/// A container's items (by id) as KOTOR's container lists them: an item
/// joins the first stack before it of its template with room (as many as
/// `most` of its template, `baseitems.2da`'s `stacking`), else is a stack
/// of its own; each stack's template, items and count.
fn stacks<'a>(
    items: &[(u32, &'a sim::inventory::Item)],
    most: impl Fn(&str) -> u16,
) -> Vec<(&'a str, Vec<u32>, u32)> {
    let mut stacks: Vec<(&str, Vec<u32>, u32)> = Vec::new();
    for &(id, held) in items {
        let (template, count) = (held.template.as_str(), u32::from(held.stack.max(1)));
        let room = u32::from(most(template));
        match stacks
            .iter_mut()
            .find(|(of, _, had)| room > 1 && *of == template && had + count <= room)
        {
            Some((_, ids, had)) => {
                ids.push(id);
                *had += count;
            }
            None => stacks.push((template, vec![id], count)),
        }
    }
    stacks
}

/// "Experience Points Gained: ", "… Lost: ".
const GAINED: u32 = 1536;
const LOST: u32 = 1537;

/// A notice in words, its item by name and the game's own lines when the
/// assets know them.
pub fn words(notice: &Notice, ui: Option<&mut Ui>) -> String {
    use sim::locks::lines;
    use ui::draw::Textures;
    let line = |ui: &Option<&mut Ui>, strref: u32, otherwise: &str| {
        ui.as_ref()
            .and_then(|ui| ui.assets.as_ref()?.text(Source::Campaign, strref))
            .map_or_else(
                || otherwise.to_string(),
                |text| {
                    fmt_odyssey::code_page(&fmt_odyssey::tlk::unbraced(&text))
                        .trim_end()
                        .to_string()
                },
            )
    };
    match notice {
        Notice::Line(strref) => line(&ui, *strref, &format!("(line {strref})")),
        Notice::Count {
            line: strref,
            count,
        } => line(&ui, *strref, "<CUSTOM0>").replace("<CUSTOM0>", &count.to_string()),
        Notice::Check {
            skill,
            roll,
            total,
            dc,
            success,
        } => {
            let outcome = match (success, 20 + total >= i16::from(*dc)) {
                (true, _) => line(&ui, lines::SUCCESS, "success"),
                (false, true) => line(&ui, lines::FAILURE, "failure"),
                (false, false) => line(&ui, lines::NOT_POSSIBLE, "success not possible"),
            };
            let custom = [
                String::new(),
                line(&ui, *skill, "Skill"),
                outcome,
                roll.map_or_else(|| line(&ui, lines::TAKE_20, "Take 20"), |_| String::new()),
                roll.unwrap_or(20).to_string(),
                format!(" {} {}", if *total < 0 { '-' } else { '+' }, total.abs()),
                dc.to_string(),
            ];
            let mut text = line(
                &ui,
                lines::CHECK,
                "<CUSTOM0> <CUSTOM1>. <CUSTOM2>! <CUSTOM3> (roll <CUSTOM4><CUSTOM5>) vs. DC <CUSTOM6>",
            );
            for (at, value) in custom.iter().enumerate() {
                text = text.replace(&format!("<CUSTOM{at}>"), value);
            }
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        Notice::Blast {
            sum,
            demolitions,
            strength,
            dc,
            success,
        } => {
            let outcome = match success {
                true => line(&ui, lines::BLAST_SUCCEEDED, "Succeeded"),
                false => line(&ui, lines::BLAST_FAILED, "Failed"),
            };
            let custom = [
                String::new(),
                outcome,
                sum.to_string(),
                "20".into(),
                demolitions.to_string(),
                strength.to_string(),
                dc.to_string(),
            ];
            let mut text = line(
                &ui,
                lines::BLAST,
                "Attempt to blast lock <CUSTOM1>: <CUSTOM2> ( <CUSTOM3> + <CUSTOM4>/2 + mine strength(<CUSTOM5>)) VS Lock DC of <CUSTOM6>",
            );
            for (at, value) in custom.iter().enumerate() {
                text = text.replace(&format!("<CUSTOM{at}>"), value);
            }
            text
        }
        Notice::TrapSeen { name } => line(
            &ui,
            lines::MINE_SEEN,
            "This mine appears to be a: <CUSTOM0>",
        )
        .replace("<CUSTOM0>", &line(&ui, *name, "mine")),
        Notice::Gone { item, by } => {
            let name = ui
                .and_then(|ui| ui.assets.as_mut()?.item(item))
                .map_or_else(
                    || item.clone(),
                    |look| String::from_utf8_lossy(&look.name).into_owned(),
                );
            match by {
                Some(seat) => format!("{name}: taken by player {}", seat.0 + 1),
                None => format!("{name}: already taken"),
            }
        }
        Notice::NotOpen => "That is not open.".into(),
        Notice::OutOfReach => "That is out of reach.".into(),
        Notice::Empty => "Nothing is left in it.".into(),
        Notice::NoTrade => "You are not trading with that store.".into(),
        Notice::NotForSale => "That is not for sale.".into(),
        Notice::CannotAfford => "You cannot afford that.".into(),
        Notice::CannotEquip => "That cannot be equipped there.".into(),
        Notice::NotYours { by: Some(seat) } => {
            format!("Player {} plays that character.", seat.0 + 1)
        }
        Notice::NotYours { by: None } => "Only the host equips companions nobody plays.".into(),
        Notice::Said(words) => words.clone(),
        Notice::LevelRefused => "The level could not be taken.".into(),
        // The game's own lines (K1's and TSL's talk tables share them).
        Notice::Experience(xp) => {
            let gained = if *xp >= 0 { GAINED } else { LOST };
            format!("{}{}", line(&ui, gained, "Experience: "), xp.abs())
        }
        // KOTOR refuses a move with enemies nearby [verify its own line in
        // K1's dialog.tlk, then send it as a `Notice::Line`].
        Notice::InCombat => "You cannot do that with enemies nearby.".into(),
        Notice::NotYourBreak => {
            "Only the player who called the break, or the host, can end it.".into()
        }
        Notice::CannotForce => "Only the host can go now, after 5 seconds.".into(),
        Notice::CannotRevive => "That one cannot be revived now.".into(),
        Notice::CannotUse => "That cannot be used now.".into(),
        Notice::CannotUpgrade => "That upgrade does not fit there.".into(),
        // Screens open and sounds play; they are not words
        // (`script_screens`, `ambience`).
        Notice::Show(screen) => format!("{screen:?}"),
        Notice::Sound(sound) => format!("[{}]", sound.resref()),
    }
}
