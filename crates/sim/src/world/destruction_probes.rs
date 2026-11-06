//! Blows on destructible props (`kjk sim probes --only destruction`): the
//! design's feel table through `destruction::worth`, and blows queued on the
//! proving ground, whose gate stands in as a chair with its brushes, beside
//! a damage-only panel, a plot chair and a brute, and on an arena's made
//! chair.

use std::ops::RangeInclusive;

use super::objects::{Battered, Battering};
use super::{Id, Object, SimWorld};
use crate::combat::damage::{self, Source};
use crate::combat::sheet::{Weapon, damage_types};
use crate::destruction::{self, Class, Destructible, most_hp, worth};
use crate::module::{ObjectData, ObjectKind, Placement, Toughness};
use crate::probe::check;
use crate::snapshot::{ObjectState, SimId};
use crate::{StepReason, TickInput, WorldConfig};

/// K1's rows: a luxurious chair (metal, medium), a computer panel
/// (electronic, damage only), a ceramic jar (small).
const CHAIR: u16 = 20;
const PANEL: u16 = 23;
const JAR: u16 = 179;

fn kind(row: u16) -> Result<Destructible, String> {
    destruction::catalogue("k1", row).ok_or(format!("k1/{row} is not destructible"))
}

/// A source's average blow (hundredths), its damage-type row, and whether
/// it is an area's.
type Arm = (&'static str, i32, Option<u8>, bool);

const PISTOL: Arm = ("blaster pistol 1d6", 350, Some(12), false);
const HEAVY_PISTOL: Arm = ("heavy pistol 1d8", 450, Some(12), false);
const VIBROBLADE: Arm = ("vibroblade 2d6+2", 900, Some(2), false);
const SABER: Arm = ("lightsaber 2d8+2", 1100, Some(12), false);
const FISTS: Arm = ("fists 1d3+2", 400, Some(0), false);
const FRAG: Arm = ("frag grenade 20", 2000, Some(1), true);

/// Blows of `arm` it takes to break `d` (or wreck it, damage only); `None`
/// when they never count.
fn hits_to_break((_, average, damage_type, area): Arm, d: Destructible) -> Option<i32> {
    let each = worth(average, damage_type, area, d);
    (each > 0).then(|| (most_hp(d) * 100 + each - 1) / each)
}

fn feel() -> Result<String, String> {
    let (chair, jar) = (kind(CHAIR)?, kind(JAR)?);
    // The design's table: hits to break, `None` never.
    let table: [(Arm, Destructible, Option<RangeInclusive<i32>>); 8] = [
        (PISTOL, chair, Some(6..=10)),
        (PISTOL, jar, Some(1..=2)),
        (HEAVY_PISTOL, chair, Some(3..=5)),
        (VIBROBLADE, chair, Some(3..=4)),
        (SABER, chair, Some(2..=2)),
        (FISTS, chair, None),
        (FRAG, jar, Some(1..=1)),
        (FRAG, chair, Some(1..=1)),
    ];
    let mut seen = Vec::new();
    let mut ok = true;
    for (arm, d, want) in table {
        let hits = hits_to_break(arm, d);
        ok &= match (&want, hits) {
            (Some(range), Some(hits)) => range.contains(&hits),
            (None, None) => true,
            _ => false,
        };
        seen.push(format!(
            "{} on {:?} {:?}: {hits:?} (want {want:?})",
            arm.0, d.material, d.size
        ));
    }
    check(ok, seen.join("; "))
}

/// A placeable of K1's row `row` at `at`, as loading makes it
/// (`destruction::fill`), `plot` or not.
fn prop(row: u16, at: [f32; 3], plot: bool) -> ObjectData {
    let mut facts = ObjectData {
        kind: ObjectKind::Placeable,
        tag: format!("prop{row}"),
        hit_points: Some((15, 15)),
        toughness: Toughness {
            hardness: 5,
            plot,
            ..Toughness::default()
        },
        ..ObjectData::unknown(&format!("prop{row}"), at, 0.0)
    };
    destruction::fill(&mut facts, "k1", row, |_| false);
    facts
}

/// The proving ground's entities in the blows probe: the gate made a chair
/// (its brushes the level's entity 0), then after the trigger a panel, a
/// plot chair (destructible had the guard missed: it keeps its kind) and
/// the brute who strikes.
const GATE_CHAIR: u32 = 0;
const PANEL_AT: u32 = 2;
const PLOT_CHAIR: u32 = 3;
const BRUTE: u32 = 4;

fn ground() -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let Some(Placement::Own(gate)) = data.placements.first_mut() else {
        return Err("the ground's first object is not its door".into());
    };
    *gate = prop(CHAIR, gate.position, false);
    let floor = |x, y| crate::determinism::ground_point(x, y, 0.0);
    let mut plot_chair = prop(CHAIR, floor(-4.0, -4.0), true);
    plot_chair.destructible = destruction::catalogue("k1", CHAIR);
    data.placements.extend(
        [
            prop(PANEL, floor(-6.0, -4.0), false),
            plot_chair,
            ObjectData::creature(
                "brute",
                "brute",
                floor(-5.0, 3.0),
                crate::combat::Sheet::commoner(),
            ),
        ]
        .map(Placement::Own),
    );
    let library = crate::probe::library("", std::iter::empty::<(String, _)>());
    SimWorld::new(
        WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())
}

/// The id of the object numbered `index`.
fn id_of(world: &mut SimWorld, index: u32) -> Option<SimId> {
    let mut query = world.world.query::<(&Id, &Object)>();
    query
        .iter(&world.world)
        .find(|(_, object)| object.index == index)
        .map(|(id, _)| id.0)
}

/// The state and facts of the object numbered `index`.
fn object_of(world: &mut SimWorld, index: u32) -> Option<(ObjectState, ObjectData)> {
    let mut query = world.world.query::<&Object>();
    query
        .iter(&world.world)
        .find(|object| object.index == index)
        .and_then(|object| Some((object.state, (**object.facts.as_ref()?).clone())))
}

/// A blaster pistol's shot (1d6 energy), a whole round's worth.
fn pistol() -> Source {
    Source::Weapon {
        weapon: Weapon {
            dice: 1,
            die: 6,
            damage_types: damage_types::ENERGY,
            ..Weapon::UNARMED
        },
        cadence_ms: damage::WHOLE_CADENCE_MS,
        share: 1000,
        area: false,
    }
}

/// One step with `by`'s `source` blow queued on the object numbered
/// `index`, as a shot stopped on it, stepped for `reason`: what the step
/// battered.
fn blow(
    world: &mut SimWorld,
    index: u32,
    by: SimId,
    source: Source,
    reason: StepReason,
) -> Vec<(SimId, u16, bool)> {
    world
        .world
        .resource_mut::<Battering>()
        .shot(Some(index), by, source);
    let tick = world.tick() + 1;
    crate::step(world, tick, &TickInput::default(), 50, reason);
    let battered = world.world.resource::<Battered>();
    battered
        .0
        .iter()
        .map(|taken| (taken.object, taken.lost, taken.broke))
        .collect()
}

/// The most blows any one prop takes in the probe.
const MOST_BLOWS: u32 = 500;

fn blows() -> Result<String, String> {
    let mut world = ground()?;
    let brute = id_of(&mut world, BRUTE).ok_or("no brute")?;
    let lost = |world: &mut SimWorld, index| object_of(world, index).map(|(state, _)| state.damage);
    // The chair the gate became: the brute's pistol breaks it.
    let mut chair_hits = None;
    for hit in 1..=MOST_BLOWS {
        let taken = blow(
            &mut world,
            GATE_CHAIR,
            brute,
            pistol(),
            StepReason::Authority,
        );
        if taken.iter().any(|(_, _, broke)| *broke) {
            chair_hits = Some(hit);
            break;
        }
    }
    let (chair, chair_facts) = object_of(&mut world, GATE_CHAIR).ok_or("no chair")?;
    let passable = chair.passable(&chair_facts);
    let slot_gone = world
        .level()
        .states([(GATE_CHAIR, &chair)].into_iter())
        .is_some_and(|states| states.first() == Some(&collision::GONE));
    let again = blow(
        &mut world,
        GATE_CHAIR,
        brute,
        pistol(),
        StepReason::Authority,
    );
    // The plot chair takes nothing.
    for _ in 0..20 {
        blow(
            &mut world,
            PLOT_CHAIR,
            brute,
            pistol(),
            StepReason::Authority,
        );
    }
    let plot_lost = lost(&mut world, PLOT_CHAIR);
    // The panel only shows damage: never past 1 hit point left.
    let panel_kind = kind(PANEL)?;
    let before = lost(&mut world, PANEL_AT);
    let predicted = blow(&mut world, PANEL_AT, brute, pistol(), StepReason::Predict);
    let after_predict = lost(&mut world, PANEL_AT);
    for _ in 0..MOST_BLOWS {
        blow(&mut world, PANEL_AT, brute, pistol(), StepReason::Authority);
    }
    let (panel, panel_facts) = object_of(&mut world, PANEL_AT).ok_or("no panel")?;
    let panel_left = panel.hit_points(&panel_facts).map(|(now, _)| now);
    let panel_stage = destruction::stage(panel_kind, panel.damage);
    let panel_broken = panel.broken(&panel_facts) || panel.passable(&panel_facts);
    check(
        chair_hits.is_some_and(|hits| (6..=10).contains(&hits))
            && passable
            && slot_gone
            && again.is_empty()
            && plot_lost == Some(0)
            && before == Some(0)
            && predicted.is_empty()
            && after_predict == Some(0)
            && panel_kind.class == Class::Damage
            && panel_left == Some(1)
            && panel_stage == 2
            && !panel_broken,
        format!(
            "the chair broke at pistol shot {chair_hits:?} (want 6 to 10), passable {passable}, \
             its brushes gone {slot_gone}, a blow after {again:?} (want none); the plot \
             chair lost {plot_lost:?} (want 0); a predicted blow on the panel {predicted:?} \
             left it at {after_predict:?} lost (want 0); after {MOST_BLOWS} shots the panel \
             has {panel_left:?} HP (want 1), stage {panel_stage} (want 2), broken \
             {panel_broken}"
        ),
    )
}

/// A frag grenade's blast 2 m before the gate made a chair reaches it,
/// though the gate's own brushes stand between the blast and its middle.
fn blast_on_brushes() -> Result<String, String> {
    let mut world = ground()?;
    let chair = id_of(&mut world, GATE_CHAIR).ok_or("no chair")?;
    let at = crate::math::scale(
        crate::determinism::ground_point(-2.0, 0.0, 0.0),
        1.0 / collision::UNIT,
    );
    let mut query = world.world.query::<(&Id, &Object)>();
    let objects: Vec<(&Id, &Object)> = query.iter(&world.world).collect();
    let reached = super::objects::blast_reach(world.level(), None, objects.into_iter(), at);
    let caught = reached.iter().any(|(id, _)| *id == chair);
    check(
        caught,
        format!("a blast 2 m before the gate chair caught {reached:?} (want the chair {chair:?})"),
    )
}

/// An arena's made chair, broken, no longer collides as its footprint.
fn made_chair() -> Result<String, String> {
    let at = crate::determinism::ground_point(3.0, 4.0, 0.0);
    let props = vec![super::arena::Prop {
        appearance: CHAIR,
        name: "chair".into(),
        at,
        yaw: 270.0,
        footprint: vec![[-0.5, -0.5, 0.0], [0.5, 0.5, 1.0]],
        destructible: destruction::catalogue("k1", CHAIR),
    }];
    let mut world = super::arena_probes::arena_with(props)?.0;
    // Laid out on the first steps.
    for _ in 0..4 {
        let tick = world.tick() + 1;
        crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
    }
    let placed = world.level().placed().len();
    let made = {
        let mut query = world.world.query::<(&Id, &Object)>();
        query
            .iter(&world.world)
            .find(|(_, object)| object.kind == ObjectKind::Placeable)
            .map(|(id, object)| (id.0, object.index))
    };
    let (id, index) = made.ok_or("the arena made no chair")?;
    let bodies = |world: &mut SimWorld| {
        let mut query = world.world.query::<(&Id, &Object)>();
        query
            .iter(&world.world)
            .find(|(own, _)| own.0 == id)
            .map_or(0, |(_, object)| {
                super::made_bodies(id, object, at, placed).len()
            })
    };
    let standing = bodies(&mut world);
    let mut hits = 0;
    while hits < MOST_BLOWS
        && !blow(&mut world, index, id, pistol(), StepReason::Authority)
            .iter()
            .any(|(_, _, broke)| *broke)
    {
        hits += 1;
    }
    let broken = bodies(&mut world);
    check(
        standing > 0 && hits < MOST_BLOWS && broken == 0,
        format!(
            "the made chair collides as {standing} boxes standing, {broken} broken (want 0) \
             after {} shots",
            hits + 1
        ),
    )
}

/// The arena's made props of `props` (row, where on the ground), laid out:
/// the world and each one's id and number, in order.
fn arena_props(props: &[(u16, [f32; 2])]) -> Result<(SimWorld, Vec<(SimId, u32)>), String> {
    let laid: Vec<super::arena::Prop> = props
        .iter()
        .map(|&(row, [x, y])| super::arena::Prop {
            appearance: row,
            name: format!("prop {row}"),
            at: crate::determinism::ground_point(x, y, 0.0),
            yaw: 270.0,
            footprint: vec![[-0.4, -0.4, 0.0], [0.4, 0.4, 1.0]],
            destructible: destruction::catalogue("k1", row),
        })
        .collect();
    let mut world = super::arena_probes::arena_with(laid.clone())?.0;
    for _ in 0..4 {
        let tick = world.tick() + 1;
        crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
    }
    let mut query = world.world.query::<(&Id, &Object)>();
    let made: Vec<(SimId, u32, [f32; 3])> = query
        .iter(&world.world)
        .filter_map(|(id, object)| {
            let facts = object.facts.as_ref()?;
            (object.kind == ObjectKind::Placeable).then_some((id.0, object.index, facts.position))
        })
        .collect();
    let ids = laid
        .iter()
        .map(|prop| {
            made.iter()
                .find(|(.., at)| (at[0] - prop.at[0]).hypot(at[1] - prop.at[1]) < 0.01)
                .map(|&(id, index, _)| (id, index))
                .ok_or(format!("{} was not laid out", prop.name))
        })
        .collect::<Result<_, _>>()?;
    Ok((world, ids))
}

/// One step with `row`'s power cast by seat 0's player from `from` toward
/// `toward` (both on the ground, metres).
fn power(world: &mut SimWorld, row: u16, from: [f32; 2], toward: [f32; 2]) {
    let caster = {
        let mut query = world.world.query::<(&Id, &super::Avatar)>();
        query.iter(&world.world).next().map(|(id, _)| id.0)
    };
    let point = |[x, y]: [f32; 2]| crate::determinism::ground_point(x, y, 1.0);
    if let Some(caster) = caster {
        world
            .world
            .resource_mut::<Battering>()
            .power(caster, row, point(from), point(toward));
    }
    let tick = world.tick() + 1;
    crate::step(
        world,
        tick,
        &TickInput::default(),
        50,
        StepReason::Authority,
    );
}

fn powers() -> Result<String, String> {
    use crate::combat::spells;
    const FROM: [f32; 2] = [-7.0, -3.0];
    let chairs = [[-4.0, -3.0], [-2.0, -3.0], [2.0, -3.0]];
    let lay: Vec<(u16, [f32; 2])> = chairs.iter().map(|&at| (CHAIR, at)).collect();
    let state = |world: &mut SimWorld, index| object_of(world, index).map(|(state, _)| state);
    let broken = |world: &mut SimWorld, ids: &[(SimId, u32)]| {
        ids.iter()
            .map(|&(_, index)| {
                object_of(world, index).is_some_and(|(state, facts)| state.broken(&facts))
            })
            .collect::<Vec<_>>()
    };
    // Force Wave round the caster: the chairs 3 and 5 m off, not 9.
    let (mut world, ids) = arena_props(&lay)?;
    power(&mut world, spells::FORCE_POWER_FORCE_WAVE, FROM, chairs[0]);
    let waved = broken(&mut world, &ids);
    // Force Push at the nearest: it alone.
    let (mut world, ids) = arena_props(&lay)?;
    power(&mut world, spells::FORCE_POWER_FORCE_PUSH, FROM, chairs[0]);
    let pushed = broken(&mut world, &ids);
    // Force Lightning at a panel 2 m off: wrecked within three casts.
    let panel_at = [-7.0, -1.0];
    let (mut world, ids) = arena_props(&[(PANEL, panel_at)])?;
    for _ in 0..3 {
        power(&mut world, spells::FORCE_POWER_LIGHTNING, FROM, panel_at);
    }
    let panel_kind = kind(PANEL)?;
    let panel =
        state(&mut world, ids[0].1).map(|state| destruction::stage(panel_kind, state.damage));
    check(
        waved == [true, true, false] && pushed == [true, false, false] && panel == Some(2),
        format!(
            "Force Wave broke {waved:?} (want [true, true, false]); Force Push {pushed:?} \
             (want [true, false, false]); three Force Lightnings left the panel at stage \
             {panel:?} (want 2)"
        ),
    )
}

/// The probes.
pub fn probes() -> crate::probe::Checks {
    vec![
        ("destruction_feel", feel()),
        ("destruction_blows", blows()),
        ("destruction_made_chair", made_chair()),
        ("destruction_blast_on_brushes", blast_on_brushes()),
        ("destruction_powers", powers()),
    ]
}
