//! What one step of the harness reads of the world: the objects by tag,
//! where to stand, doors and exits ahead, the fallen, the party's keepers.

use super::*;

/// What one step of the harness reads, made once a step from the snapshot
/// it acts on, each part when first asked: the area's table of objects
/// (made again for each object asked about, it was most of a walk's time),
/// where each object's entity is, the doors as they stand, as a walk tries
/// them and as sight lines meet them, and the routes asked for.
pub(super) struct Here<'a> {
    pub(super) level: &'a sim::Level,
    pub(super) snapshot: &'a Snapshot,
    /// Doors pressed this goal that stayed shut (`refused`): planned as
    /// locked.
    pub(super) refused: &'a BTreeSet<u32>,
    /// The faces of the navigation mesh under triggers that turned the
    /// player back this goal (`TurnedBack`): planned round where a way
    /// round them leads. Empty when none did.
    pub(super) walled: Arc<[bool]>,
    pub(super) table: OnceCell<sim::Instances>,
    pub(super) entities: OnceCell<BTreeMap<u32, usize>>,
    pub(super) doors: OnceCell<BTreeMap<u32, sim::ObjectState>>,
    pub(super) tried: OnceCell<BTreeSet<u32>>,
    pub(super) sight: OnceCell<Option<Vec<u8>>>,
    pub(super) creatures: OnceCell<Vec<Creature>>,
    /// The faces of the navigation mesh under the placeables play made
    /// (`MadeWalls`), which the mesh, baked before, does not know: planned
    /// round where a way round them leads. Empty when none stands.
    pub(super) made: Arc<[bool]>,
    pub(super) routes: RefCell<BTreeMap<RouteKey, Way>>,
}

/// What shuts in where the player can get to (`sim::walk::frontier`): each
/// face's object, if any, and its centre to the nearest metre.
pub(super) type Shut = Vec<(Option<u32>, [i32; 3])>;

/// A route asked for (`Here::route`): its ends' bits, and whether through
/// the doors a walk tries.
pub(super) type RouteKey = ([u32; 6], bool);

/// A route's corners, if a way leads there, and whether it goes round the
/// triggers that turned the player back (`Here::way`).
pub(super) type Way = (Option<Vec<[f32; 3]>>, bool);

impl<'a> Here<'a> {
    pub(super) fn new(level: &'a sim::Level, snapshot: &'a Snapshot, refused: &'a BTreeSet<u32>) -> Self {
        Here {
            level,
            snapshot,
            refused,
            walled: Arc::new([]),
            table: OnceCell::new(),
            entities: OnceCell::new(),
            doors: OnceCell::new(),
            tried: OnceCell::new(),
            sight: OnceCell::new(),
            creatures: OnceCell::new(),
            made: Arc::new([]),
            routes: RefCell::default(),
        }
    }

    /// This step's reading, planning round the faces `walled` names.
    pub(super) fn walled(self, walled: &Arc<[bool]>) -> Self {
        Here {
            walled: walled.clone(),
            ..self
        }
    }

    /// Whether face `face` is under a trigger that turned the player back,
    /// or under a placeable play made.
    pub(super) fn walls(&self, face: u32) -> bool {
        self.walled.get(face as usize).copied().unwrap_or(false)
            || self.made.get(face as usize).copied().unwrap_or(false)
    }

    /// This step's reading, planning round the faces under the placeables
    /// play made (`MadeWalls::faces`).
    pub(super) fn made(self, made: &Arc<[bool]>) -> Self {
        Here {
            made: made.clone(),
            ..self
        }
    }

    pub(super) fn objects(&self) -> &sim::Instances {
        self.table
            .get_or_init(|| sim::Instances::in_snapshot(self.level, self.snapshot))
    }

    pub(super) fn facts(&self, index: u32) -> Option<Arc<ObjectData>> {
        self.objects().facts(index).cloned()
    }

    /// The facts of the object `entity` is, if it is one.
    pub(super) fn facts_of(&self, entity: &sim::EntityState) -> Option<Arc<ObjectData>> {
        match entity.kind {
            sim::Kind::Object(_, index) => self.facts(index),
            _ => None,
        }
    }

    /// The tag of the object `entity` is, for a report: `?` for none.
    pub(super) fn tag_of(&self, entity: Option<&sim::EntityState>) -> String {
        entity
            .and_then(|entity| self.facts_of(entity))
            .map_or_else(|| "?".to_string(), |data| data.tag.clone())
    }

    /// Object `index`'s entity, if it is in play.
    pub(super) fn entity(&self, index: u32) -> Option<&'a sim::EntityState> {
        let at = self.entities.get_or_init(|| {
            (0..)
                .zip(&self.snapshot.entities)
                .filter_map(|(at, entity)| match entity.kind {
                    sim::Kind::Object(_, index) => Some((index, at)),
                    _ => None,
                })
                .collect()
        });
        at.get(&index).map(|at| &self.snapshot.entities[*at])
    }

    /// Door `index` as it stands (one out of play, gone: a way through).
    pub(super) fn door(&self, index: u32) -> sim::ObjectState {
        self.doors
            .get_or_init(|| {
                self.snapshot
                    .entities
                    .iter()
                    .filter_map(|entity| match (entity.kind, entity.object) {
                        (sim::Kind::Object(_, index), Some(object)) => Some((index, object)),
                        _ => None,
                    })
                    .collect()
            })
            .get(&index)
            .copied()
            .unwrap_or(sim::ObjectState::GONE)
    }

    /// Door `index` as a walk plans through it: a locked one with a
    /// fail-to-open script counted open, as a player tries it and its
    /// script may open it (Ahto City's gate, once the docking fee is paid;
    /// walking into a closed door near presses use on it), unless it was
    /// pressed and stayed shut (`refused`).
    pub(super) fn to_try(&self, index: u32) -> sim::ObjectState {
        let tried = self.tried.get_or_init(|| {
            (0..)
                .zip(self.level.placed().iter())
                .filter(|(_, object)| {
                    object.kind == ObjectKind::Door
                        && object.scripts.iter().any(|(hook, _)| hook == "OnFailToOpen")
                })
                .map(|(index, _)| index)
                .collect()
        });
        let mut state = self.door(index);
        if self.refused.contains(&index) {
            state.lock = sim::lock::LOCKED;
        } else if tried.contains(&index) {
            state.lock = sim::lock::UNLOCKED;
        }
        state
    }

    /// The doors as a line of sight meets them (`Level::states_in`).
    pub(super) fn sight(&self) -> Option<&[u8]> {
        self.sight
            .get_or_init(|| self.level.states_in(self.snapshot))
            .as_deref()
    }

    /// Whether the player's eye sees foe `id`'s chest.
    pub(super) fn sees(&self, player: &sim::motion::MotionState, id: sim::SimId) -> bool {
        self.snapshot.get(id).and_then(|e| e.player).is_some_and(|foe| {
            self.level
                .sees(self.sight(), mind::eye(player), mind::chest(&foe))
        })
    }

    /// The corners of a route from `from` to `to` (origins, units), through
    /// the doors as a walk tries them (`tried`) or as they stand
    /// (`sim::walk::route`), round the triggers that turned the player back
    /// where a way round them leads, else through them (`walled`); and
    /// whether it goes round them: each asked once a step.
    pub(super) fn way(&self, from: [f32; 3], to: [f32; 3], tried: bool) -> Way {
        let bits = [from, to].concat();
        let key = (std::array::from_fn(|k| bits[k].to_bits()), tried);
        if let Some(way) = self.routes.borrow().get(&key) {
            return way.clone();
        }
        let state = |index| {
            if tried {
                self.to_try(index)
            } else {
                self.door(index)
            }
        };
        let round = (!self.walled.is_empty() || !self.made.is_empty())
            .then(|| sim::walk::route_round(self.level, from, to, &state, &|f| self.walls(f)))
            .flatten();
        let way = match round {
            Some(corners) => (Some(corners), true),
            None => (sim::walk::route(self.level, from, to, &state), false),
        };
        self.routes.borrow_mut().insert(key, way.clone());
        way
    }

    /// `way`'s corners.
    pub(super) fn route(&self, from: [f32; 3], to: [f32; 3], tried: bool) -> Option<Vec<[f32; 3]>> {
        self.way(from, to, tried).0
    }

    /// Whether the walk from `from` to `to` goes round the triggers that
    /// turned the player back (a way round them leads there).
    pub(super) fn goes_round(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        self.way(from, to, true).1
    }

    /// Whether a way leads from `from` to `to`, through the doors a walk
    /// tries.
    pub(super) fn leads(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        self.route(from, to, true).is_some()
    }

    /// Whether a way leads from `from` to `to` through the doors as they
    /// stand: a fight's, which never tries a locked door (its script is a
    /// route's to set off: the Endar Spire's bridge door, pressed going
    /// after a Sith beyond it, let the bridge's scene play out a goal early).
    pub(super) fn open_way(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        self.route(from, to, false).is_some()
    }

    /// A walker to `at` (metres), or, when no way leads there (a locked
    /// door's doorway, a placeable), to the nearest floor before it on the
    /// player's side that a way leads to; the last stretch is walked
    /// straight. Where a way leads is judged from where the walk sets out
    /// over the navigation mesh: off it, the spot it steps back on to first
    /// (`sim::walk::back_on`). From off it no way leads anywhere, and the
    /// walk kept making for the target itself once back on, a door no way
    /// leads into (Anchorhead's way to the docking bay, from a planter's rim
    /// a stall's sidestep had climbed).
    pub(super) fn approach(&self, player: &sim::motion::MotionState, at: [f32; 3]) -> Walker {
        let to = walker_to(self.level, player, at);
        let me = player.origin.map(|v| v * UNIT);
        let d = flat(me, at).max(f32::EPSILON);
        let from = sim::walk::back_on(self.level, player.origin).unwrap_or(player.origin);
        let leads = |walker: &Walker| self.leads(from, walker.goal);
        if leads(&to) {
            return to;
        }
        [0.8, 1.6, 2.4, 3.2]
            .into_iter()
            .filter(|step| *step < d)
            .map(|step| {
                let before = [
                    at[0] + (me[0] - at[0]) / d * step,
                    at[1] + (me[1] - at[1]) / d * step,
                    at[2],
                ];
                walker_to(self.level, player, before)
            })
            .find(leads)
            // None on the straight way (a desk in a room beside it): any side.
            .or_else(|| {
                [0.8f32, 1.2, 1.6, 2.4].into_iter().find_map(|far| {
                    (0..8).find_map(|k| {
                        let angle = k as f32 * std::f32::consts::FRAC_PI_4;
                        let side = [at[0] + far * angle.cos(), at[1] + far * angle.sin(), at[2]];
                        Some(walker_to(self.level, player, side)).filter(leads)
                    })
                })
            })
            .unwrap_or(to)
    }

    /// A walker to talk to a creature at `at` (metres) within `reach`
    /// across the floor: `approach`'s, or, when no way leads there (the
    /// speaker on a ledge no path climbs: Dxun's Kumus on his ridge), a
    /// stand on the floor below within the reach that a way leads to, the
    /// nearest the player first (a talk's reach is flat,
    /// `sim::world::uses::may_use`).
    pub(super) fn approach_talk(&self, player: &sim::motion::MotionState, at: [f32; 3], reach: f32) -> Walker {
        let to = self.approach(player, at);
        let from = sim::walk::back_on(self.level, player.origin).unwrap_or(player.origin);
        if self.leads(from, to.goal) {
            return to;
        }
        let me = player.origin.map(|v| v * UNIT);
        let mut stands: Vec<(f32, Walker)> = TALK_RINGS
            .iter()
            .map(|share| share * reach)
            .flat_map(|far| {
                (0..16).map(move |k| {
                    let angle = k as f32 * std::f32::consts::FRAC_PI_8;
                    [at[0] + far * angle.cos(), at[1] + far * angle.sin(), me[2]]
                })
            })
            .map(|side| walker_to(self.level, player, side))
            .filter(|walker| self.leads(from, walker.goal))
            .map(|walker| (flat(walker.goal.map(|v| v * UNIT), me), walker))
            .collect();
        stands.sort_by(|a, b| a.0.total_cmp(&b.0));
        stands.into_iter().next().map_or(to, |(_, walker)| walker)
    }

    /// Where a use of what stands at `at` (metres) is walked to and reached
    /// from: a placeable with action points (`ObjectData::use_points`) at
    /// the one nearest the player, as KOTOR walks a use to the point TSL's
    /// `CSWSCreature::GetUseRange` takes (`CSWSPlaceable::GetNearestActionPoint`,
    /// within 0.1 m when it is a safe place to stand) and `GetIsInUseRange`
    /// measures to (`sim::world::uses::Target::aimed`); anything else at
    /// `at`. TSL's medbay bin, walked to along the line from the player,
    /// was made for beside the bed, where the dummy Exile lying on it held
    /// the player 2.7 m off; its action point stands 2.2 m clear of him.
    pub(super) fn use_point(me: [f32; 3], at: [f32; 3], facts: Option<&ObjectData>) -> [f32; 3] {
        facts
            .filter(|facts| facts.kind == ObjectKind::Placeable)
            .and_then(|facts| facts.nearest_use_point(me))
            .unwrap_or(at)
    }
}

/// The instances tagged (or made from a template named) `tag`, by number:
/// placed or made, there or gone.
pub(super) fn tagged(here: &Here, tag: &str) -> Vec<u32> {
    here.objects()
        .iter()
        // By tag, or by template (one of many alike: the Vulkars'
        // lieutenant among his guards).
        .filter(|(_, data)| {
            data.tag.eq_ignore_ascii_case(tag) || data.template.eq_ignore_ascii_case(tag)
        })
        .map(|(index, _)| index)
        .collect()
}

/// Where object `index` stands, if it is there (metres): a trigger's, the
/// middle of its outline (its origin may lie outside it).
pub(super) fn standing(here: &Here, index: u32) -> Option<[f32; 3]> {
    let entity = here.entity(index)?;
    let data = here.facts(index);
    Some(match data {
        Some(data) if data.kind == ObjectKind::Trigger && !data.geometry.is_empty() => {
            inside_trigger(&data, entity.position)
        }
        _ => entity.position,
    })
}

/// The object `tag` names, if it is there: its index and where it stands
/// (`standing`). `TAG@N` is the area's `N`th of that tag, placed or made,
/// counted whether or not those before it are still there (one fallen and
/// taken away moves no other's number); a tag alone, the first there.
pub(super) fn find(here: &Here, tag: &str, nth: Option<usize>) -> Option<(u32, [f32; 3])> {
    let tagged = tagged(here, tag);
    match nth {
        Some(nth) => {
            let index = *tagged.get(nth - 1)?;
            standing(here, index).map(|at| (index, at))
        }
        None => tagged
            .into_iter()
            .find_map(|index| standing(here, index).map(|at| (index, at))),
    }
}

/// The object a goal's tag names: one of its alternatives (`a|b|c`, any of
/// a herd will do: Komad's banthas), the nearest to `player` that a way
/// round the triggers that turned it back this goal leads to (`Here::walled`:
/// one wandered out past Tatooine's dune edge), else the nearest; a tag
/// alone, as `find`.
pub(super) fn find_any(
    here: &Here,
    tag: &str,
    nth: Option<usize>,
    player: &sim::motion::MotionState,
) -> Option<(u32, [f32; 3])> {
    if !tag.contains('|') {
        return find(here, tag, nth);
    }
    let from = player.origin.map(|v| v * UNIT);
    let behind = |at: [f32; 3]| {
        !here.walled.is_empty()
            && !here.goes_round(player.origin, walker_to(here.level, player, at).goal)
    };
    let mut found: Vec<(u32, [f32; 3])> = tag
        .split('|')
        .filter_map(|tag| find(here, tag, nth))
        .collect();
    found.sort_by(|a, b| flat(a.1, from).total_cmp(&flat(b.1, from)).then(a.0.cmp(&b.0)));
    found
        .iter()
        .find(|(_, at)| !behind(*at))
        .or(found.first())
        .copied()
}

/// How far (metres) the player moves in one step to have been jumped
/// there, not walked: a run covers some 5 cm a step.
pub(super) const JUMPED_M: f32 = 1.0;

/// The rings `Here::approach_talk` looks for a stand on, as shares of the
/// talk's reach: inside it, so a walk arriving short still talks.
const TALK_RINGS: [f32; 4] = [0.4, 0.55, 0.7, 0.8];

/// The triggers that turned the player back this goal: one it stood in while
/// a conversation held it and was then jumped out of (Tatooine's dune edge,
/// walked into, plays its warning and jumps the player back inside the
/// dunes), not one it walked out of; and the faces of the navigation mesh
/// under them (`sim::walk::faces_under`), which the goal's walks plan round
/// where a way round them leads (`Here::walled`), as a player keeps off an
/// edge once shown it. Of one level: another's are dropped.
pub(super) struct TurnedBack {
    pub(super) level: String,
    pub(super) stood_in: BTreeSet<u32>,
    pub(super) was_at: Option<[f32; 3]>,
    pub(super) triggers: BTreeSet<u32>,
    pub(super) walled: Arc<[bool]>,
}

impl Default for TurnedBack {
    fn default() -> Self {
        TurnedBack {
            level: String::new(),
            stood_in: BTreeSet::new(),
            was_at: None,
            triggers: BTreeSet::new(),
            walled: Arc::new([]),
        }
    }
}

impl TurnedBack {
    /// One step of the player at `at` (metres) on `level`, `talking` or
    /// not; `holding` the triggers that hold it there (asked only while one
    /// may be stood in). Whether a trigger turned it back this step.
    pub(super) fn note(
        &mut self,
        level: &str,
        at: [f32; 3],
        talking: bool,
        holding: impl FnOnce() -> BTreeSet<u32>,
    ) -> bool {
        if self.level != level {
            *self = TurnedBack {
                level: level.to_string(),
                ..TurnedBack::default()
            };
        }
        let jumped = self.was_at.is_some_and(|was| flat(was, at) > JUMPED_M);
        self.was_at = Some(at);
        if !talking && self.stood_in.is_empty() {
            return false;
        }
        let holding = holding();
        if talking {
            self.stood_in.extend(&holding);
        }
        let left: Vec<u32> = self.stood_in.difference(&holding).copied().collect();
        self.stood_in.retain(|index| holding.contains(index));
        let known = self.triggers.len();
        if jumped {
            self.triggers.extend(left);
        }
        self.triggers.len() > known
    }

    /// The faces under the triggers that turned the player back, made anew.
    pub(super) fn wall(&mut self, here: &Here) {
        let mut walled = Vec::new();
        for trigger in self.triggers.iter().filter_map(|index| here.facts(*index)) {
            let under = sim::walk::faces_under(here.level, &trigger);
            walled.resize(under.len(), false);
            for (walled, under) in walled.iter_mut().zip(under) {
                *walled |= under;
            }
        }
        self.walled = walled.into();
    }
}

/// The triggers holding a body at `at` (metres), but those that lead out.
pub(super) fn holding(here: &Here, at: [f32; 3]) -> BTreeSet<u32> {
    here.objects()
        .iter()
        .filter(|(_, data)| {
            data.kind == ObjectKind::Trigger
                && data.exit().is_none()
                && here.level.in_trigger(data, at)
        })
        .map(|(index, _)| index)
        .collect()
}

/// Where to stand in `trigger` placed at `at` (metres): the middle of its
/// outline, else, where that is outside it (a concave outline: the Upper
/// City cantina's Canderous), the spot in it nearest the middle on a
/// quarter-metre grid; the middle when none is.
pub(super) fn inside_trigger(trigger: &ObjectData, at: [f32; 3]) -> [f32; 3] {
    const GRID_M: f32 = 0.25;
    let outline = &trigger.geometry;
    let n = outline.len() as f32;
    let [mx, my, mz] = outline.iter().fold([0.0; 3], |[x, y, z], p| {
        [x + p[0] / n, y + p[1] / n, z + p[2] / n]
    });
    let middle = [at[0] + mx, at[1] + my, at[2]];
    // Within the outline, seen from above: a body with its soles at the
    // outline's height (no floor asked; the walk finds the floor).
    let inside = |x: f32, y: f32| {
        let origin = at[2] + mz - sim::motion::SOLE_Z * UNIT;
        trigger.contains([x, y, origin], None)
    };
    if inside(middle[0], middle[1]) {
        return middle;
    }
    let (low, high) = outline.iter().fold(
        ([f32::MAX; 2], [f32::MIN; 2]),
        |(low, high), p| {
            (
                [low[0].min(p[0]), low[1].min(p[1])],
                [high[0].max(p[0]), high[1].max(p[1])],
            )
        },
    );
    let steps = |from: f32, to: f32| ((to - from) / GRID_M).ceil().max(0.0) as usize;
    (0..=steps(low[0], high[0]))
        .flat_map(|i| (0..=steps(low[1], high[1])).map(move |j| (i, j)))
        .map(|(i, j)| {
            let x = at[0] + low[0] + i as f32 * GRID_M;
            let y = at[1] + low[1] + j as f32 * GRID_M;
            [x, y, at[2]]
        })
        .filter(|spot| inside(spot[0], spot[1]))
        .min_by(|a, b| flat(*a, middle).total_cmp(&flat(*b, middle)))
        .unwrap_or(middle)
}

/// A creature seen dead (`note_fallen`): its body, where it lies (metres),
/// when it was first seen dead, and where its remains stand, once the
/// engine has said so (`sim::remains::Bag::at`, as its body is taken).
#[derive(Debug, Clone, Copy)]
pub(super) struct Fell {
    pub(super) id: sim::SimId,
    pub(super) at: [f32; 3],
    pub(super) ms: u64,
    pub(super) bag: Option<[f32; 3]>,
}

impl Fell {
    /// Whether a `use` of it, its remains not found holding anything, is
    /// done: emptied by this use (`taken`, the remains' take all), or taken
    /// away leaving no remains: gone from `corpses` without ever having
    /// remains to come (it held nothing that drops). One whose remains were
    /// made and not looted is not done, wherever its body ended its fade.
    pub(super) fn used(&self, taken: bool, corpses: &BTreeMap<u32, sim::remains::Corpse>) -> bool {
        taken || (self.bag.is_none() && !corpses.contains_key(&self.id.0))
    }
}

/// Where each creature lies dead in `snapshot`, into `fallen` by its
/// number: a `use` of one dead is its remains', left where it lay once its
/// body is taken away (`sim::remains`), and a `to` one goes where it lay.
/// Once `corpses` says where its remains will stand, that is where it lies:
/// a body can still be moved as it fades, its remains standing where it
/// was taken.
pub(super) fn note_fallen(
    fallen: &mut BTreeMap<u32, Fell>,
    snapshot: &Snapshot,
    corpses: &BTreeMap<u32, sim::remains::Corpse>,
) {
    for entity in &snapshot.entities {
        let sim::Kind::Object(ObjectKind::Creature, index) = entity.kind else {
            continue;
        };
        if entity.fighter.is_some_and(|f| f.dead()) {
            let bag = corpses
                .get(&entity.id.0)
                .and_then(|corpse| corpse.bag)
                .map(|bag| bag.at);
            fallen
                .entry(index)
                .and_modify(|fell| {
                    fell.bag = fell.bag.or(bag);
                    fell.at = fell.bag.unwrap_or(entity.position);
                })
                .or_insert(Fell {
                    id: entity.id,
                    at: bag.unwrap_or(entity.position),
                    ms: snapshot.time_ms,
                    bag,
                });
        }
    }
}

/// Where the creature a goal's tag names fell (`fallen`): `TAG@N`'s, else
/// the last of that tag to fall.
pub(super) fn fell(here: &Here, fallen: &BTreeMap<u32, Fell>, tag: &str, nth: Option<usize>) -> Option<Fell> {
    let tagged = tagged(here, tag);
    match nth {
        Some(nth) => tagged.get(nth - 1).and_then(|index| fallen.get(index)).copied(),
        None => tagged
            .iter()
            .filter_map(|index| fallen.get(index))
            .max_by_key(|fell| fell.ms)
            .copied(),
    }
}

/// The remains standing nearest `lay` within `REMAINS_M` still holding
/// something (`state`'s items; one emptied waits to be closed and go): their
/// number and place.
pub(super) fn remains_at(here: &Here, state: &sim::script::State, lay: [f32; 3]) -> Option<(u32, [f32; 3])> {
    here.snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let sim::Kind::Object(ObjectKind::Placeable, index) = entity.kind else {
                return None;
            };
            sim::remains::bag_row(&here.objects().made_from(index)?.template)?;
            state
                .items
                .values()
                .any(|item| item.holder == entity.id.0)
                .then_some(())?;
            let d = flat(entity.position, lay);
            (d <= REMAINS_M).then_some((d, index, entity.position))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index, at)| (index, at))
}

/// Whether `goal`, foes standing near, fights them first: an exit or a use
/// while the party is in a fight (`in_combat`, until it has fought once this
/// goal, `fought`): an exit the party may not take in one, a lift whose
/// script wants the party gathered (the Leviathan's, a trooper spawned on
/// the way to it holding Bastila back); and an exit or a use whose way a
/// door stayed shut on since (`shut_since`): the Star Forge's frozen door,
/// Malak's scene past it waiting for the command centre's troopers to fall.
pub(super) fn fights_first(goal: &Goal, fought: bool, in_combat: bool, shut_since: bool) -> bool {
    match goal {
        Goal::Exit(_) | Goal::Use(_) => (!fought && in_combat) || shut_since,
        _ => false,
    }
}

/// The nearest closed door of the module within `DOOR_NEAR_M` of `at`: its
/// index, and where it stands.
/// One that leads to another module only when `leaving` (a fight or a walk
/// elsewhere never takes the party out); none pressed this goal that stayed
/// shut (`Here::refused`) but the goal's own (`own`, which a script may open
/// on a later press: Taris's lower door once the Sith let the party by):
/// passing one, a press opens nothing, and a door that says why (the Star
/// Forge's frozen door's line) would hold the world in its conversation
/// again and again, the foes about standing in it.
pub(super) fn closed_door_near(
    here: &Here,
    at: [f32; 3],
    leaving: bool,
    own: Option<u32>,
) -> Option<(u32, [f32; 3])> {
    here.snapshot
        .entities
        .iter()
        .filter_map(|entity| match entity.kind {
            sim::Kind::Object(ObjectKind::Door, index) => Some((index, entity)),
            _ => None,
        })
        .filter(|(index, entity)| {
            entity.object.is_some_and(|object| object.state == 0)
                && (own == Some(*index) || !here.refused.contains(index))
                && flat(entity.position, at) <= DOOR_NEAR_M
                && (leaving || here.facts(*index).is_none_or(|data| data.exit().is_none()))
        })
        .min_by(|a, b| flat(a.1.position, at).total_cmp(&flat(b.1.position, at)))
        .map(|(index, entity)| (index, entity.position))
}

/// Whether seat 0's use key, pressed in `command`, acts on object `index`
/// alone, as the sim picks it (`SimWorld::key_use`): a creature by its
/// body, anything else by itself. A press that would act on something
/// else (a button beside the door it is for, a terminal, a creature to
/// talk to) is not made.
pub(super) fn key_meets(world: &mut sim::SimWorld, snapshot: &Snapshot, command: &Command, index: u32) -> bool {
    match world.key_use(Seat(0), command.angles) {
        Some(sim::KeyUse::Object(of)) => of == index,
        Some(sim::KeyUse::Creature(id)) => snapshot
            .get(id)
            .is_some_and(|e| matches!(e.kind, sim::Kind::Object(_, of) if of == index)),
        None => false,
    }
}

/// Where the body at `at` (metres), moving at `velocity` (units a
/// second) and looking along `yaw` (degrees), would step into a trigger or
/// door that leads to another module other than `way_out` (an `exit`'s or a
/// `to`'s own, and any sharing its tag and way, or its tag where it leads
/// nowhere itself: the Elders' inner door and their gate are both
/// `RakataDoor2`): only a goal naming one takes the party out by one, as only
/// a door's `exit` opens one that leads out. `Ahead::Command`: `command`
/// carries it in (`EXIT_AHEAD_M` on); `Ahead::Slides`: let go (or
/// stopped), it slides in before ground friction stops it (a run ended beside
/// a map's edge: the Dune Sea's way into the Sand People's territory, a
/// goal met at full tilt).
pub(super) fn exit_ahead(
    here: &Here,
    at: [f32; 3],
    velocity: [f32; 3],
    yaw: f32,
    command: &Command,
    way_out: Option<u32>,
) -> Option<Ahead> {
    let (level, instances) = (here.level, here.objects());
    let own = way_out.and_then(|index| instances.get(index));
    let taken = |data: &ObjectData| {
        own.is_some_and(|own| {
            own.tag.eq_ignore_ascii_case(&data.tag)
                && (own.exit().is_none() || own.exit() == data.exit())
        })
    };
    let out = |point: [f32; 2]| {
        instances.iter().any(|(index, data)| {
            !taken(data)
                && data.exit().is_some()
                && match data.kind {
                    ObjectKind::Trigger => level.in_trigger(data, [point[0], point[1], at[2]]),
                    // A door that leads out is run into (or its open
                    // doorway walked into): Kashyyyk's town door, by which
                    // the walk out of the village arrives; anywhere along a
                    // wide one, as the sim takes it (the Elders' gate, backed
                    // into in their fight).
                    ObjectKind::Door => {
                        flat([point[0], point[1], 0.0], [data.position[0], data.position[1], 0.0])
                            <= EXIT_DOOR_M
                            || level.at_door(index, [point[0], point[1], at[2]])
                    }
                    _ => false,
                }
        })
    };
    let (f, r) = (f32::from(command.forward), f32::from(command.right));
    let driven = f != 0.0 || r != 0.0;
    if driven {
        let (s, c) = yaw.to_radians().sin_cos();
        let way = [f * c + r * s, f * s - r * c];
        let length = way[0].hypot(way[1]);
        let into = out([
            at[0] + way[0] / length * EXIT_AHEAD_M,
            at[1] + way[1] / length * EXIT_AHEAD_M,
        ]);
        // Still driven, by a way that does not lead out: its own.
        if !into {
            return None;
        }
    }
    // Let go, or to be stopped: where its slide ends, every quarter metre
    // of the way (a trigger may be thin); never one it stands in already
    // (where it arrived, or a goal's own way out beside another of its tag),
    // whose way the sim has taken or not.
    let speed = velocity[0].hypot(velocity[1]);
    let slide = speed / sim::motion::FRICTION * UNIT;
    let steps = (slide / 0.25).ceil() as usize;
    let slides = !out([at[0], at[1]]) && (1..=steps).any(|step| {
        let d = slide * step as f32 / steps as f32;
        out([
            at[0] + velocity[0] / speed * d,
            at[1] + velocity[1] / speed * d,
        ])
    });
    match (driven, slides) {
        (true, slides) => Some(Ahead::Command { slides }),
        (false, true) => Some(Ahead::Slides),
        (false, false) => None,
    }
}

/// What carries a body out by a way it is not to take (`exit_ahead`): its
/// command (and whether, let go, it would still slide in), or, let go, its
/// slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ahead {
    Command { slides: bool },
    Slides,
}

/// A command that pushes against `velocity` (units a second) for a body
/// looking along `yaw` (degrees): braking, as a player's key the other way.
pub(super) fn braking(velocity: [f32; 3], yaw: f32) -> (i8, i8) {
    let speed = velocity[0].hypot(velocity[1]);
    if speed <= 1.0 {
        return (0, 0);
    }
    let back = [-velocity[0] / speed, -velocity[1] / speed];
    let (s, c) = yaw.to_radians().sin_cos();
    // `way = forward·(c, s) + right·(s, −c)` (`exit_ahead`), inverted.
    let forward = back[0] * c + back[1] * s;
    let right = back[0] * s - back[1] * c;
    ((forward * 127.0) as i8, (right * 127.0) as i8)
}

/// Object `index`'s changing state: a door's open or closed, its lock.
pub(super) fn object_of(here: &Here, index: u32) -> sim::ObjectState {
    here.entity(index)
        .and_then(|e| e.object)
        .unwrap_or_else(|| sim::ObjectState {
            state: 0,
            ..sim::ObjectState::default()
        })
}


/// A walker to the floor nearest (`x`, `y`) metres, at the height of
/// `from`'s soles (units).
pub(super) fn walker_to(level: &sim::Level, from: &sim::motion::MotionState, at: [f32; 3]) -> Walker {
    let soles = (from.origin[2] + sim::motion::SOLE_Z) * UNIT;
    let floor = level
        .nav()
        .and_then(|nav| nav.height([at[0], at[1], soles], 50.0, 50.0))
        .unwrap_or(at[2]);
    Walker::new([
        at[0] / UNIT,
        at[1] / UNIT,
        floor / UNIT - sim::motion::SOLE_Z,
    ])
}

/// A stand round `at` (metres; one of rings of eight, within a talk's
/// reach) that a way leads to, from which the sim lets the use through
/// (`may_use`: the sim's own check, `SimWorld::may_use`, its line of sight
/// and reach in height): the
/// one nearest the player, as a player steps round to use it (KOTOR's use
/// wants a clear line: Uthar behind his desk's side; Hrakert's airlock
/// door behind its frame's edge; Kashyyyk's Star Map hologram, over the
/// floor round its dais, behind the dais's rim from near it).
pub(super) fn sighted_stand(
    here: &Here,
    player: &sim::motion::MotionState,
    at: [f32; 3],
    may_use: &mut dyn FnMut(&sim::motion::MotionState) -> bool,
) -> Option<Walker> {
    let me = player.origin.map(|v| v * UNIT);
    [1.2f32, 2.0, 2.8]
        .into_iter()
        .flat_map(|far| {
            (0..8).map(move |k| {
                let angle = k as f32 * std::f32::consts::FRAC_PI_4;
                [at[0] + far * angle.cos(), at[1] + far * angle.sin(), at[2]]
            })
        })
        .map(|spot| (spot, walker_to(here.level, player, spot)))
        .filter(|(_, walker)| {
            let mut there = *player;
            there.origin = walker.goal;
            may_use(&there) && here.leads(player.origin, walker.goal)
        })
        .min_by(|a, b| flat(a.0, me).total_cmp(&flat(b.0, me)))
        .map(|(_, walker)| walker)
}

/// The command that looks at `at` (metres) from where `player` stands.
pub(super) fn facing(player: &sim::motion::MotionState, at: [f32; 3]) -> Command {
    let (pitch, yaw) = mind::aim(player.origin, at.map(|v| v / UNIT));
    mind::looking(player, pitch, yaw)
}

/// The command that looks at the chest of a foe standing at `at` (metres)
/// to strike it: with a gun, the crosshair on it, as a player's shots go
/// where its crosshair line meets, above its eye (`mind::crosshair`).
pub(super) fn aiming(player: &sim::motion::MotionState, me: &sim::EntityState, at: [f32; 3]) -> Command {
    let chest = [at[0], at[1], at[2] + 0.3];
    if !ranged(me) {
        return facing(player, chest);
    }
    let (pitch, yaw) = mind::aim(mind::crosshair(player), chest.map(|v| v / UNIT));
    mind::looking(player, pitch, yaw)
}

/// Whether `me` fights with a gun.
pub(super) fn ranged(me: &sim::EntityState) -> bool {
    me.sheet
        .is_some_and(|sheet| sheet.main_weapon().delivery.ranged())
}

/// The living creatures hostile to the player within `radius` metres of
/// `me` that can be hurt (not plot: a scene's controller), nearest first:
/// (id, place). Hostile as the factions stand now (the scripts'
/// `AdjustReputation`: the Sand People at peace with one in their robes).
pub(super) fn hostiles(
    here: &Here,
    world: &sim::SimWorld,
    me: [f32; 3],
    radius: f32,
) -> Vec<(sim::SimId, [f32; 3])> {
    let mut found: Vec<(f32, sim::SimId, [f32; 3])> = here
        .creatures(world)
        .iter()
        .filter(|creature| creature.hostile && !creature.plot)
        .map(|creature| (flat(creature.at, me), creature.id, creature.at))
        .filter(|(d, ..)| *d <= radius)
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, id, at)| (id, at)).collect()
}

/// A living creature of the area, as the harness weighs it: its body,
/// number and place, whether it is hostile to the player now and plot,
/// and whom it fights.
#[derive(Debug, Clone, Copy)]
pub(super) struct Creature {
    pub(super) id: sim::SimId,
    pub(super) index: u32,
    pub(super) at: [f32; 3],
    pub(super) hostile: bool,
    pub(super) plot: bool,
    pub(super) target: Option<sim::SimId>,
}

impl Here<'_> {
    /// The area's living creatures (`Creature`), once a step.
    pub(super) fn creatures(&self, world: &sim::SimWorld) -> &[Creature] {
        self.creatures.get_or_init(|| {
            let player = self.snapshot.own(sim::Seat(0)).map(|me| me.id);
            self.snapshot
                .entities
                .iter()
                .filter_map(|entity| {
                    let sim::Kind::Object(ObjectKind::Creature, index) = entity.kind else {
                        return None;
                    };
                    let fighter = entity.fighter.filter(|f| f.alive())?;
                    let sheet = entity
                        .sheet
                        .or_else(|| self.facts(index).and_then(|d| d.sheet))?;
                    Some(Creature {
                        id: entity.id,
                        index,
                        at: entity.position,
                        hostile: player.and_then(|player| world.relation(entity.id, player))
                            == Some(sim::combat::Relation::Hostile),
                        plot: sheet.flags & sim::combat::sheet::flags::PLOT != 0,
                        target: fighter.target,
                    })
                })
                .collect()
        })
    }
}

/// A hostile creature keeping the party in combat: who, where, its tag,
/// whom it fights, whether it is plot, and the party's body nearest it
/// (its name, metres).
pub(super) struct Keeper {
    pub(super) id: sim::SimId,
    pub(super) at: [f32; 3],
    pub(super) tag: String,
    pub(super) target: Option<sim::SimId>,
    pub(super) plot: bool,
    pub(super) near: (String, f32),
}

/// What keeps the party in combat, as the sim reads it
/// (`SimWorld::combat_keepers`), each with the party's body nearest it
/// (the player at `me`).
pub(super) fn keepers(here: &Here, world: &sim::SimWorld, me: [f32; 3]) -> Vec<Keeper> {
    let keeping = world.combat_keepers();
    let bodies: Vec<(String, [f32; 3])> = std::iter::once(("the player".into(), me))
        .chain(
            mates(world, here.snapshot)
                .into_iter()
                .map(|mate| (format!("NPC {}", mate.npc), mate.body.position)),
        )
        .collect();
    here.creatures(world)
        .iter()
        .filter(|creature| keeping.contains(&creature.id))
        .filter_map(|creature| {
            let near = bodies
                .iter()
                .map(|(name, at)| {
                    let d = (0..3).map(|k| (at[k] - creature.at[k]).powi(2)).sum::<f32>();
                    (name.clone(), d.sqrt())
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            Some(Keeper {
                id: creature.id,
                at: creature.at,
                tag: here.tag_of(here.entity(creature.index)),
                target: creature.target,
                plot: creature.plot,
                near,
            })
        })
        .collect()
}

/// What an `if` or `while` asks.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Condition {
    /// A script global has the value (a truth as 0 or 1).
    Global(String, i32),
    /// The party is in the module.
    In(String),
    /// A companion (by NPC number) is available, or not.
    Npc(u8, bool),
    /// A local of the object a tag names (`find`'s): a number (`boolean`
    /// false) less than, equal to or greater than the value, as a script's
    /// `GetLocalNumber` reads it (a byte, unsigned); or a boolean the value
    /// (a truth: 0 or 1), as `GetLocalBoolean`'s raw bit is read true. An
    /// object not there holds none (0, as a script reads one that is not).
    Local {
        tag: String,
        nth: Option<usize>,
        boolean: bool,
        index: i32,
        order: std::cmp::Ordering,
        value: i32,
    },
}

impl Condition {
    /// Whether it holds now, the party in `module` (`here`'s).
    pub(super) fn holds(&self, state: &sim::script::State, module: &str, here: &Here) -> bool {
        match self {
            Condition::Local {
                tag,
                nth,
                boolean,
                index,
                order,
                value,
            } => {
                let read = find(here, tag, *nth)
                    .and_then(|(index, _)| here.entity(index))
                    .zip(here.snapshot.place.as_ref())
                    .map_or(0, |(entity, place)| {
                        let id = ObjectId(entity.id.0);
                        sim::script::local(state, (&place.game, module), id, *boolean, *index)
                    });
                let read = if *boolean { i32::from(read != 0) } else { read };
                read.cmp(value) == *order
            }
            Condition::Global(name, value) => match sim::script::global(&state.booleans, name) {
                Some(truth) => *truth == (*value != 0),
                None => sim::script::global(&state.numbers, name).copied().unwrap_or(0) == *value,
            },
            Condition::Npc(npc, available) => state.party.available.contains_key(npc) == *available,
            Condition::In(name) => module.eq_ignore_ascii_case(name),
        }
    }
}

/// One goal's outcome.
pub(super) struct Outcome {
    pub(super) goal: String,
    pub(super) module: String,
    pub(super) started_ms: u64,
    pub(super) ended_ms: u64,
    /// Wall-clock milliseconds the goal took (what it cost, beside the
    /// play time it spent).
    pub(super) wall_ms: u128,
    pub(super) met: bool,
    /// Why it was not met: where the player stood and what held it.
    pub(super) held: Vec<String>,
}

/// How far `p` stands off the straight way from `from` to `to` (metres,
/// seen from above).
pub(super) fn off_the_way(from: [f32; 3], to: [f32; 3], p: [f32; 3]) -> f32 {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let length = (dx * dx + dy * dy).max(f32::EPSILON);
    let t = (((p[0] - from[0]) * dx + (p[1] - from[1]) * dy) / length).clamp(0.0, 1.0);
    flat(p, [from[0] + dx * t, from[1] + dy * t, 0.0])
}

/// The doors and placeables within 2 m of the straight way from `from` to
/// `to` (metres): what may keep a route from there, each with its state,
/// and a door's lock and Security DC.
pub(super) fn on_the_way(here: &Here, from: [f32; 3], to: [f32; 3]) -> Vec<String> {
    here.snapshot
        .entities
        .iter()
        .filter_map(|entity| match entity.kind {
            sim::Kind::Object(kind @ (ObjectKind::Door | ObjectKind::Placeable), index)
                if off_the_way(from, to, entity.position) <= 2.0 =>
            {
                let data = here.facts(index)?;
                let state = object_of(here, index);
                let lock = match (&data.lock, state.locked(data.opens)) {
                    (Some(lock), true) => format!(", locked, DC {}", lock.open_dc),
                    (None, true) => ", locked".into(),
                    _ => String::new(),
                };
                Some(format!("{kind:?} {} state {}{lock}", data.tag, state.state))
            }
            _ => None,
        })
        .collect()
}

/// Whether every companion of the party table here stands up within a
/// follower's distance of its leader (`ai::FOLLOW_NEAR`) of the player at
/// `at` (metres): where the sim's companions stop following (`minds`).
pub(super) fn gathered(world: &sim::SimWorld, snapshot: &Snapshot, at: [f32; 3]) -> bool {
    mates(world, snapshot).iter().all(|mate| {
        mate.body.fighter.is_some_and(|f| f.alive())
            && flat(mate.body.position, at) <= sim::combat::ai::FOLLOW_NEAR * UNIT
    })
}

/// A companion of the party table with a body here: its NPC number and
/// body.
pub(super) struct Mate<'a> {
    pub(super) npc: u8,
    pub(super) body: &'a sim::EntityState,
}

pub(super) fn mates<'a>(world: &sim::SimWorld, snapshot: &'a Snapshot) -> Vec<Mate<'a>> {
    let party = &world.script_state().party;
    party
        .members
        .iter()
        .filter_map(|&npc| {
            let body = snapshot.get(sim::SimId(party.available.get(&npc)?.object?))?;
            Some(Mate { npc, body })
        })
        .collect()
}

/// How far (metres) from the player a companion's fight keeps it from the
/// party: farther, a use waits on it (`straggling`).
pub(super) const STRAY_M: f32 = 10.0;

/// The farthest (metres from the player at `me`, plus one) of the foes
/// keeping the party in combat (`keepers`, those the player can get at)
/// that fight a companion standing more than `STRAY_M` off: a fight that
/// holds the party apart. A use (a lift, a console) waits for it to be
/// fought, as a player goes to help before pressing: the scripts that
/// refuse a way on ask the party to gather (the Leviathan's hangar lift,
/// `k_plev_chkprox`: a companion more than 15 m from the lift's console),
/// and a companion in a fight follows no one.
pub(super) fn straggling(keepers: &[Keeper], mates: &[(sim::SimId, [f32; 3])], me: [f32; 3]) -> Option<f32> {
    keepers
        .iter()
        .filter(|keeper| {
            mates
                .iter()
                .any(|(id, at)| keeper.target == Some(*id) && flat(*at, me) > STRAY_M)
        })
        .map(|keeper| flat(keeper.at, me) + 1.0)
        .max_by(f32::total_cmp)
}

/// The body the first seat plays: the companion it was switched to
/// (`SwitchPlayerCharacter`, `Party::switched_body`), else its own
/// character.
fn played<'a>(world: &sim::SimWorld, snapshot: &'a Snapshot) -> Option<&'a sim::EntityState> {
    let own = snapshot.own(Seat(0))?;
    Some(
        world
            .script_state()
            .party
            .switched_body()
            .and_then(|id| snapshot.get(sim::SimId(id)))
            .filter(|body| body.sheet.is_some())
            .unwrap_or(own),
    )
}

/// The class the player would take a level in now, if the party's XP
/// allows one: the class the body it plays took last (a companion it was
/// switched to levels in its own, with the game's picks, as TSL's level-up screen levels the
/// character the player controls: Visas Marr in Dxun's jungle), else row
/// 0 for its own character (a career not begun here: its sheet's).
pub(super) fn level_due(world: &sim::SimWorld, snapshot: &Snapshot) -> Option<u8> {
    let own = snapshot.own(Seat(0))?;
    let body = played(world, snapshot)?;
    let sheet = body.sheet?;
    let state = world.script_state();
    let classes = world.level().scripts()?.classes();
    // A companion's career not begun here is its template's classes.
    let refused = BTreeSet::new();
    let template = || {
        Here::new(world.level(), snapshot, &refused)
            .facts_of(body)
            .and_then(|facts| facts.classes.last().map(|(class, _)| *class))
    };
    let last = state
        .careers
        .get(&body.id.0)
        .and_then(|career| career.last().map(|(class, _)| *class))
        .or_else(|| (body.id != own.id).then(template).flatten());
    if body.id != own.id && last.is_none() {
        return None;
    }
    classes
        .may_level(sheet.level, state.xp(sim::inventory::PARTY))
        .then(|| last.unwrap_or(0))
}

/// The picks `plan` makes for the player's next level of `class`
/// (`Classes::planned`); none for a companion the seat plays (the game's
/// picks).
pub(super) fn planned(
    world: &sim::SimWorld,
    snapshot: &Snapshot,
    class: u8,
    plan: &sim::levels::Plan,
) -> Option<sim::levels::Choices> {
    let avatar = snapshot.own(Seat(0))?;
    if played(world, snapshot)?.id != avatar.id {
        return None;
    }
    let sheet = avatar.sheet?;
    let classes = world.level().scripts()?.classes();
    let career = world
        .script_state()
        .careers
        .get(&avatar.id.0)
        .cloned()
        .unwrap_or_else(|| vec![(class, sheet.level)]);
    classes.planned(&sheet, &career, class, plan)
}
