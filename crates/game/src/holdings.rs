//! Who holds the weapon models spawned in a play scene: each one is in a
//! character's hands (`Wielded`) or free, for its owner's character to
//! take when it is drawn.
//!
//! A weapon model lives as long as its scene unless it is replaced: a seat
//! armed again (`armory`) retires what it held, despawned, and a creature
//! made in play (`made`) brings its own and takes them when it goes. So the
//! scene's table of composed weapons (`LoadedScene::arms`) is read once,
//! when the scene comes (`seed`). From then on weapons only change hands, and a
//! world starting over in the same scene (a joined player's swap to the
//! host's world) hands each character's weapons back to its owner
//! (`reset`). No weapon is named again once it is retired. `probes` plays
//! these hand-overs on a world of its own.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;
use sim::SimId;
use sim::combat::sheet::Sheet;

use crate::weapons::{Armed, Hands, Hide, Owner, Place, Wielded, place};

/// Weapons never held: despawned, their blades' indices returned to retire
/// (`scene::RetiredBlades`).
pub fn retire(arms: [Option<Armed>; 2], commands: &mut Commands) -> Vec<usize> {
    arms.into_iter()
        .flatten()
        .flat_map(|armed| {
            commands.entity(armed.root).despawn();
            armed.blades
        })
        .collect()
}

/// A character's weapons in its hands, and where it holds them.
struct Holder {
    owner: Option<Owner>,
    hands: Hands,
    wielded: Wielded,
}

/// Every weapon of a scene, held or free (see the module documentation).
#[derive(Default)]
pub struct Holdings {
    /// The scene's composed weapons were read.
    seeded: bool,
    free: BTreeMap<Owner, [Option<Armed>; 2]>,
    /// By character.
    held: BTreeMap<SimId, Holder>,
}

impl Holdings {
    /// The scene's composed weapons, by owner, free: read the first time
    /// only, before anything else is armed in the scene.
    pub fn seed(&mut self, composed: impl IntoIterator<Item = (Owner, [Option<Armed>; 2])>) {
        if !self.seeded {
            self.seeded = true;
            self.free.extend(composed);
        }
    }

    /// Hangs `owner`'s free weapons in character `id`'s `hands` (none for
    /// a character without an owner).
    pub fn wield(
        &mut self,
        (id, owner): (SimId, Option<Owner>),
        sheet: &Sheet,
        hands: Hands,
        commands: &mut Commands,
    ) {
        let arms = owner
            .and_then(|owner| self.free.remove(&owner))
            .unwrap_or([None, None]);
        let wielded = Wielded::new(sheet, arms, &hands, commands);
        self.held.insert(
            id,
            Holder {
                owner,
                hands,
                wielded,
            },
        );
    }

    /// Arms `owner` again with `arms` (spawned by the armory, hidden): in
    /// the hands of its character `id` if that is drawn, else free until it
    /// is. What it had is retired; its blades' indices returned.
    pub fn rearm(
        &mut self,
        (owner, id): (Owner, Option<SimId>),
        sheet: &Sheet,
        arms: [Option<Armed>; 2],
        commands: &mut Commands,
    ) -> Vec<usize> {
        match id.and_then(|id| self.held.get_mut(&id)) {
            Some(holder) => {
                let retired = holder.wielded.retire(commands);
                holder.wielded = Wielded::new(sheet, arms, &holder.hands, commands);
                retired
            }
            None => self
                .free
                .insert(owner, arms)
                .map_or_else(Vec::new, |old| retire(old, commands)),
        }
    }

    /// Character `id`'s weapons hung in other `hands`: those of a body it
    /// is drawn in now (a disguise's, or its own again).
    pub fn rehang(&mut self, id: SimId, sheet: &Sheet, hands: Hands, commands: &mut Commands) {
        let Some(Holder { owner, wielded, .. }) = self.held.remove(&id) else {
            return;
        };
        let wielded = Wielded::new(sheet, wielded.into_arms(), &hands, commands);
        self.held.insert(
            id,
            Holder {
                owner,
                hands,
                wielded,
            },
        );
    }

    /// Character `id` of `owner` is gone with its body (a creature made in
    /// play, `made`): what it held, and what waited free for it, is
    /// retired; its blades' indices returned.
    pub fn forget(&mut self, (id, owner): (SimId, Owner), commands: &mut Commands) -> Vec<usize> {
        let mut retired = self
            .held
            .remove(&id)
            .map_or_else(Vec::new, |holder| holder.wielded.retire(commands));
        if let Some(arms) = self.free.remove(&owner) {
            retired.extend(retire(arms, commands));
        }
        retired
    }

    /// The world starts over: no character holds anything. In the same
    /// scene (`kept`) each one's weapons go back to its owner, free; a new
    /// scene took them with the old one, and its own composed weapons are
    /// read next (`seed`). Blades' indices of weapons left without an owner
    /// returned to retire.
    pub fn reset(&mut self, kept: bool, commands: &mut Commands) -> Vec<usize> {
        let held = std::mem::take(&mut self.held);
        if !kept {
            self.free.clear();
            self.seeded = false;
            return Vec::new();
        }
        let mut retired = Vec::new();
        for Holder { owner, wielded, .. } in held.into_values() {
            let arms = wielded.into_arms();
            let old = match owner {
                Some(owner) => self.free.insert(owner, arms),
                None => Some(arms),
            };
            retired.extend(old.map_or_else(Vec::new, |old| retire(old, commands)));
        }
        retired
    }

    /// Character `id` lets go of what it holds: free again for its owner's
    /// character to take when it is drawn (a seat dressed again,
    /// `wardrobe`). One without an owner keeps nothing; its blades' indices
    /// returned to retire.
    pub fn release(&mut self, id: SimId, commands: &mut Commands) -> Vec<usize> {
        let Some(Holder { owner, wielded, .. }) = self.held.remove(&id) else {
            return Vec::new();
        };
        let arms = wielded.into_arms();
        match owner {
            Some(owner) => self
                .free
                .insert(owner, arms)
                .map_or_else(Vec::new, |old| retire(old, commands)),
            None => retire(arms, commands),
        }
    }

    pub fn wielded(&self, id: SimId) -> Option<&Wielded> {
        Some(&self.held.get(&id)?.wielded)
    }

    pub fn wielded_mut(&mut self, id: SimId) -> Option<&mut Wielded> {
        Some(&mut self.held.get_mut(&id)?.wielded)
    }
}

/// One probe: what it checks, whether it held, what was seen.
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// A world to hand weapons over in: bodies' hands, weapon models, and the
/// commands the hand-overs queue, applied after each step.
struct Bench {
    world: World,
    queue: CommandQueue,
    sheet: Sheet,
}

impl Bench {
    fn new() -> Bench {
        Bench {
            world: World::new(),
            queue: CommandQueue::default(),
            sheet: Sheet::dev_jedi(),
        }
    }

    /// A weapon model with one blade.
    fn weapon(&mut self, blade: usize) -> Armed {
        Armed {
            root: self.world.spawn(Transform::IDENTITY).id(),
            name: format!("weapon {blade}"),
            blades: vec![blade],
            ends: Vec::new(),
            gun: None,
        }
    }

    fn hands(&mut self) -> Hands {
        Hands {
            held: [(); 2].map(|()| {
                Some((
                    self.world.spawn(Transform::IDENTITY).id(),
                    Transform::IDENTITY,
                ))
            }),
            ..Hands::default()
        }
    }

    /// Runs `step` and applies the commands it queued; why not, if
    /// applying them panicked.
    fn run<T>(&mut self, step: impl FnOnce(&mut Commands, &Sheet) -> T) -> Result<T, String> {
        let value = step(
            &mut Commands::new(&mut self.queue, &self.world),
            &self.sheet,
        );
        std::panic::catch_unwind(AssertUnwindSafe(|| self.queue.apply(&mut self.world))).map_err(
            |panic| {
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(ToString::to_string))
                    .unwrap_or_else(|| "a panic".to_string())
            },
        )?;
        Ok(value)
    }

    /// Whether character `id` holds `weapon` and nothing else, hung in its
    /// right hand (`hands`).
    fn holds(&self, holdings: &Holdings, (id, hands): (SimId, &Hands), weapon: &Armed) -> bool {
        let held = holdings
            .wielded(id)
            .is_some_and(|wielded| wielded.models().eq([weapon.root]));
        let parent = self.world.get::<ChildOf>(weapon.root).map(ChildOf::parent);
        held && parent.is_some() && parent == hands.held[0].map(|(hand, _)| hand)
    }

    fn gone(&self, weapon: &Armed) -> bool {
        self.world.get_entity(weapon.root).is_err()
    }
}

fn arms(weapon: &Armed) -> [Option<Armed>; 2] {
    [Some(weapon.clone()), None]
}

/// The hand-overs: the scene's weapons held, a seat armed again, the world
/// starting over in the same scene (a joined player's swap to the host's
/// world, which once hung a retired weapon and crashed), a seat drawn in a
/// disguise's body and back in its own, a seat armed before it is drawn, a
/// new scene, a weapon gone before it is hung, and where a conversation
/// that hides items draws each weapon (`weapons::place`).
pub fn probes() -> Vec<Check> {
    let mut checks = Vec::new();
    // What a step gave: whether it held and what was seen, or the panic
    // applying its commands.
    let mut check = |name, outcome: Result<(bool, String), String>| {
        let (ok, detail) = outcome.unwrap_or_else(|panic| (false, format!("panicked: {panic}")));
        checks.push(Check { name, ok, detail });
    };
    let (seat0, seat1, creature) = (Owner::Seat(0), Owner::Seat(1), Owner::Creature(7));
    let ids = [SimId(0), SimId(1), SimId(2)];

    // The duel's two seats and a creature, holding the scene's weapons.
    let mut bench = Bench::new();
    let composed = [bench.weapon(0), bench.weapon(1), bench.weapon(2)];
    let hands = [bench.hands(), bench.hands(), bench.hands()];
    let owners = [seat0, seat1, creature];
    // As `play_view` does every frame.
    let seed = |holdings: &mut Holdings| {
        holdings.seed(owners.into_iter().zip(composed.iter().map(arms)));
    };
    let mut holdings = Holdings::default();
    seed(&mut holdings);
    let wield_all = |holdings: &mut Holdings, commands: &mut Commands, sheet: &Sheet| {
        for ((id, owner), hands) in ids.into_iter().zip(owners).zip(hands) {
            holdings.wield((id, Some(owner)), sheet, hands, commands);
        }
    };
    let each_held = |bench: &Bench, holdings: &Holdings, weapons: [&Armed; 3]| {
        ids.into_iter()
            .zip(&hands)
            .zip(weapons)
            .all(|(held, weapon)| bench.holds(holdings, held, weapon))
    };
    let held = bench.run(|commands, sheet| wield_all(&mut holdings, commands, sheet));
    check(
        "composed",
        held.map(|()| {
            let ok = each_held(
                &bench,
                &holdings,
                [&composed[0], &composed[1], &composed[2]],
            );
            (ok, "each character holds the scene's weapon for it".into())
        }),
    );

    // Seat 0 armed again (a joined player reading the host's sheets).
    let saber = bench.weapon(3);
    let retired = bench.run(|commands, sheet| {
        holdings.rearm((seat0, Some(ids[0])), sheet, arms(&saber), commands)
    });
    check(
        "armed again",
        retired.map(|retired| {
            let ok = retired == [0] && bench.gone(&composed[0]) && bench.holds(&holdings, (ids[0], &hands[0]), &saber);
            let detail = format!("the saber in seat 0's hand, its weapon before despawned; retired blades {retired:?}");
            (ok, detail)
        }),
    );

    // The world starts over in the same scene; everyone is cast again.
    let again = bench.run(|commands, sheet| {
        let retired = holdings.reset(true, commands);
        seed(&mut holdings);
        wield_all(&mut holdings, commands, sheet);
        retired
    });
    check(
        "same scene",
        again.map(|retired| {
            let ok = retired.is_empty() && each_held(&bench, &holdings, [&saber, &composed[1], &composed[2]]);
            let detail = format!("each character takes what it held, never the retired weapon; retired blades {retired:?}");
            (ok, detail)
        }),
    );

    // Seat 1 drawn in a disguise's body, then in its own again: its
    // weapon goes from hand to hand, nothing retired.
    let disguise = bench.hands();
    for (name, now) in [("disguised", disguise), ("own body again", hands[1])] {
        let moved = bench.run(|commands, sheet| holdings.rehang(ids[1], sheet, now, commands));
        check(
            name,
            moved.map(|()| {
                let ok = bench.holds(&holdings, (ids[1], &now), &composed[1]);
                (
                    ok,
                    "seat 1 holds its weapon in the body it is drawn in".into(),
                )
            }),
        );
    }

    // A seat armed before its character is drawn, twice: each time what it
    // had is retired; drawn, it takes the last.
    let mut bench = Bench::new();
    let composed = bench.weapon(0);
    let (first, second) = (bench.weapon(1), bench.weapon(2));
    let hands = bench.hands();
    let mut holdings = Holdings::default();
    holdings.seed([(seat0, arms(&composed))]);
    let retired = bench.run(|commands, sheet| {
        let mut retired = holdings.rearm((seat0, Some(ids[0])), sheet, arms(&first), commands);
        retired.extend(holdings.rearm((seat0, None), sheet, arms(&second), commands));
        holdings.wield((ids[0], Some(seat0)), sheet, hands, commands);
        retired
    });
    check(
        "armed before drawn",
        retired.map(|retired| {
            let ok = retired == [0, 1]
                && bench.gone(&composed)
                && bench.gone(&first)
                && bench.holds(&holdings, (ids[0], &hands), &second);
            let detail = format!(
                "the last weapon armed held, the others despawned; retired blades {retired:?}"
            );
            (ok, detail)
        }),
    );

    // A new scene: its own composed weapons are read.
    let fresh = bench.weapon(3);
    let swapped = bench.run(|commands, sheet| {
        let retired = holdings.reset(false, commands);
        holdings.seed([(seat0, arms(&fresh))]);
        holdings.wield((ids[0], Some(seat0)), sheet, hands, commands);
        retired
    });
    check(
        "new scene",
        swapped.map(|retired| {
            let ok = retired.is_empty() && bench.holds(&holdings, (ids[0], &hands), &fresh);
            (ok, "the new scene's weapon held".into())
        }),
    );

    // A weapon despawned before its hanging is applied.
    let gone = bench.weapon(4);
    let hung = bench.run(|commands, sheet| {
        commands.entity(gone.root).despawn();
        Wielded::new(sheet, arms(&gone), &hands, commands);
    });
    check(
        "gone before hung",
        hung.map(|()| (bench.gone(&gone), "a warning, not a crash".into())),
    );

    // What a conversation hides (S8), held or carried, out or away.
    let wrong: Vec<_> = [Hide::None, Hide::Held, Hide::All]
        .into_iter()
        .flat_map(|hidden| [(hidden, true), (hidden, false)])
        .flat_map(|(hidden, in_hand)| [(hidden, in_hand, true), (hidden, in_hand, false)])
        .filter(|&(hidden, in_hand, out)| {
            let want = match (hidden, in_hand, out) {
                (Hide::All, ..) | (Hide::Held, true, _) => Place::Nowhere,
                (Hide::None, true, true) => Place::Hand,
                _ => Place::Carried,
            };
            place(hidden, in_hand, out) != want
        })
        .collect();
    check(
        "conversation hides",
        Ok((
            wrong.is_empty(),
            format!(
                "out in hand, else carried; Held hides the hand's weapon wherever it is, All every one; wrong: {wrong:?}"
            ),
        )),
    );
    checks
}
