//! Probes with known answers: traces whose right result is known, on
//! made-up levels (`synthetic`), with what each checks and whether it
//! passed. `kjk collision probes` prints them; the approved test
//! `collision_probes` requires them all to pass. Probes on a module's own
//! collision are `collision_odyssey::probes`.

use crate::{
    BoxWorld, CONTACT_GAP, Capsule, CollisionWorld, Contents, Extents, Level, LevelBuilder, Shape,
    Solid, Tag, Trace, UNIT,
};

/// One probe: what it checks, the trace, and whether the answer is the
/// known one.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub expect: String,
    pub trace: Option<Trace>,
    pub pass: bool,
}

/// Probes in the order they ran.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probes(pub Vec<Probe>);

impl Probes {
    pub fn check(&mut self, name: &str, expect: &str, trace: Option<Trace>, pass: bool) {
        self.0.push(Probe {
            name: name.to_string(),
            expect: expect.to_string(),
            trace,
            pass,
        });
    }

    pub fn failed(&self) -> usize {
        self.0.iter().filter(|probe| !probe.pass).count()
    }
}

/// JA's standing box (`DEFAULT_MINS/MAXS`).
pub fn player() -> Extents {
    Extents {
        mins: [-15.0 * UNIT, -15.0 * UNIT, -24.0 * UNIT],
        maxs: [15.0 * UNIT, 15.0 * UNIT, 40.0 * UNIT],
    }
}

/// JA's crouched box (`CROUCH_MAXS_2`).
pub fn crouched() -> Extents {
    Extents {
        maxs: [15.0 * UNIT, 15.0 * UNIT, 16.0 * UNIT],
        ..player()
    }
}

/// A trace in a line: how far, where, and what it hit; `label` names the
/// material when the caller knows its table.
pub fn describe(trace: &Trace, label: Option<&str>) -> String {
    let n = trace.plane.normal;
    let mut text = format!(
        "fraction {:.6} end ({:.4}, {:.4}, {:.4})",
        trace.fraction, trace.end[0], trace.end[1], trace.end[2]
    );
    if trace.hit() {
        text += &format!(
            " plane ({:.3}, {:.3}, {:.3}) {:.4}, material {}",
            n[0], n[1], n[2], trace.plane.dist, trace.surface.material
        );
        if let Some(label) = label {
            text += &format!(" ({label})");
        }
        text += &format!(", contents {}", trace.contents.0);
        if let Some(entity) = trace.entity {
            text += &format!(", entity {entity}");
        }
    }
    if trace.start_solid {
        text += ", START SOLID";
    }
    if trace.all_solid {
        text += ", ALL SOLID";
    }
    text
}

pub fn near(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

/// FNV-1a over a trace's fields as bits: a digest of many answers.
pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv::new()
    }
}

impl Fnv {
    pub fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn word(&mut self, word: u32) {
        for byte in word.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn trace(&mut self, t: &Trace) {
        self.word(t.fraction.to_bits());
        for v in t.end.iter().chain(&t.plane.normal).chain([&t.plane.dist]) {
            self.word(v.to_bits());
        }
        self.word(u32::from(t.start_solid) | u32::from(t.all_solid) << 1);
        self.word(u32::from(t.surface.material));
        self.word(t.contents.0);
        self.word(t.entity.unwrap_or(u32::MAX));
    }
}

/// A small generator for the differential probes' traces (PCG-style, so
/// every run draws the same ones).
pub struct Draw(pub u64);

impl Draw {
    /// The next draw, from 0 up to 1.
    pub fn unit(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

/// Probes on made-up levels: the clip itself.
pub fn synthetic() -> Probes {
    let mut probes = Probes::default();
    let player = player();
    let feet = player.mins[2];

    let empty: Level = LevelBuilder::new().build();
    let t = empty.trace([0.0; 3], [1.0; 3], &player, Contents::PLAYER_SOLID);
    probes.check(
        "empty",
        "an empty level lets every trace through",
        Some(t),
        !t.hit(),
    );

    // A floor triangle under a falling player.
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[-4.0, -4.0, 0.0], [4.0, -4.0, 0.0], [0.0, 4.0, 0.0]],
        Tag::level(Contents::SOLID, 10),
    );
    let floor = builder.build();
    let t = floor.trace(
        [0.0, 0.0, 2.0],
        [0.0, 0.0, -2.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    let gap = t.end[2] + feet;
    probes.check(
        "floor",
        "a player dropped on a floor triangle stops CONTACT_GAP (0.125 units) above it, on its plane, material 10",
        Some(t),
        t.fraction < 1.0
            && near(gap, CONTACT_GAP, 1e-5)
            && t.plane.normal == [0.0, 0.0, 1.0]
            && t.surface.material == 10,
    );
    let t = floor.trace(
        [0.0, 0.0, -2.0],
        [0.0, 0.0, 2.0],
        &Extents::POINT,
        Contents::SOLID,
    );
    probes.check(
        "underside",
        "a point rising into the same triangle stops under its slab (1 cm deep): triangles are solid from both sides",
        Some(t),
        t.fraction < 1.0 && near(t.end[2], -0.01 - CONTACT_GAP, 1e-5),
    );
    let t = floor.trace(
        [5.0, 0.0, 2.0],
        [5.0, 0.0, -2.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    probes.check(
        "beside",
        "a player dropped beside the triangle, clear of its edge by more than its half width, misses",
        Some(t),
        !t.hit(),
    );
    // Past the long edge from (4, -4) to (0, 4): the box's corner reaches
    // over it before its centre does; the edge bevels catch that.
    let t = floor.trace(
        [2.0, 0.5, 2.0],
        [2.0, 0.5, -2.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    probes.check(
        "edge bevel",
        "a player whose centre is past the triangle's slanted edge but whose corner is over it lands on it",
        Some(t),
        t.fraction < 1.0 && near(t.end[2] + feet, CONTACT_GAP, 1e-4),
    );
    let t = floor.trace(
        [2.0, 0.5, 2.0],
        [2.0, 0.5, -2.0],
        &Extents::POINT,
        Contents::SOLID,
    );
    probes.check(
        "edge point",
        "a point at the same place misses: the bevels do not widen the triangle itself",
        Some(t),
        !t.hit(),
    );

    // A wall across +X, facing the runner.
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[2.0, 3.0, -1.0], [2.0, -3.0, -1.0], [2.0, 0.0, 4.0]],
        Tag::level(Contents::SOLID, 7),
    );
    let wall = builder.build();
    let t = wall.trace(
        [0.0, 0.0, 0.5],
        [4.0, 0.0, 0.5],
        &player,
        Contents::PLAYER_SOLID,
    );
    probes.check(
        "wall",
        "a player running at a wall stops its half width plus the epsilon short (x = 2 − 0.381 − 0.003175)",
        Some(t),
        near(t.end[0], 2.0 - player.maxs[0] - CONTACT_GAP, 1e-4) && t.plane.normal[0] < -0.99,
    );
    let again = wall.trace(t.end, [4.0, 0.0, 0.5], &player, Contents::PLAYER_SOLID);
    probes.check(
        "wall again",
        "tracing on from where it stopped moves nothing and does not start solid",
        Some(again),
        again.fraction < 1e-6 && !again.start_solid,
    );

    // Solids: start inside, get out; never get out.
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-1.0, -1.0, -1.0],
        [1.0, 1.0, 1.0],
        Tag::level(Contents::SOLID, 0),
    );
    let block = builder.build();
    let t = block.trace(
        [0.0, 0.0, 0.0],
        [3.0, 0.0, 0.0],
        &Extents::POINT,
        Contents::SOLID,
    );
    probes.check(
        "start solid",
        "a point leaving a box it starts in: start solid, not all solid, goes the whole way",
        Some(t),
        t.start_solid && !t.all_solid && t.fraction == 1.0,
    );
    let t = block.trace(
        [0.0, 0.0, 0.0],
        [0.5, 0.0, 0.0],
        &Extents::POINT,
        Contents::SOLID,
    );
    probes.check(
        "all solid",
        "a point that never leaves the box: all solid, fraction 0, end = start",
        Some(t),
        t.all_solid && t.fraction == 0.0 && t.end == [0.0, 0.0, 0.0],
    );
    let t = block.trace(
        [0.0, 0.0, 0.0],
        [0.5, 0.0, 0.0],
        &Extents::POINT,
        Contents::PLAYER_CLIP,
    );
    probes.check(
        "mask",
        "the same trace with a mask the box's contents are not in passes",
        Some(t),
        !t.hit(),
    );
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [1.0, -1.0, -1.0],
        [1.1, 1.0, 1.0],
        Tag::level(Contents::WALK_CLIP, 0),
    );
    let edge_wall = builder.build();
    let walker = edge_wall.trace([0.0; 3], [2.0, 0.0, 0.0], &player, Contents::PLAYER_SOLID);
    let camera = edge_wall.trace(
        [0.0; 3],
        [2.0, 0.0, 0.0],
        &player,
        Contents::SOLID | Contents::PLAYER_CLIP,
    );
    probes.check(
        "walk clip",
        "a walkmesh edge wall stops a walker's move and not the camera's",
        Some(walker),
        walker.hit() && !camera.hit(),
    );

    // The capsule: the same rest height as the box on a floor, closer to
    // a box's corner.
    let capsule = Capsule::from_extents(&player);
    let on_floor = floor.sweep(
        [0.0, 0.0, 2.0],
        [0.0, 0.0, -2.0],
        &Shape::Capsule(capsule),
        Contents::SOLID,
    );
    probes.check(
        "capsule floor",
        "JA's capsule (radius 15 units, half height 32) rests where the box does",
        Some(on_floor),
        near(on_floor.end[2] + feet, CONTACT_GAP, 1e-4),
    );
    // QUIRKS.md Q-0019: JA pushes a brush's planes out by the radius, so
    // at a box's vertical edge the capsule stops where the box does, not
    // where a round body would.
    let diagonal = |shape: &Shape| {
        let d = 0.707_106_77;
        block.sweep(
            [3.0 * d + 1.0, 3.0 * d + 1.0, 0.0],
            [1.0, 1.0, 0.0],
            shape,
            Contents::SOLID,
        )
    };
    let as_box = diagonal(&Shape::Box(player));
    let as_capsule = diagonal(&Shape::Capsule(capsule));
    probes.check(
        "capsule edge",
        "running diagonally at a box's vertical edge, the capsule stops where the box does (Q-0019: square, not round)",
        Some(as_capsule),
        near(as_capsule.end[0], as_box.end[0], 1e-5) && near(as_capsule.end[1], as_box.end[1], 1e-5),
    );
    // A wall leaning back at 45 degrees: the box's lower front edge meets
    // it first, the capsule's lower sphere later.
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[2.0, 3.0, -1.0], [2.0, -3.0, -1.0], [7.0, 0.0, 4.0]],
        Tag::level(Contents::SOLID, 7),
    );
    let slope = builder.build();
    let run =
        |shape: &Shape| slope.sweep([-2.0, 0.0, 0.5], [6.0, 0.0, 0.5], shape, Contents::SOLID);
    let as_box = run(&Shape::Box(player));
    let as_capsule = run(&Shape::Capsule(capsule));
    let expected = {
        // How far each shape reaches from its origin into the wall: the
        // box by its lower front edge, the capsule by its lower sphere
        // (its centre sits above the origin, where the box's centre is).
        let n = std::f32::consts::FRAC_1_SQRT_2;
        let box_support = player.maxs[0] * n + (-feet) * n;
        let capsule_support =
            capsule.radius + n * (capsule.half_height - capsule.radius) - n * capsule.center[2];
        (box_support - capsule_support) / n
    };
    probes.check(
        "capsule slope",
        &format!(
            "running at a wall leaning back 45 degrees, the capsule gets {expected:.3} m further than the box"
        ),
        Some(as_capsule),
        near(as_capsule.end[0] - as_box.end[0], expected, 2e-3),
    );

    // The body round across (`Shape::Upright`): against a face square to
    // the axes it stops where the box does; against an upright wall at 45
    // degrees by its radius, where the box's corner reaches √2 of it.
    let at_face =
        |shape: &Shape| block.sweep([3.0, 0.0, 0.0], [0.0, 0.0, 0.0], shape, Contents::SOLID);
    let (as_box, upright) = (
        at_face(&Shape::Box(player)),
        at_face(&Shape::Upright(player)),
    );
    probes.check(
        "upright face",
        "running square at a face, the body round across stops where the box does",
        Some(upright),
        near(upright.end[0], as_box.end[0], 1e-5),
    );
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[4.0, 0.0, -2.0], [0.0, 4.0, -2.0], [2.0, 2.0, 4.0]],
        Tag::level(Contents::SOLID, 0),
    );
    let diagonal_wall = builder.build();
    let at_wall =
        |shape: &Shape| diagonal_wall.sweep([0.0; 3], [3.0, 3.0, 0.0], shape, Contents::SOLID);
    let (as_box, upright) = (
        at_wall(&Shape::Box(player)),
        at_wall(&Shape::Upright(player)),
    );
    // How far short of the wall each origin stops, along its normal: the
    // box's corner by √2 − 1 of its half-size further than the round body.
    let short = |t: &Trace| (4.0 - t.end[0] - t.end[1]) * std::f32::consts::FRAC_1_SQRT_2;
    let farther = player.maxs[0] * (std::f32::consts::SQRT_2 - 1.0);
    probes.check(
        "upright diagonal",
        &format!(
            "running at an upright wall at 45 degrees, the body round across gets {farther:.3} m closer than the box, whose corner leads"
        ),
        Some(upright),
        near(short(&as_box) - short(&upright), farther, 1e-3),
    );
    // The walls standing in for KOTOR's point on the walkmesh (set back by
    // the box's half-width) it meets as the box.
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[4.0, 0.0, -2.0], [0.0, 4.0, -2.0], [2.0, 2.0, 4.0]],
        Tag::level(Contents::WALK_CLIP, 0),
    );
    let outline = builder.build();
    let at_outline =
        |shape: &Shape| outline.sweep([0.0; 3], [3.0, 3.0, 0.0], shape, Contents::PLAYER_SOLID);
    let (as_box, upright) = (
        at_outline(&Shape::Box(player)),
        at_outline(&Shape::Upright(player)),
    );
    probes.check(
        "upright outline",
        "running at a walkmesh outline's wall at 45 degrees, the body round across stops where the box does",
        Some(upright),
        near(upright.end[0], as_box.end[0], 1e-5) && upright.fraction < 1.0,
    );

    // A door's brushes: solid in state 0 only.
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-1.0, -0.2, 0.0],
        [1.0, 0.2, 3.0],
        Tag {
            contents: Contents::SOLID,
            surface: crate::Surface { material: 7 },
            entity: Some(42),
            states: 1,
        },
    );
    let mut door = builder.build();
    let closed = door.trace(
        [0.0, -2.0, 1.0],
        [0.0, 2.0, 1.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    door.set_state(42, 1);
    let open = door.trace(
        [0.0, -2.0, 1.0],
        [0.0, 2.0, 1.0],
        &player,
        Contents::PLAYER_SOLID,
    );
    probes.check(
        "toggle",
        "a door's brush stops a player in state 0 (and names entity 42), not in state 1",
        Some(closed),
        closed.hit() && closed.entity == Some(42) && !open.hit(),
    );

    // Standing in a doorway: the box, a unit larger than the player's,
    // reaches into the floor as well as into the door's closed shape. A
    // trace in place through the whole level names the floor (the lower
    // brush); the entity's own overlap test finds the door.
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-3.0, -3.0, -1.0],
        [3.0, 3.0, 0.0],
        Tag::level(Contents::SOLID, 1),
    );
    builder.aabb(
        [-1.0, -0.2, 0.0],
        [1.0, 0.2, 3.0],
        Tag {
            contents: Contents::SOLID,
            surface: crate::Surface { material: 7 },
            entity: Some(42),
            states: 1,
        },
    );
    let doorway = builder.build();
    let grown = Extents {
        mins: player.mins.map(|v| v - UNIT),
        maxs: player.maxs.map(|v| v + UNIT),
    };
    let standing = [0.0, 0.0, 24.125 * UNIT];
    let whole = doorway.trace(standing, standing, &grown, Contents::SOLID);
    let inside = doorway.entity_overlaps(42, 0, standing, &grown, Contents::SOLID);
    let aside = doorway.entity_overlaps(42, 0, [0.0, 1.5, 24.125 * UNIT], &grown, Contents::SOLID);
    let open_shape = doorway.entity_overlaps(42, 1, standing, &grown, Contents::SOLID);
    probes.check(
        "doorway overlap",
        "in a doorway over a floor, a trace in place names the floor, not door 42; door 42's own \
         overlap test finds it closed-shaped there, not 1.5 m aside nor in its open state",
        Some(whole),
        whole.all_solid && whole.entity.is_none() && inside && !aside && !open_shape,
    );

    // Differential: boxes in a Level against the same boxes in BoxWorld,
    // the contract's stand-in, over random traces.
    let mut draw = Draw(12_345);
    let mut boxes = BoxWorld::new();
    let mut builder = LevelBuilder::new();
    for _ in 0..200 {
        let min = [
            draw.range(-20.0, 20.0),
            draw.range(-20.0, 20.0),
            draw.range(-3.0, 3.0),
        ];
        let size = [
            draw.range(0.1, 4.0),
            draw.range(0.1, 4.0),
            draw.range(0.1, 2.0),
        ];
        let max = std::array::from_fn(|i| min[i] + size[i]);
        boxes = boxes.with(Solid::level(min, max));
        builder.aabb(min, max, Tag::level(Contents::SOLID, 0));
    }
    let level = builder.build();
    let (mut same, mut total) = (0, 0);
    let mut first_difference = None;
    for i in 0..20_000 {
        let start = [
            draw.range(-22.0, 22.0),
            draw.range(-22.0, 22.0),
            draw.range(-4.0, 4.0),
        ];
        let end = [
            draw.range(-22.0, 22.0),
            draw.range(-22.0, 22.0),
            draw.range(-4.0, 4.0),
        ];
        let extents = if i % 2 == 0 { player } else { Extents::POINT };
        let a = boxes.trace(start, end, &extents, Contents::SOLID);
        let b = level.trace(start, end, &extents, Contents::SOLID);
        // All-solid box traces keep the first box of the list in BoxWorld,
        // the lowest brush in the Level: the same one.
        // An all-solid trace's plane means nothing (BoxWorld keeps the
        // last hit's, the Level none).
        let equal = a.fraction.to_bits() == b.fraction.to_bits()
            && a.end.map(f32::to_bits) == b.end.map(f32::to_bits)
            && a.start_solid == b.start_solid
            && a.all_solid == b.all_solid
            && (!a.hit() || a.all_solid || a.plane.normal == b.plane.normal);
        total += 1;
        if equal {
            same += 1;
        } else if first_difference.is_none() {
            first_difference = Some(b);
        }
    }
    probes.check(
        "boxes differential",
        &format!(
            "200 random boxes, 20 000 random box and point traces: the Level gives BoxWorld's answer bit for bit ({same} of {total})"
        ),
        first_difference,
        same == total,
    );

    // The brushes as faces for another engine: a floor triangle gives its
    // front only (not its slab's back and sides), a box its six sides.
    let mut builder = LevelBuilder::new();
    let tag = Tag::level(Contents::SOLID, 0);
    builder.triangle([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]], tag);
    builder.aabb([5.0, 5.0, 0.0], [6.0, 7.0, 3.0], tag);
    let faces = builder
        .build()
        .faces_in(([-10.0; 3], [10.0; 3]), Contents::SOLID, None);
    let area = |t: &[[f32; 3]; 3]| {
        let x = crate::brush::cross(crate::brush::sub(t[1], t[0]), crate::brush::sub(t[2], t[0]));
        crate::brush::dot(x, x).sqrt() * 0.5
    };
    let total: f32 = faces.iter().map(|(_, t)| area(t)).sum();
    probes.check(
        "faces",
        &format!(
            "a triangle (2 m²) and a 1×2×3 m box (22 m²) give 24 m² of faces, 1 + 12 triangles ({} faces, {total} m²)",
            faces.len()
        ),
        None,
        faces.len() == 13 && near(total, 24.0, 1e-3),
    );

    // A sill: a floor strip 8 cm across, tilted 1.35 milliradians (a
    // doorway's in Manaan's Sith base), its high edge at `top`. A box
    // overlapping it from the low side, its soles half a millimetre over
    // `top`, is clear of it: the strip's axial bevel bounds it, not its
    // face's plane carried out under the box's far side (that buried the
    // box, and move_body's look under it, a quarter unit down, was all solid).
    let top = 0.08 * 0.00135;
    let mut builder = LevelBuilder::new();
    builder.triangle(
        [[0.0, -2.0, top], [0.08, -2.0, 0.0], [0.0, 2.0, top]],
        Tag::level(Contents::SOLID, 10),
    );
    let sill = builder.build();
    let at = [-0.26, 0.0, top + 0.0005 - feet];
    let t = sill.trace(
        at,
        [at[0], at[1], at[2] - 0.25 * UNIT],
        &player,
        Contents::PLAYER_SOLID,
    );
    probes.check(
        "sill",
        "a box standing half a millimetre over a nearly flat strip it overlaps lands on it (the quarter unit under it is not solid)",
        Some(t),
        !t.start_solid && t.fraction < 1.0 && t.plane.normal[2] > 0.99,
    );
    probes
}
