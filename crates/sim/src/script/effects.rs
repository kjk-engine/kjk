//! The effects scripts make (`Effect…`) and apply (`ApplyEffectToObject`):
//! the engine's effects as values the scripts keep, so the games' own
//! scripts (`k_sp1_generic`, items, cutscenes) decide them and the sim
//! carries them out (I22 C10, C14).
//!
//! An effect value is a `Value::Effect(Handle(n))`, `State::effects[n - 1]`;
//! a link holds its parts. Every `Effect…` routine makes one: those the sim
//! applies by kind, any other as `Other` with its routine's name, so a link
//! keeps the parts the sim knows and the report names the rest.
//!
//! Applied, an effect waits in `State::applied` for the next step's damage
//! pipeline (`world::hurt`): damage as a script's hit (never filtered,
//! C17), death as a killing one, heals and FP at once; a modifier for its
//! duration in one of the character's lasting slots
//! (`combat::effects::Lasting`, folded into its sheet); a hold (stun,
//! paralysis, fear, ...) as C11's (`ja::STUN_MS` at most, then immune `ja::IMMUNE_MS`; fear
//! ends on damage) unless an immunity keeps it off; a KOTOR state (TSL's
//! cutscene holds, Force Crush's, a mind trick, ...) in a lasting slot for
//! its whole time (`combat::effects::state`). Visual effects change
//! nothing in the sim. `GetFirstEffect` walks the lasting slots
//! (`Effect::Slot`); `RemoveEffect` and `ClearAllEffects` take them off.

use script_ncs::vm::ObjectId;

use crate::combat::effects::Modifies;
use crate::wire::{DecodeError, Reader, Writer};

/// The holds (C11) an effect puts on: what immunity keeps each off
/// (`IMMUNITY_TYPE_*`), and whether it is fear (ends on damage). A droid's
/// stun (`EffectDroidStun`, made for droids) has none: a droid's immunity
/// to the mind's effects does not keep it off. A whirlwind
/// (`EffectWhirlWind`) holds as a stun does; the views spin its victim. A
/// choke (`EffectChoke`, the gas traps') only its link's immunities keep
/// off, and it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hold {
    #[default]
    Stun,
    Paralyze,
    Fear,
    Sleep,
    DroidStun,
    Whirlwind,
    Choke,
}

impl Hold {
    pub fn immunity(self) -> Option<u8> {
        use crate::combat::force::crowd::immunity;
        match self {
            Hold::Stun | Hold::Whirlwind => Some(immunity::STUN),
            Hold::Paralyze => Some(immunity::PARALYSIS),
            Hold::Fear => Some(immunity::FEAR),
            Hold::Sleep => Some(immunity::SLEEP),
            Hold::DroidStun | Hold::Choke => None,
        }
    }

    /// The Sentinel's Force Immunity feat that keeps it off, if any.
    pub fn feat(self) -> Option<u16> {
        use crate::combat::sheet::feats;
        match self {
            Hold::Stun | Hold::Whirlwind => Some(feats::FORCE_IMMUNITY_STUN),
            Hold::Paralyze => Some(feats::FORCE_IMMUNITY_PARALYSIS),
            Hold::Fear => Some(feats::FORCE_IMMUNITY_FEAR),
            Hold::Sleep | Hold::DroidStun | Hold::Choke => None,
        }
    }

    /// The KOTOR state it stands for (`combat::effects::state`), whose
    /// visual (`state::visual`) is what the games show as it takes hold: a
    /// stun's `VFX_DUR_STUN`, a droid's stun's `VFX_IMP_DROID_DISABLE`,
    /// nothing for paralysis, horror, sleep, a whirlwind or a choke.
    pub fn state(self) -> u8 {
        use crate::combat::effects::state;
        match self {
            Hold::Stun => state::STUNNED,
            Hold::Paralyze => state::PARALYZED,
            Hold::Fear => state::HORRIFIED,
            Hold::Sleep => state::SLEEP,
            Hold::DroidStun => state::DROID_STUN,
            Hold::Whirlwind => state::WHIRLWIND,
            Hold::Choke => state::CHOKE,
        }
    }

    const ALL: [Hold; 7] = [
        Hold::Stun,
        Hold::Paralyze,
        Hold::Fear,
        Hold::Sleep,
        Hold::DroidStun,
        Hold::Whirlwind,
        Hold::Choke,
    ];

    /// Its byte in a character's Force state (`ForceState::hold`): a stun
    /// 0 and fear 1, as the fear flag was before it.
    pub(crate) fn code(self) -> u8 {
        match self {
            Hold::Stun => 0,
            Hold::Fear => 1,
            Hold::Paralyze => 2,
            Hold::Sleep => 3,
            Hold::DroidStun => 4,
            Hold::Whirlwind => 5,
            Hold::Choke => 6,
        }
    }

    pub(crate) fn of_code(code: u8) -> Option<Hold> {
        [
            Hold::Stun,
            Hold::Fear,
            Hold::Paralyze,
            Hold::Sleep,
            Hold::DroidStun,
            Hold::Whirlwind,
            Hold::Choke,
        ]
        .get(usize::from(code))
        .copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Whole HP, of a `DAMAGE_TYPE_*`.
    Damage {
        amount: u16,
        kind: u16,
    },
    Heal(u16),
    Death,
    /// Whole FP given or taken.
    HealForcePoints(u16),
    DamageForcePoints(u16),
    /// A lasting change (`combat::effects::Lasting`): what, by how much,
    /// of what.
    Modifier {
        modifies: Modifies,
        amount: i16,
        of: u8,
    },
    Hold(Hold),
    /// A KOTOR state (`combat::effects::state`): its number, and whether
    /// a cutscene's (no immunity keeps it off). Lasting, in a slot
    /// (`Modifies::State`).
    State {
        state: u8,
        cutscene: bool,
    },
    /// An energy shield for its time (`EffectForceShield`, a
    /// `forceshields.2da` row's numbers): it takes up to `points` of the
    /// damage types `types` (`DAMAGE_TYPE_*` bits) in all.
    Shield {
        types: u16,
        points: u16,
    },
    /// A downed party member up again (`EffectResurrection`).
    Resurrection,
    /// A poison's course (`EffectPoison`, a `poison.2da` row's).
    Poison(crate::combat::items::Course),
    /// Thrown back and knocked down, away from `from` (metres;
    /// `EffectForcePushTargeted`'s centre), else from whoever applied it
    /// (`EffectForcePushed`): a script's push outside a cast, which no
    /// power's delivery has shoved already.
    Pushed {
        from: Option<[f32; 3]>,
    },
    /// Seen only: a `visualeffects.2da` row (`EffectVisualEffect`; a beam
    /// from `from` to the body part `part`, `EffectBeam`), carried to the
    /// views as an event (`combat::fighter::Visual`); none for feedback
    /// nothing draws yet (`EffectForceResisted`, ...).
    Visual {
        row: Option<u16>,
        from: Option<ObjectId>,
        part: u8,
    },
    /// A lasting effect on a character (`GetFirstEffect`): whose, its id.
    Slot {
        owner: ObjectId,
        id: u32,
    },
    /// `EffectLinkEffects`: its parts, in order.
    Linked(Vec<Effect>),
    /// A kind the sim does not carry out yet: its routine's name.
    Other(String),
}

impl Effect {
    /// Its parts, a link's flattened.
    pub fn leaves(&self) -> Vec<&Effect> {
        match self {
            Effect::Linked(parts) => parts.iter().flat_map(Effect::leaves).collect(),
            leaf => vec![leaf],
        }
    }

    /// What it is called in a report.
    pub fn name(&self) -> String {
        match self {
            Effect::Damage { .. } => "EffectDamage".into(),
            Effect::Heal(_) => "EffectHeal".into(),
            Effect::Death => "EffectDeath".into(),
            Effect::HealForcePoints(_) => "EffectHealForcePoints".into(),
            Effect::DamageForcePoints(_) => "EffectDamageForcePoints".into(),
            Effect::Linked(_) => "EffectLinkEffects".into(),
            Effect::Modifier { modifies, .. } => format!("Effect{modifies:?}"),
            Effect::Hold(hold) => format!("Effect{hold:?}"),
            Effect::State { state, .. } => format!("a state ({state})"),
            Effect::Shield { .. } => "EffectForceShield".into(),
            Effect::Resurrection => "EffectResurrection".into(),
            Effect::Poison(_) => "EffectPoison".into(),
            Effect::Pushed { from: Some(_) } => "EffectForcePushTargeted".into(),
            Effect::Pushed { from: None } => "EffectForcePushed".into(),
            Effect::Visual { .. } => "EffectVisualEffect".into(),
            Effect::Slot { .. } => "an effect on a character".into(),
            Effect::Other(name) => name.clone(),
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        match self {
            Effect::Damage { amount, kind } => {
                out.u8(0);
                out.u16(*amount);
                out.u16(*kind);
            }
            Effect::Heal(amount) => {
                out.u8(1);
                out.u16(*amount);
            }
            Effect::Death => out.u8(2),
            Effect::HealForcePoints(amount) => {
                out.u8(3);
                out.u16(*amount);
            }
            Effect::DamageForcePoints(amount) => {
                out.u8(4);
                out.u16(*amount);
            }
            Effect::Linked(parts) => {
                out.u8(5);
                out.u32(parts.len() as u32);
                for part in parts {
                    part.encode(out);
                }
            }
            Effect::Other(name) => {
                out.u8(6);
                out.string(name);
            }
            Effect::Modifier {
                modifies,
                amount,
                of,
            } => {
                out.u8(7);
                out.u8(modifies.index());
                out.u16(*amount as u16);
                out.u8(*of);
            }
            Effect::Hold(hold) => {
                out.u8(8);
                out.u8(Hold::ALL.iter().position(|h| h == hold).unwrap_or(0) as u8);
            }
            Effect::Visual { row, from, part } => {
                out.u8(9);
                out.u16(row.unwrap_or(u16::MAX));
                out.u32(from.map_or(u32::MAX, |id| id.0));
                out.u8(*part);
            }
            Effect::Slot { owner, id } => {
                out.u8(10);
                out.u32(owner.0);
                out.u32(*id);
            }
            Effect::Shield { types, points } => {
                out.u8(11);
                out.u16(*types);
                out.u16(*points);
            }
            Effect::Resurrection => out.u8(12),
            Effect::Poison(course) => {
                out.u8(13);
                course.encode(out);
            }
            Effect::State { state, cutscene } => {
                out.u8(14);
                out.u8(*state);
                out.u8(u8::from(*cutscene));
            }
            Effect::Pushed { from } => {
                out.u8(15);
                out.u8(u8::from(from.is_some()));
                for axis in from.unwrap_or([0.0; 3]) {
                    out.f32(axis);
                }
            }
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Effect, DecodeError> {
        let at = input.at();
        Ok(match input.u8()? {
            0 => Effect::Damage {
                amount: input.u16()?,
                kind: input.u16()?,
            },
            1 => Effect::Heal(input.u16()?),
            2 => Effect::Death,
            3 => Effect::HealForcePoints(input.u16()?),
            4 => Effect::DamageForcePoints(input.u16()?),
            5 => Effect::Linked(
                (0..input.u32()?)
                    .map(|_| Effect::decode(input))
                    .collect::<Result<_, _>>()?,
            ),
            6 => Effect::Other(input.string()?),
            7 => Effect::Modifier {
                modifies: Modifies::of_index(input.u8()?)
                    .ok_or_else(|| input.invalid("effect kind", at))?,
                amount: input.u16()? as i16,
                of: input.u8()?,
            },
            8 => Effect::Hold(
                *Hold::ALL
                    .get(usize::from(input.u8()?))
                    .ok_or_else(|| input.invalid("hold", at))?,
            ),
            9 => Effect::Visual {
                row: Some(input.u16()?).filter(|row| *row != u16::MAX),
                from: Some(input.u32()?)
                    .filter(|id| *id != u32::MAX)
                    .map(ObjectId),
                part: input.u8()?,
            },
            10 => Effect::Slot {
                owner: ObjectId(input.u32()?),
                id: input.u32()?,
            },
            11 => Effect::Shield {
                types: input.u16()?,
                points: input.u16()?,
            },
            12 => Effect::Resurrection,
            13 => Effect::Poison(crate::combat::items::Course::decode(input)?),
            14 => Effect::State {
                state: input.u8()?,
                cutscene: input.u8()? != 0,
            },
            15 => {
                let placed = input.u8()? != 0;
                let from = [input.f32()?, input.f32()?, input.f32()?];
                Effect::Pushed {
                    from: placed.then_some(from),
                }
            }
            _ => return Err(input.invalid("effect", at)),
        })
    }
}

/// An effect applied, waiting for the next step's pipeline: on whom, by
/// whom (the script's caller), what, until when (sim ms; `None` at once,
/// `u64::MAX` until removed), from which spell row (`u16::MAX` none), and
/// the id its lasting parts take.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub target: ObjectId,
    pub by: ObjectId,
    pub effect: Effect,
    pub until: Option<u64>,
    pub spell: u16,
    pub id: u32,
    /// Where it was put (metres; `ApplyEffectAtLocation`), if at a place.
    pub at: Option<[f32; 3]>,
}

impl Applied {
    /// The `spell` of a scene's melee blow landing (`Strike::applied`): no
    /// `spells.2da` row, it marks the damage as struck with the attacker's
    /// weapon in hand.
    pub const BLOW: u16 = u16::MAX - 1;

    /// Whether a spell's impact applied it (a power, a grenade).
    pub fn cast(&self) -> bool {
        !matches!(self.spell, u16::MAX | Self::BLOW)
    }

    /// Whether it is a scene's melee blow landing (`BLOW`).
    pub fn blow(&self) -> bool {
        self.spell == Self::BLOW
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.target.0);
        out.u32(self.by.0);
        self.effect.encode(out);
        out.u64(self.until.unwrap_or(0));
        out.u8(u8::from(self.until.is_some()));
        out.u16(self.spell);
        out.u32(self.id);
        out.u8(u8::from(self.at.is_some()));
        for v in self.at.unwrap_or([0.0; 3]) {
            out.f32(v);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Applied, DecodeError> {
        let target = ObjectId(input.u32()?);
        let by = ObjectId(input.u32()?);
        let effect = Effect::decode(input)?;
        let until = input.u64()?;
        let lasting = input.u8()? != 0;
        Ok(Applied {
            target,
            by,
            effect,
            until: lasting.then_some(until),
            spell: input.u16()?,
            id: input.u32()?,
            at: {
                let placed = input.u8()? != 0;
                let at = [input.f32()?, input.f32()?, input.f32()?];
                placed.then_some(at)
            },
        })
    }
}

/// A blow decided before it lands: a script's (`CutsceneAttack`, an
/// assured attack) or a round between creatures (`world::minds`). Its
/// damage lands at the attack's hit, `due_ms`, `hit_ms` into the swing
/// (`combat::rounds`; a gun's and a `CutsceneAttack`'s half a round in),
/// not when it was asked for. The attacker plays its row (`animation`)
/// from `starts_ms`, its target the row the round chose for it (`answer`,
/// `blows`); a target that is attacking it back has the other half of the
/// round. A gunner fires it as shots (`world::minds`): a clip with a
/// `weapondischarge.2da` row fires each of its bolts as the clip has the
/// gun fire it (`shots`, the moments the exe sends its projectiles at),
/// the last one carrying the blow; one without fires one bolt that meets
/// the target as the blow falls due. The bolt carrying it lands the damage
/// where it meets the target, the target struck then (one that never meets
/// it lands the blow `SHOT_GRACE_MS` after it falls due); the others, and
/// every bolt of a miss, are fired past the target, which dodges, and land
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strike {
    pub target: ObjectId,
    pub by: ObjectId,
    pub amount: u16,
    pub due_ms: u64,
    /// An assured attack's (`EffectAssuredHit`), which its attacker's own
    /// swing landing meanwhile stands in for; else a `CutsceneAttack`'s.
    pub assured: bool,
    /// The script's result is a hit (a miss's damage never lands).
    pub landed: bool,
    /// The `animations.2da` row its attacker swings (`combat::rounds`; the
    /// script's own for a `CutsceneAttack`); none (0) where a script names
    /// none.
    pub animation: u16,
    /// The row its target answers with (`combat::rounds::Table::answer`),
    /// `combat::rounds::NONE` for none.
    pub answer: u16,
    /// How long into the swing the blow lands (ms; the table's `hit1`).
    pub hit_ms: u16,
    /// Whether the bodies have been given their animations (`began`).
    pub began: bool,
    /// When its gunner's bolts leave (`Discharge`), set as it begins.
    pub shots: Discharge,
}

/// When each bolt of a gun's attack clip leaves the gun: ms after the clip
/// begins, in order (`weapondischarge.2da`'s `shot1` to `shot12` for the
/// clip's `animations.2da` row, a creature model's own row first:
/// `Library::discharge`), and which hand fires it (its `switchmask`: a
/// shot's `1` the left gun of a pair, as the two pistols' clips `b6a*`
/// mark theirs and every one-gun clip leaves its mask empty). None for a
/// clip the table has no row for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Discharge {
    times: [u16; Discharge::MOST],
    count: u8,
    /// Bit n: the nth shot (in order) leaves the left hand.
    left: u16,
}

impl Discharge {
    /// The most bolts a clip fires (the table's columns).
    pub const MOST: usize = 12;

    /// `times` (ms after the clip begins), the first `MOST` of them, sorted,
    /// all from the right hand.
    pub fn new(times: &[u16]) -> Discharge {
        Discharge::of(times, 0)
    }

    /// `times` as `new` has them, bit n of `left` the nth time's shot from
    /// the left hand (the table's order, kept through the sort).
    pub fn of(times: &[u16], left: u16) -> Discharge {
        let mut shots: Vec<(u16, bool)> = times
            .iter()
            .take(Discharge::MOST)
            .enumerate()
            .map(|(at, &ms)| (ms, left >> at & 1 != 0))
            .collect();
        shots.sort_by_key(|&(ms, _)| ms);
        let mut discharge = Discharge::default();
        for (at, (ms, left)) in shots.into_iter().enumerate() {
            discharge.times[at] = ms;
            discharge.left |= u16::from(left) << at;
            discharge.count += 1;
        }
        discharge
    }

    /// `weapondischarge.2da`'s `switchmask` cell (`101010`: a character a
    /// shot, in order) as `of` takes it.
    pub fn mask(cell: &str) -> u16 {
        cell.chars()
            .take(Discharge::MOST)
            .enumerate()
            .fold(0, |bits, (at, c)| bits | u16::from(c == '1') << at)
    }

    /// The times (ms after the clip begins).
    pub fn times(&self) -> &[u16] {
        &self.times[..usize::from(self.count)]
    }

    /// The hand (0 the right, 1 the left) the `index`th shot leaves.
    pub fn hand(&self, index: usize) -> u8 {
        (self.left >> index.min(Discharge::MOST) & 1) as u8
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.count);
        for ms in self.times() {
            out.u16(*ms);
        }
        out.u16(self.left);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Discharge, DecodeError> {
        let count = usize::from(input.u8()?).min(Discharge::MOST);
        let mut times = [0; Discharge::MOST];
        for ms in times.iter_mut().take(count) {
            *ms = input.u16()?;
        }
        let left = input.u16()?;
        Ok(Discharge::of(&times[..count], left))
    }
}

/// How long after a shot's blow falls due it waits on its bolt before it
/// lands without it (ms; the book's `force.routines.shot_grace_ms`).
pub const SHOT_GRACE_MS: u64 = rules::force::routines::SHOT_GRACE_MS;

/// The animations a blow plays on bodies (`Acting::animation`): a kind
/// plus the `animations.2da` row whose own clip the views draw: the
/// attacker's swing (`SWING`), its target's answer to a hit (`HURT`) or to
/// a blow turned aside (`GUARD`), the answer's row the round chose
/// (`combat::rounds::Table::answer`).
pub mod blows {
    /// The attacker's swing.
    pub const SWING: u16 = 1000;
    /// Its target struck by it.
    pub const HURT: u16 = 2000;
    /// Its target parrying or dodging it.
    pub const GUARD: u16 = 3000;
    /// Rows are below this.
    pub const ROWS: u16 = 1000;

    /// What `animation` plays and its row, if it is one of these.
    pub fn of(animation: u16) -> Option<(u16, u16)> {
        let kind = animation / ROWS * ROWS;
        matches!(kind, SWING | HURT | GUARD).then_some((kind, animation - kind))
    }

    /// The target of a scene's shot, struck at `now` (its bolt landed, or
    /// its grace over without it): it plays its answer `row` (`HURT`), as
    /// a melee blow's target does, unless its own attack plays, the blow
    /// killed it (it plays its death) or the table gives it none.
    pub fn struck(target: &mut crate::combat::fighter::Fighter, row: u16, now: u64) {
        let attacking = target.acting.is_some_and(|acting| {
            acting.playing(now) && of(acting.animation).is_some_and(|(kind, _)| kind == SWING)
        });
        if row < ROWS && !attacking && !target.dead() {
            target.acting = Some(crate::combat::idle::Acting {
                animation: HURT + row,
                since: now,
                ms: (crate::script::actions::ROUND_MS / 2) as u32,
            });
        }
    }
}

impl Strike {
    /// When its swing begins (sim ms): its hit's moment into the swing
    /// before the hit.
    pub fn starts_ms(&self) -> u64 {
        self.due_ms.saturating_sub(u64::from(self.hit_ms))
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.target.0);
        out.u32(self.by.0);
        out.u16(self.amount);
        out.u64(self.due_ms);
        out.u8(u8::from(self.assured));
        out.u8(u8::from(self.landed));
        out.u16(self.animation);
        out.u16(self.answer);
        out.u16(self.hit_ms);
        out.u8(u8::from(self.began));
        self.shots.encode(out);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Strike, DecodeError> {
        Ok(Strike {
            target: ObjectId(input.u32()?),
            by: ObjectId(input.u32()?),
            amount: input.u16()?,
            due_ms: input.u64()?,
            assured: input.u8()? != 0,
            landed: input.u8()? != 0,
            animation: input.u16()?,
            answer: input.u16()?,
            hit_ms: input.u16()?,
            began: input.u8()? != 0,
            shots: Discharge::decode(input)?,
        })
    }

    /// When (sim ms) each of its bolts leaves the gun, its clip begun at
    /// `starts_ms`.
    pub fn shot_times(&self) -> impl Iterator<Item = u64> + '_ {
        let starts = self.starts_ms();
        self.shots
            .times()
            .iter()
            .map(move |ms| starts + u64::from(*ms))
    }

    /// Its damage, as the step's effects take it.
    pub(crate) fn applied(&self) -> Applied {
        Applied {
            target: self.target,
            by: self.by,
            effect: Effect::Damage {
                amount: self.amount,
                kind: 0,
            },
            until: None,
            spell: Applied::BLOW,
            id: 0,
            at: None,
        }
    }
}

/// A whole amount from a script's int (negative as none).
pub(crate) fn amount(value: i32) -> u16 {
    value.clamp(0, i32::from(u16::MAX)) as u16
}
