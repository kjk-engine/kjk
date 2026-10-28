//! The trap and stealth XP routines (`docs/COMBAT.md`;
//! `world::traps` has the traps in play, `world::covert` stealth): a trap's
//! facts (`ObjectData::trap`) and state (`State::traps`), who detected and
//! disarmed what, the nearest trap; the area's stealth XP (`StealthXp`) and
//! whether a creature is stealthed (TSL's `IsStealthed`); TSL's mines a
//! script spawns or sets off, and the Demolitions a mine was set with
//! (`TrapState::owner_demolitions`).

use script_ncs::vm::{ObjectId, Value};

use super::engine::{Bound, Engine, int, object, truth};
use super::{Change, Made, State, StealthXp, TrapState, last};
use crate::combat::sheet::{factions, skills};
use crate::module::{ModuleData, ObjectKind};
use crate::snapshot::trap;

impl StealthXp {
    /// An area's as its facts start it.
    fn starting(module: &ModuleData) -> StealthXp {
        let facts = module.area_stealth;
        StealthXp {
            max: facts.xp_max,
            current: facts.xp_current.min(facts.xp_max),
            loss: facts.xp_loss,
            enabled: facts.xp_enabled,
        }
    }
}

impl State {
    /// The area's stealth XP, as its facts start it the first time.
    pub(crate) fn stealth_xp(&mut self, module: &ModuleData) -> &mut StealthXp {
        self.stealth_xp
            .entry(module.area.clone())
            .or_insert_with(|| StealthXp::starting(module))
    }
}

impl Engine<'_> {
    /// `target`'s trap facts, if it has a trap.
    fn trap_of(&self, target: ObjectId) -> Option<&crate::traps::Trap> {
        self.view.facts(target)?.trap.as_ref()
    }

    /// Whether `target` is a trap still armed: not disabled (in the world,
    /// or this step by a script).
    fn armed(&self, target: ObjectId) -> bool {
        self.trap_of(target).is_some()
            && self
                .view
                .thing(target)
                .is_some_and(|thing| thing.trap & trap::DISABLED == 0)
            && !self
                .state
                .traps
                .get(&target.0)
                .is_some_and(|kept| kept.disabled)
    }

    /// TSL's `SpawnMine(type, place, detect DC, disarm DC, creator)`: a
    /// mine of `traps.2da` row `type` made at the place (`traps::laid_facts`,
    /// the square of a laid mine), its DCs the row's modifiers plus those
    /// given; the creator's (a creature's faction and its Demolitions now,
    /// a placeable's faction), else hostile (`HOSTILE_1`); no one has
    /// detected it.
    fn spawn_mine(&mut self, args: &[Value]) {
        let module = self.view.module;
        let (Ok(row), Some((at, _))) = (u8::try_from(int(args, 0)), self.place(args, 1)) else {
            return;
        };
        let (Some(trap_row), Some(mut facts)) = (
            module.traps.get(usize::from(row)),
            crate::traps::laid_facts(module, &crate::traps::laid_template(row)),
        ) else {
            return;
        };
        let dc = |modifier: u8, base: i32| (i32::from(modifier) + base).clamp(0, 255) as u8;
        let laid = crate::traps::Laid {
            detect_dc: dc(trap_row.detect_mod, int(args, 2)),
            disarm_dc: dc(trap_row.disarm_mod, int(args, 3)),
        };
        let creator = object(args, 4);
        let (faction, demolitions) = match self.character(creator) {
            Some(c) => (c.faction, self.skill_rank(&c, skills::DEMOLITIONS)),
            None => {
                let placeable = self
                    .view
                    .thing(creator)
                    .filter(|thing| thing.kind == Some(ObjectKind::Placeable))
                    .and_then(|thing| thing.facts.as_deref());
                (
                    placeable.map_or(factions::HOSTILE_1, |facts| facts.faction),
                    0,
                )
            }
        };
        facts.position = at;
        let id = ObjectId(self.view.next_id + self.out.made.len() as u32);
        self.out.made.push(Made {
            id,
            facts: std::sync::Arc::new(facts),
            at,
            yaw: 0.0,
            kept: None,
        });
        self.state.traps.insert(
            id.0,
            TrapState {
                creator: self.view.thing(creator).map(|_| creator.0),
                faction: Some(faction),
                laid: Some(laid),
                owner_demolitions: demolitions.clamp(-128, 127) as i8,
                ..TrapState::default()
            },
        );
    }

    pub(super) fn trap_call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        let target = object(args, 0);
        let found = self.trap_of(target);
        let kept = self.state.traps.get(&target.0);
        let detected_by =
            |who: ObjectId| kept.is_some_and(|kept| kept.detected_by.contains(&who.0));
        match bound {
            GetIsTrapped => truth(self.armed(target)),
            GetTrapBaseType => Value::Int(found.map_or(-1, |trap| i32::from(trap.row))),
            GetTrapDetectable => truth(found.is_some_and(|trap| trap.detectable)),
            GetTrapDisarmable => truth(found.is_some_and(|trap| trap.disarmable)),
            GetTrapFlagged => truth(found.is_some_and(|trap| trap.flagged)),
            GetTrapOneShot => truth(found.is_some_and(|trap| trap.one_shot)),
            GetTrapDetectDC => Value::Int(
                kept.and_then(|kept| kept.laid)
                    .map(|laid| laid.detect_dc)
                    .or(found.map(|trap| trap.detect_dc))
                    .map_or(0, i32::from),
            ),
            GetTrapDisarmDC => Value::Int(
                kept.and_then(|kept| kept.laid)
                    .map(|laid| laid.disarm_dc)
                    .or(found.map(|trap| trap.disarm_dc))
                    .map_or(0, i32::from),
            ),
            GetTrapKeyTag => Value::String(
                found
                    .map_or_else(String::new, |trap| trap.key.clone())
                    .into(),
            ),
            GetTrapCreator => Value::Object(
                kept.and_then(|kept| kept.creator)
                    .map_or(ObjectId::INVALID, ObjectId),
            ),
            GetTrapDetectedBy => truth(detected_by(object(args, 1))),
            SetTrapDetectedBy => {
                let who = object(args, 1);
                if found.is_none() || self.view.thing(who).is_none() {
                    return Value::Int(0);
                }
                let party = self
                    .view
                    .thing(who)
                    .and_then(|thing| thing.character)
                    .is_some_and(|character| character.party);
                let kept = self.state.traps.entry(target.0).or_default();
                if !kept.detected_by.contains(&who.0) {
                    kept.detected_by.push(who.0);
                }
                if party {
                    let bits = self.view.thing(target).map_or(0, |thing| thing.trap);
                    self.out.changes.push(Change::Trap {
                        object: target,
                        bits: bits | trap::PARTY_SEES,
                    });
                }
                Value::Int(1)
            }
            SetTrapDisabled => {
                if found.is_some() {
                    self.state.traps.entry(target.0).or_default().disabled = true;
                    let bits = self.view.thing(target).map_or(0, |thing| thing.trap);
                    self.out.changes.push(Change::Trap {
                        object: target,
                        bits: bits | trap::DISABLED,
                    });
                }
                Value::Int(0)
            }
            GetLastTrapDetected => Value::Object(
                self.state
                    .last
                    .get(&(target.0, last::TRAP_DETECTED))
                    .map_or(ObjectId::INVALID, |id| ObjectId(*id)),
            ),
            GetLastDisarmed => Value::Object(
                self.state
                    .last
                    .get(&(caller.0, last::DISARMED))
                    .map_or(ObjectId::INVALID, |id| ObjectId(*id)),
            ),
            // The nearest armed trap to `target` (only those it detected
            // when asked), by distance across the floor.
            GetNearestTrapToObject => {
                let only_detected = int(args, 1) != 0;
                let Some(from) = self.view.thing(target).map(|thing| thing.position) else {
                    return Value::Object(ObjectId::INVALID);
                };
                let nearest = self
                    .view
                    .things
                    .iter()
                    .filter(|thing| thing.id != target && self.armed(thing.id))
                    .filter(|thing| {
                        !only_detected
                            || self
                                .state
                                .traps
                                .get(&thing.id.0)
                                .is_some_and(|kept| kept.detected_by.contains(&target.0))
                    })
                    .min_by(|a, b| {
                        let across =
                            |at: [f32; 3]| (at[0] - from[0]).powi(2) + (at[1] - from[1]).powi(2);
                        across(a.position).total_cmp(&across(b.position))
                    })
                    .map_or(ObjectId::INVALID, |thing| thing.id);
                Value::Object(nearest)
            }
            // No trap-only saves in the effects engine: the effect as it is.
            VersusTrapEffect => args.first().cloned().unwrap_or(Value::Int(0)),
            AwardStealthXP => {
                let xp = self.state.stealth_xp(self.view.module);
                if xp.enabled && xp.current != 0 {
                    let amount = xp.current;
                    xp.current = 0;
                    xp.enabled = false;
                    self.out.reward(self.state, amount);
                }
                Value::Int(0)
            }
            GetStealthXPEnabled => truth(self.state.stealth_xp(self.view.module).enabled),
            SetStealthXPEnabled => {
                self.state.stealth_xp(self.view.module).enabled = int(args, 0) != 0;
                Value::Int(0)
            }
            GetCurrentStealthXP => Value::Int(self.state.stealth_xp(self.view.module).current),
            SetCurrentStealthXP => {
                let xp = self.state.stealth_xp(self.view.module);
                xp.current = int(args, 0).clamp(0, xp.max.max(0));
                Value::Int(0)
            }
            GetMaxStealthXP => Value::Int(self.state.stealth_xp(self.view.module).max),
            SetMaxStealthXP => {
                let xp = self.state.stealth_xp(self.view.module);
                xp.max = int(args, 0).max(0);
                xp.current = xp.current.min(xp.max);
                Value::Int(0)
            }
            GetStealthXPDecrement => Value::Int(self.state.stealth_xp(self.view.module).loss),
            SetStealthXPDecrement => {
                self.state.stealth_xp(self.view.module).loss = int(args, 0).max(0);
                Value::Int(0)
            }
            // The Demolitions a mine's setter had as it set it: a door's
            // (its mine's), a placeable's or a trigger's; 0 for any other.
            GetOwnerDemolitionsSkill => {
                let mine = self
                    .state
                    .door_mines
                    .get(&target.0)
                    .map_or(target.0, |mine| mine.mine);
                let kind = self.view.thing(target).and_then(|thing| thing.kind);
                let set = matches!(
                    kind,
                    Some(ObjectKind::Door | ObjectKind::Placeable | ObjectKind::Trigger)
                );
                Value::Int(match self.state.traps.get(&mine) {
                    Some(kept) if set => i32::from(kept.owner_demolitions),
                    _ => 0,
                })
            }
            SpawnMine => {
                self.spawn_mine(args);
                Value::Int(0)
            }
            // Taken away (the blast is its trap script's, run first by the
            // caller: `k_sp1_generic`'s droid powers on a mine).
            DetonateMine => {
                if self.view.thing(target).and_then(|thing| thing.kind) == Some(ObjectKind::Trigger)
                {
                    self.out.changes.push(Change::destroy(target));
                }
                Value::Int(0)
            }
            IsStealthed => truth(
                self.view
                    .thing(target)
                    .and_then(|thing| thing.character)
                    .is_some_and(|character| character.stealth),
            ),
            _ => Value::Int(0),
        }
    }
}
