//! A level's collision: convex brushes (triangles, boxes, clip brushes) in
//! a bounding-volume tree, traced with the brush clip (`brush::clip`).
//!
//! Determinism: the tree is a pure function of the brushes in the order
//! they were added (splits sort by centroid with the brush index breaking
//! ties), traversal visits children in a fixed order, and of equal hits the
//! lowest brush index wins, so a trace's result does not depend on the tree
//! (with one exception: a brush whose face all but level stands in for its
//! +Z bevel can report a hit before the path enters its grown bounds, and
//! the tree skips it or not by the order it visits brushes; see
//! [`Level::trace_down`], which keeps to the tree's answer there). The
//! build uses only `+ - * /`, `sqrt` and comparisons, so every machine
//! builds the same planes from the same input; [`Level::digest`] names
//! them.
//!
//! Brushes owned by an entity (a door, a placeable) are solid only in some
//! of its states: [`Level::set_state`] switches them. The states are the
//! sim's to keep; the level holds only the current one of each.

use crate::brush::{self, Clip, Swept};
use crate::{
    CONTACT_GAP, Capsule, CollisionWorld, Contents, Extents, Plane, Shape, Surface, Trace,
};

/// A brush's owner states: solid in every state.
pub const ALWAYS: u8 = u8::MAX;

/// An entity's state once it is out of play (destroyed): solid in none.
pub const GONE: u8 = 8;

/// What a brush is made of and who owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub contents: Contents,
    pub surface: Surface,
    /// The entity it belongs to, if not the level.
    pub entity: Option<u32>,
    /// The entity's states (bit n for state n) in which it is solid.
    pub states: u8,
}

impl Tag {
    /// Level geometry of this material.
    pub fn level(contents: Contents, material: u16) -> Tag {
        Tag {
            contents,
            surface: Surface { material },
            entity: None,
            states: ALWAYS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Brush {
    first: u32,
    count: u32,
    min: [f32; 3],
    max: [f32; 3],
    tag: Tag,
    /// Index into `Level::entities` (and a state list), or `u32::MAX` for
    /// the level's own.
    slot: u32,
}

/// A tree node: an inner node's children are `a` and `b`; a leaf holds
/// `count` brushes from `order[a]`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Node {
    min: [f32; 3],
    max: [f32; 3],
    a: u32,
    b: u32,
    count: u32,
}

/// Brushes per leaf at most.
const LEAF: usize = 4;

/// How deep a triangle's slab is behind its face: 1 cm, three times the
/// clip epsilon.
pub const TRIANGLE_DEPTH: f32 = 0.01;

/// Collects brushes, then builds the tree.
#[derive(Debug, Clone, Default)]
pub struct LevelBuilder {
    brushes: Vec<Brush>,
    planes: Vec<Plane>,
    /// Inputs that make no brush (degenerate triangles).
    pub refused: usize,
}

impl LevelBuilder {
    pub fn new() -> LevelBuilder {
        LevelBuilder::default()
    }

    pub fn len(&self) -> usize {
        self.brushes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.brushes.is_empty()
    }

    fn push(&mut self, points: &[[f32; 3]], planes: Option<Vec<Plane>>, tag: Tag) -> bool {
        let Some(planes) = planes else {
            self.refused += 1;
            return false;
        };
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in points {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        self.brushes.push(Brush {
            first: self.planes.len() as u32,
            count: planes.len() as u32,
            min,
            max,
            tag,
            slot: u32::MAX,
        });
        self.planes.extend(planes);
        true
    }

    /// A triangle, solid from both sides: a slab [`TRIANGLE_DEPTH`] deep
    /// behind its face (the side its corners turn anticlockwise from). The
    /// brush clip needs a solid to have an inside: through a flat one, a
    /// trace's way in and way out fall on the same fraction and it passes.
    /// False (and counted in `refused`) when it has no area.
    pub fn triangle(&mut self, corners: [[f32; 3]; 3], tag: Tag) -> bool {
        let [a, b, c] = corners;
        let Some(normal) = brush::normalize(brush::cross(brush::sub(b, a), brush::sub(c, a)))
        else {
            self.refused += 1;
            return false;
        };
        let deep = |p: [f32; 3]| std::array::from_fn(|i| p[i] - normal[i] * TRIANGLE_DEPTH);
        let points = [a, b, c, deep(a), deep(b), deep(c)];
        let side = |p: [f32; 3], q: [f32; 3]| brush::cross(brush::sub(q, p), normal);
        let faces = [
            normal,
            normal.map(|n| -n),
            side(a, b),
            side(b, c),
            side(c, a),
        ];
        let edges = [
            (0, 1),
            (1, 2),
            (2, 0),
            (3, 4),
            (4, 5),
            (5, 3),
            (0, 3),
            (1, 4),
            (2, 5),
        ];
        let planes = brush::planes(&points, &faces, &edges);
        self.push(&points, planes, tag)
    }

    /// An axis-aligned box.
    pub fn aabb(&mut self, min: [f32; 3], max: [f32; 3], tag: Tag) -> bool {
        let points = [min, max];
        let planes = brush::planes(&points, &[], &[]);
        self.push(&points, planes, tag)
    }

    /// The convex hull of `points`, given its face normals and its edges
    /// (pairs of indices into `points`).
    pub fn convex(
        &mut self,
        points: &[[f32; 3]],
        faces: &[[f32; 3]],
        edges: &[(usize, usize)],
        tag: Tag,
    ) -> bool {
        let planes = brush::planes(points, faces, edges);
        self.push(points, planes, tag)
    }

    pub fn build(mut self) -> Level {
        let mut entities: Vec<u32> = self
            .brushes
            .iter()
            .filter_map(|brush| brush.tag.entity)
            .collect();
        entities.sort_unstable();
        entities.dedup();
        for brush in &mut self.brushes {
            if let Some(entity) = brush.tag.entity {
                brush.slot = entities.binary_search(&entity).expect("collected above") as u32;
            }
        }
        let states = vec![0; entities.len()];
        let mut order: Vec<u32> = (0..self.brushes.len() as u32).collect();
        let mut nodes = Vec::new();
        if !order.is_empty() {
            let count = order.len();
            split(&self.brushes, &mut order, 0, count, &mut nodes);
        }
        Level {
            brushes: self.brushes,
            planes: self.planes,
            nodes,
            order,
            entities,
            states,
        }
    }
}

fn centroid(brush: &Brush, axis: usize) -> f32 {
    (brush.min[axis] + brush.max[axis]) * 0.5
}

/// Builds the node over `order[from..from + count]` and returns its index.
fn split(
    brushes: &[Brush],
    order: &mut [u32],
    from: usize,
    count: usize,
    nodes: &mut Vec<Node>,
) -> u32 {
    let slice = &mut order[from..from + count];
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut cmin = [f32::INFINITY; 3];
    let mut cmax = [f32::NEG_INFINITY; 3];
    for &index in slice.iter() {
        let brush = &brushes[index as usize];
        for i in 0..3 {
            min[i] = min[i].min(brush.min[i]);
            max[i] = max[i].max(brush.max[i]);
            cmin[i] = cmin[i].min(centroid(brush, i));
            cmax[i] = cmax[i].max(centroid(brush, i));
        }
    }
    let here = nodes.len();
    nodes.push(Node {
        min,
        max,
        a: from as u32,
        b: 0,
        count: count as u32,
    });
    if count <= LEAF {
        return here as u32;
    }
    // The longest axis of the centroids; the first of equal ones.
    let mut axis = 0;
    for i in 1..3 {
        if cmax[i] - cmin[i] > cmax[axis] - cmin[axis] {
            axis = i;
        }
    }
    slice.sort_by(|&x, &y| {
        centroid(&brushes[x as usize], axis)
            .total_cmp(&centroid(&brushes[y as usize], axis))
            .then(x.cmp(&y))
    });
    let half = count / 2;
    let a = split(brushes, order, from, half, nodes);
    let b = split(brushes, order, from + half, count - half, nodes);
    nodes[here] = Node {
        min,
        max,
        a,
        b,
        count: 0,
    };
    here as u32
}

/// One brush of a built level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushView<'a> {
    pub planes: &'a [Plane],
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub tag: Tag,
}

/// Sizes of a built level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelStats {
    pub brushes: usize,
    pub planes: usize,
    pub nodes: usize,
    pub depth: usize,
    pub entities: usize,
}

/// A built level: see the module documentation.
#[derive(Debug, Clone)]
pub struct Level {
    brushes: Vec<Brush>,
    planes: Vec<Plane>,
    nodes: Vec<Node>,
    order: Vec<u32>,
    /// The entities that own brushes, by number: a brush's `slot` indexes
    /// this and `states`.
    entities: Vec<u32>,
    /// Each entity's current state.
    states: Vec<u8>,
}

/// A level traced with entity states of the caller's in place of its own:
/// the sim keeps door states in its snapshots and traces the level with
/// them, so worlds sharing one level never see each other's doors.
#[derive(Clone, Copy)]
pub struct Stated<'a> {
    level: &'a Level,
    states: &'a [u8],
}

impl Level {
    /// Sets an entity's state (0 at build); false when no brush has it.
    pub fn set_state(&mut self, entity: u32, state: u8) -> bool {
        match self.slot(entity) {
            Some(slot) => {
                self.states[slot] = state;
                true
            }
            None => false,
        }
    }

    pub fn state(&self, entity: u32) -> Option<u8> {
        self.slot(entity).map(|slot| self.states[slot])
    }

    /// The entities that own brushes, by number.
    pub fn entities(&self) -> impl Iterator<Item = u32> + '_ {
        self.entities.iter().copied()
    }

    /// Where an entity's state sits in a state list (`states`,
    /// `with_states`); `None` when no brush has it.
    pub fn slot(&self, entity: u32) -> Option<usize> {
        self.entities.binary_search(&entity).ok()
    }

    /// This level traced with `states` (one per entity, in `entities`
    /// order) in place of its own.
    pub fn with_states<'a>(&'a self, states: &'a [u8]) -> Stated<'a> {
        assert_eq!(
            states.len(),
            self.entities.len(),
            "one state per entity of the level"
        );
        Stated {
            level: self,
            states,
        }
    }

    pub fn brush_count(&self) -> usize {
        self.brushes.len()
    }

    /// Whether a box of `extents` standing at `at` overlaps (or touches) a
    /// brush of the level's own (no entity's) made of contents in `mask`;
    /// with `upright`, the box round across (`Shape::Upright`).
    pub fn level_overlaps(
        &self,
        at: [f32; 3],
        extents: &Extents,
        upright: bool,
        mask: Contents,
    ) -> bool {
        let swept = if upright {
            Swept::upright(extents)
        } else {
            Swept::Box {
                mins: extents.mins,
                maxs: extents.maxs,
            }
        };
        let min = std::array::from_fn(|i| at[i] + extents.mins[i]);
        let max = std::array::from_fn(|i| at[i] + extents.maxs[i]);
        let mut overlaps = false;
        self.visit(min, max, |_, brush| {
            if !overlaps && brush.slot == u32::MAX && brush.tag.contents.intersects(mask) {
                let planes =
                    &self.planes[brush.first as usize..(brush.first + brush.count) as usize];
                let swept = swept.against(brush.tag.contents);
                overlaps = brush::clip(planes, at, at, &swept) == Clip::AllSolid;
            }
        });
        overlaps
    }

    /// Whether a box of `extents` standing at `at` overlaps (or touches) a
    /// brush of `entity` that is solid in `state` and made of contents in
    /// `mask`: that entity's brushes alone, whatever else the box is in.
    /// A zero-length trace through the whole level cannot say it: it names
    /// only the lowest-numbered brush the box is inside (in a doorway, the
    /// floor under it).
    pub fn entity_overlaps(
        &self,
        entity: u32,
        state: u8,
        at: [f32; 3],
        extents: &Extents,
        mask: Contents,
    ) -> bool {
        let Some(slot) = self.slot(entity) else {
            return false;
        };
        if state >= 8 {
            return false;
        }
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let min = std::array::from_fn(|i| at[i] + extents.mins[i]);
        let max = std::array::from_fn(|i| at[i] + extents.maxs[i]);
        let mut overlaps = false;
        self.visit(min, max, |_, brush| {
            if !overlaps
                && brush.slot == slot as u32
                && brush.tag.contents.intersects(mask)
                && state < 8
                && brush.tag.states & (1 << state) != 0
            {
                let planes =
                    &self.planes[brush.first as usize..(brush.first + brush.count) as usize];
                // Standing still, the box is inside a brush exactly when it
                // never leaves it.
                overlaps = brush::clip(planes, at, at, &swept) == Clip::AllSolid;
            }
        });
        overlaps
    }

    /// A brush as built: its planes (faces and bevels), bounds and tag,
    /// for drawing it or handing it to another clipper.
    pub fn brush(&self, index: usize) -> Option<BrushView<'_>> {
        let brush = self.brushes.get(index)?;
        Some(BrushView {
            planes: &self.planes[brush.first as usize..(brush.first + brush.count) as usize],
            min: brush.min,
            max: brush.max,
            tag: brush.tag,
        })
    }

    /// The faces of the brushes overlapping `min`..`max` that are made of
    /// `mask` and solid in `states` (one per entity, as `with_states` takes
    /// them; `None`: the level's own), as triangles, each with its brush's
    /// index: the level for another physics engine.
    pub fn faces_in(
        &self,
        (min, max): ([f32; 3], [f32; 3]),
        mask: Contents,
        states: Option<&[u8]>,
    ) -> Vec<(u32, [[f32; 3]; 3])> {
        let states = states
            .filter(|states| states.len() == self.states.len())
            .unwrap_or(&self.states);
        let mut out = Vec::new();
        self.visit(min, max, |index, brush| {
            if !Level::active(brush, mask, states) {
                return;
            }
            let planes = &self.planes[brush.first as usize..(brush.first + brush.count) as usize];
            for face in brush::faces(planes, brush.min, brush.max) {
                out.extend(
                    face.windows(2)
                        .skip(1)
                        .map(|pair| (index, [face[0], pair[0], pair[1]])),
                );
            }
        });
        out
    }

    /// The bounds of every brush, or `None` for an empty level.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.nodes.first().map(|root| (root.min, root.max))
    }

    pub fn stats(&self) -> LevelStats {
        let mut depth = 0;
        let mut stack = vec![(0usize, 1usize)];
        while let Some((node, level)) = stack.pop() {
            let Some(node) = self.nodes.get(node) else {
                break;
            };
            depth = depth.max(level);
            if node.count == 0 {
                stack.push((node.a as usize, level + 1));
                stack.push((node.b as usize, level + 1));
            }
        }
        LevelStats {
            brushes: self.brushes.len(),
            planes: self.planes.len(),
            nodes: self.nodes.len(),
            depth,
            entities: self.entities.len(),
        }
    }

    /// The level as bytes for the store (`pack`): read back, the same level.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = crate::pack::Out::default();
        out.0.extend(b"KJKC");
        out.u32(Level::FORMAT);
        out.len(self.brushes.len());
        for brush in &self.brushes {
            out.u32(brush.first);
            out.u32(brush.count);
            out.vec3(brush.min);
            out.vec3(brush.max);
            out.u32(brush.tag.contents.0);
            out.u16(brush.tag.surface.material);
            out.u32(brush.tag.entity.unwrap_or(u32::MAX));
            out.u8(brush.tag.states);
            out.u32(brush.slot);
        }
        out.len(self.planes.len());
        for plane in &self.planes {
            out.vec3(plane.normal);
            out.f32(plane.dist);
        }
        out.len(self.nodes.len());
        for node in &self.nodes {
            out.vec3(node.min);
            out.vec3(node.max);
            out.u32(node.a);
            out.u32(node.b);
            out.u32(node.count);
        }
        for list in [&self.order, &self.entities] {
            out.len(list.len());
            for value in list {
                out.u32(*value);
            }
        }
        out.len(self.states.len());
        for state in &self.states {
            out.u8(*state);
        }
        out.0
    }

    /// Bump when `encode`'s bytes change.
    const FORMAT: u32 = 1;

    /// A level `encode` wrote.
    pub fn decode(bytes: &[u8]) -> Result<Level, crate::pack::Bad> {
        let mut input = crate::pack::In(bytes);
        input.magic(b"KJKC", Level::FORMAT)?;
        let mut brushes = Vec::with_capacity(input.len(47)?);
        for _ in 0..brushes.capacity() {
            brushes.push(Brush {
                first: input.u32()?,
                count: input.u32()?,
                min: input.vec3()?,
                max: input.vec3()?,
                tag: Tag {
                    contents: Contents(input.u32()?),
                    surface: Surface {
                        material: input.u16()?,
                    },
                    entity: Some(input.u32()?).filter(|entity| *entity != u32::MAX),
                    states: input.u8()?,
                },
                slot: input.u32()?,
            });
        }
        let mut planes = Vec::with_capacity(input.len(16)?);
        for _ in 0..planes.capacity() {
            planes.push(Plane {
                normal: input.vec3()?,
                dist: input.f32()?,
            });
        }
        let mut nodes = Vec::with_capacity(input.len(36)?);
        for _ in 0..nodes.capacity() {
            nodes.push(Node {
                min: input.vec3()?,
                max: input.vec3()?,
                a: input.u32()?,
                b: input.u32()?,
                count: input.u32()?,
            });
        }
        let mut list = || -> Result<Vec<u32>, crate::pack::Bad> {
            (0..input.len(4)?).map(|_| input.u32()).collect()
        };
        let (order, entities) = (list()?, list()?);
        let states = (0..input.len(1)?)
            .map(|_| input.u8())
            .collect::<Result<_, _>>()?;
        input.finish()?;
        let level = Level {
            brushes,
            planes,
            nodes,
            order,
            entities,
            states,
        };
        level.check()?;
        Ok(level)
    }

    /// Whether every index in it points inside it (a decoded level traces
    /// only what it holds).
    fn check(&self) -> Result<(), crate::pack::Bad> {
        let bad = crate::pack::Bad;
        let planes = self.planes.len() as u64;
        let fits = |first: u32, count: u32, len: u64| u64::from(first) + u64::from(count) <= len;
        if self.brushes.iter().any(|brush| {
            !fits(brush.first, brush.count, planes)
                || (brush.slot != u32::MAX && brush.slot as usize >= self.entities.len())
        }) {
            return Err(bad("a brush past its planes or entities"));
        }
        let (nodes, order) = (self.nodes.len() as u64, self.order.len() as u64);
        if self.nodes.iter().any(|node| match node.count {
            0 => u64::from(node.a) >= nodes || u64::from(node.b) >= nodes,
            count => !fits(node.a, count, order),
        }) || self
            .order
            .iter()
            .any(|&brush| brush as usize >= self.brushes.len())
            || self.states.len() != self.entities.len()
        {
            return Err(bad("a node or index past the level"));
        }
        Ok(())
    }

    /// BLAKE3 over every brush, plane and node as bits, in order: two
    /// machines that build the same digest trace the same level.
    pub fn digest(&self) -> String {
        let mut hasher = crate::Digester::new();
        let mut floats = |values: &[f32]| {
            for value in values {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        };
        for brush in &self.brushes {
            floats(&brush.min);
            floats(&brush.max);
        }
        for plane in &self.planes {
            floats(&plane.normal);
            floats(&[plane.dist]);
        }
        for node in &self.nodes {
            floats(&node.min);
            floats(&node.max);
        }
        for brush in &self.brushes {
            hasher.update(&brush.first.to_le_bytes());
            hasher.update(&brush.count.to_le_bytes());
            hasher.update(&brush.tag.contents.0.to_le_bytes());
            hasher.update(&brush.tag.surface.material.to_le_bytes());
            hasher.update(&brush.tag.entity.unwrap_or(u32::MAX).to_le_bytes());
            hasher.update(&[brush.tag.states]);
        }
        for node in &self.nodes {
            hasher.update(&node.a.to_le_bytes());
            hasher.update(&node.b.to_le_bytes());
            hasher.update(&node.count.to_le_bytes());
        }
        for index in &self.order {
            hasher.update(&index.to_le_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    fn active(brush: &Brush, mask: Contents, states: &[u8]) -> bool {
        if !brush.tag.contents.intersects(mask) {
            return false;
        }
        if brush.slot == u32::MAX {
            return true;
        }
        let state = states[brush.slot as usize];
        state < 8 && brush.tag.states & (1 << state) != 0
    }

    /// Visits every leaf brush whose bounds overlap `min`..`max`, in the
    /// tree's fixed order.
    fn visit(&self, min: [f32; 3], max: [f32; 3], mut each: impl FnMut(u32, &Brush)) {
        if self.nodes.is_empty() {
            return;
        }
        let overlaps = |a_min: &[f32; 3], a_max: &[f32; 3]| {
            (0..3).all(|i| a_min[i] <= max[i] && a_max[i] >= min[i])
        };
        // Median splits keep the tree balanced: its depth is about
        // log2(brushes / LEAF), and the stack holds at most one node per
        // level plus one.
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = &self.nodes[stack[top] as usize];
            if !overlaps(&node.min, &node.max) {
                continue;
            }
            if node.count == 0 {
                stack[top] = node.b;
                stack[top + 1] = node.a;
                top += 2;
                continue;
            }
            for &brush in &self.order[node.a as usize..(node.a + node.count) as usize] {
                let b = &self.brushes[brush as usize];
                if overlaps(&b.min, &b.max) {
                    each(brush, b);
                }
            }
        }
    }

    /// Sweeps `swept` (whose box is `reach` around the origin) through the
    /// tree. Nodes and brushes are skipped when the path meets their
    /// bounds, grown by the shape and twice the clip epsilon, only after
    /// the best hit so far: a hit's fraction is never before the point the
    /// path enters its brush's bounds grown by the epsilon, so what is
    /// skipped could not have won, not even a tie (but for a brush whose
    /// near-level face stands in for its +Z bevel: the module's
    /// documentation).
    fn sweep_with(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        swept: Swept,
        reach: ([f32; 3], [f32; 3]),
        mask: Contents,
        states: &[u8],
    ) -> Trace {
        let path = Path::new(start, end, reach);
        let enter = |min: &[f32; 3], max: &[f32; 3]| path.enter(min, max);
        let mut found = Found::new(end);
        let mut stack = [(0u32, 0.0f32); 64];
        let mut top = 0;
        if let Some(root) = self.nodes.first()
            && let Some(t) = enter(&root.min, &root.max)
        {
            stack[0] = (0, t);
            top = 1;
        }
        while top > 0 {
            top -= 1;
            let (index, t) = stack[top];
            if t > found.limit {
                continue;
            }
            let node = &self.nodes[index as usize];
            if node.count == 0 {
                let a = &self.nodes[node.a as usize];
                let b = &self.nodes[node.b as usize];
                let ta = enter(&a.min, &a.max);
                let tb = enter(&b.min, &b.max);
                // The nearer child is visited first (pushed last); on a
                // tie, `a`.
                let limit = found.limit;
                let mut push = |child: u32, t: Option<f32>| {
                    if let Some(t) = t.filter(|&t| t <= limit) {
                        stack[top] = (child, t);
                        top += 1;
                    }
                };
                if tb.unwrap_or(f32::INFINITY) >= ta.unwrap_or(f32::INFINITY) {
                    push(node.b, tb);
                    push(node.a, ta);
                } else {
                    push(node.a, ta);
                    push(node.b, tb);
                }
                continue;
            }
            for &index in &self.order[node.a as usize..(node.a + node.count) as usize] {
                let brush = &self.brushes[index as usize];
                if !Level::active(brush, mask, states) {
                    continue;
                }
                if enter(&brush.min, &brush.max).is_none_or(|t| t > found.limit) {
                    continue;
                }
                if !crate::reaches(start, end, reach, &brush.min, &brush.max) {
                    continue;
                }
                found.consider(self, index, brush, start, end, &swept);
            }
        }
        found.finish(self, start, end)
    }
}

/// A sweep's path against bounds grown by its shape's `reach` and twice
/// the clip epsilon.
struct Path {
    start: [f32; 3],
    delta: [f32; 3],
    low: [f32; 3],
    high: [f32; 3],
}

impl Path {
    const PAD: f32 = 2.0 * CONTACT_GAP;

    fn new(start: [f32; 3], end: [f32; 3], (low, high): ([f32; 3], [f32; 3])) -> Path {
        Path {
            start,
            delta: [end[0] - start[0], end[1] - start[1], end[2] - start[2]],
            low,
            high,
        }
    }

    /// The grown bounds' far side along axis `i` from `max`.
    fn far(&self, max: &[f32; 3], i: usize) -> f32 {
        max[i] - self.low[i] + Path::PAD
    }

    /// The fraction where the path enters the grown bounds, if it does.
    fn enter(&self, min: &[f32; 3], max: &[f32; 3]) -> Option<f32> {
        let (start, delta) = (self.start, self.delta);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for i in 0..3 {
            let lo = min[i] - self.high[i] - Path::PAD;
            let hi = self.far(max, i);
            if delta[i] == 0.0 {
                if start[i] < lo || start[i] > hi {
                    return None;
                }
                continue;
            }
            let a = (lo - start[i]) / delta[i];
            let b = (hi - start[i]) / delta[i];
            let (a, b) = if a < b { (a, b) } else { (b, a) };
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return None;
            }
        }
        Some(t0)
    }
}

/// A level's brushes by the columns of a grid over X and Y, for a box of
/// one size falling straight down: in each column, every brush the box can
/// meet over the column, by how high the box's origin can be and still
/// meet it, highest first. [`Level::trace_down`] answers from it what the
/// tree does, without walking the tree: a long fall's path overlaps every
/// node it passes, while down a column only the brushes above where the
/// box lands are clipped.
#[derive(Debug, Clone)]
pub struct DropIndex {
    extents: Extents,
    origin: [f32; 2],
    cell: f32,
    size: [u32; 2],
    /// Per brush: the highest the box's origin can be when a clip of the
    /// box against the brush reports it touching (`Level::ceiling`).
    ceilings: Vec<f32>,
    /// Column `c`'s brushes are `brushes[starts[c]..starts[c + 1]]`.
    starts: Vec<u32>,
    brushes: Vec<u32>,
}

impl DropIndex {
    fn column(&self, x: f32, y: f32) -> Option<usize> {
        let fx = (x - self.origin[0]) / self.cell;
        let fy = (y - self.origin[1]) / self.cell;
        if !(fx >= 0.0 && fy >= 0.0) {
            return None;
        }
        let (ix, iy) = (fx as u32, fy as u32);
        (ix < self.size[0] && iy < self.size[1]).then_some((iy * self.size[0] + ix) as usize)
    }

    /// The columns `lo` to `hi` cover along one axis, clamped to the grid.
    fn span(&self, axis: usize, lo: f32, hi: f32) -> Option<(u32, u32)> {
        let f = |v: f32| (v - self.origin[axis]) / self.cell;
        let (a, b) = (f(lo), f(hi));
        let n = self.size[axis];
        if !(b >= 0.0 && a < n as f32) {
            return None;
        }
        Some(((a.max(0.0) as u32).min(n - 1), (b as u32).min(n - 1)))
    }
}

/// What a column's clip found out of the tree's reach: the earliest hit,
/// and whether a start inside, of a brush the tree could skip (one whose
/// planes reach past its bounds grown by the clip epsilon).
#[derive(Default)]
struct Strays {
    hit: Option<f32>,
    inside: bool,
}

impl Level {
    /// The highest a box of `extents` can have its origin, over the X and
    /// Y where its path can enter the brush's grown bounds, when a clip
    /// against the brush reports a hit or a start inside: there the box
    /// stands within the epsilon of every plane's front, so under each
    /// upward plane. Only planes facing mostly up are asked (any one gives
    /// a true bound, and a steep one's is loose and divides rounding by a
    /// small Z); every brush has one, its +Z bevel or a face all but level
    /// kept in place of it (`brush::planes`). Usually the bound is the
    /// brush's top; such a face, carried out to the box's far corner, can
    /// stand higher.
    fn ceiling(&self, brush: &Brush, extents: &Extents) -> f32 {
        let path = Path::new([0.0; 3], [0.0; 3], (extents.mins, extents.maxs));
        let lo = |i: usize| brush.min[i] - extents.maxs[i] - Path::PAD;
        let corners = [
            [lo(0), lo(1)],
            [lo(0), path.far(&brush.max, 1)],
            [path.far(&brush.max, 0), lo(1)],
            [path.far(&brush.max, 0), path.far(&brush.max, 1)],
        ];
        let planes = &self.planes[brush.first as usize..(brush.first + brush.count) as usize];
        let mut ceiling = f32::INFINITY;
        for plane in planes.iter().filter(|plane| plane.normal[2] >= 0.5) {
            let n = plane.normal;
            let corner: [f32; 3] = std::array::from_fn(|i| {
                if n[i] < 0.0 {
                    extents.maxs[i]
                } else {
                    extents.mins[i]
                }
            });
            let moved = plane.dist - brush::dot(corner, n) + CONTACT_GAP;
            let highest = corners
                .iter()
                .map(|[x, y]| (moved - n[0] * x - n[1] * y) / n[2])
                .fold(f32::NEG_INFINITY, f32::max);
            ceiling = ceiling.min(highest);
        }
        // A centimetre over, for rounding: a ceiling too high costs only a
        // clip, one too low a wrong answer.
        ceiling + 0.01
    }

    /// The [`DropIndex`] of a box of `extents` over a grid of `size`
    /// columns of `cell` metres from `origin`.
    pub fn drop_index(
        &self,
        extents: &Extents,
        origin: [f32; 2],
        cell: f32,
        size: [u32; 2],
    ) -> DropIndex {
        let mut index = DropIndex {
            extents: *extents,
            origin,
            cell,
            size,
            ceilings: self
                .brushes
                .iter()
                .map(|b| self.ceiling(b, extents))
                .collect(),
            starts: Vec::new(),
            brushes: Vec::new(),
        };
        let path = Path::new([0.0; 3], [0.0; 3], (extents.mins, extents.maxs));
        // Each brush goes in every column its grown bounds cover: where
        // `Path::enter` can let the box in.
        let spans: Vec<Option<[u32; 4]>> = self
            .brushes
            .iter()
            .map(|brush| {
                let range = |i: usize| {
                    let lo = brush.min[i] - extents.maxs[i] - Path::PAD;
                    index.span(i, lo, path.far(&brush.max, i))
                };
                let ((x0, x1), (y0, y1)) = (range(0)?, range(1)?);
                Some([x0, x1, y0, y1])
            })
            .collect();
        let columns = |[x0, x1, y0, y1]: [u32; 4]| {
            (y0..=y1).flat_map(move |iy| (x0..=x1).map(move |ix| (iy * size[0] + ix) as usize))
        };
        // Counted, then placed: each column's brushes in one run.
        let mut starts = vec![0u32; (size[0] * size[1]) as usize + 1];
        for &span in spans.iter().flatten() {
            for column in columns(span) {
                starts[column + 1] += 1;
            }
        }
        for column in 1..starts.len() {
            starts[column] += starts[column - 1];
        }
        let mut next = starts.clone();
        let mut brushes = vec![0u32; *starts.last().expect("one more than the columns") as usize];
        for (number, span) in spans.iter().enumerate() {
            for column in span.iter().flat_map(|&span| columns(span)) {
                brushes[next[column] as usize] = number as u32;
                next[column] += 1;
            }
        }
        let ceilings = &index.ceilings;
        for column in 0..starts.len() - 1 {
            brushes[starts[column] as usize..starts[column + 1] as usize].sort_unstable_by(
                |&a, &b| {
                    ceilings[b as usize]
                        .total_cmp(&ceilings[a as usize])
                        .then(a.cmp(&b))
                },
            );
        }
        index.starts = starts;
        index.brushes = brushes;
        index
    }

    /// What [`CollisionWorld::trace`] of the index's box from `start` to
    /// `end` answers, for a fall straight down within the index's grid;
    /// anything else is traced through the tree.
    ///
    /// The tree clips the brushes whose grown bounds the path enters
    /// before its best hit so far. Down a column, every such brush is
    /// clipped until the next one's ceiling is reached only after the best
    /// hit; past that, none can touch the box sooner. That is the tree's
    /// answer when every brush that decides it touches the box no sooner
    /// than the path enters its grown bounds, as a brush with its +Z bevel
    /// does: the tree then cannot have skipped it. A brush whose tilted
    /// plane reaches higher (a "stray") may have been skipped or clipped
    /// depending on the tree's order; when one could decide the answer,
    /// the fall is traced through the tree.
    pub fn trace_down(
        &self,
        index: &DropIndex,
        start: [f32; 3],
        end: [f32; 3],
        mask: Contents,
    ) -> Trace {
        let straight = start[0] == end[0] && start[1] == end[1] && end[2] < start[2];
        let column = index.column(start[0], start[1]).filter(|_| straight);
        let Some(column) = column else {
            return self.trace(start, end, &index.extents, mask);
        };
        let extents = &index.extents;
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let reach = (extents.mins, extents.maxs);
        let path = Path::new(start, end, reach);
        let mut found = Found::new(end);
        let mut strays = Strays::default();
        let range = index.starts[column] as usize..index.starts[column + 1] as usize;
        for &number in &index.brushes[range] {
            let reached = (index.ceilings[number as usize] - start[2]) / path.delta[2];
            if reached > found.limit {
                break;
            }
            let brush = &self.brushes[number as usize];
            if !Level::active(brush, mask, &self.states)
                || !crate::reaches(start, end, reach, &brush.min, &brush.max)
            {
                continue;
            }
            let Some(enter) = path.enter(&brush.min, &brush.max) else {
                continue;
            };
            let planes = &self.planes[brush.first as usize..(brush.first + brush.count) as usize];
            let clip = brush::clip(planes, start, end, &swept);
            match clip {
                Clip::Hit { fraction, .. } if fraction < enter => {
                    strays.hit = Some(strays.hit.map_or(fraction, |hit| hit.min(fraction)));
                }
                Clip::StartSolid | Clip::AllSolid if enter > 0.0 => strays.inside = true,
                _ => {}
            }
            found.record(number, clip);
        }
        let decided = found.solid != u32::MAX;
        if strays.inside || (!decided && strays.hit.is_some_and(|hit| hit <= found.trace.fraction))
        {
            return self.trace(start, end, extents, mask);
        }
        found.finish(self, start, end)
    }
}

/// The best of a sweep's hits so far: the lowest fraction, of equal ones
/// the lowest brush; an all-solid brush (the lowest) wins over any hit.
struct Found {
    trace: Trace,
    best: u32,
    solid: u32,
    /// How far a hit may still be: the best so far, 0 once all solid.
    limit: f32,
}

impl Found {
    fn new(end: [f32; 3]) -> Found {
        Found {
            trace: Trace::clear(end),
            best: u32::MAX,
            solid: u32::MAX,
            limit: 1.0,
        }
    }

    fn consider(
        &mut self,
        level: &Level,
        index: u32,
        brush: &Brush,
        start: [f32; 3],
        end: [f32; 3],
        swept: &Swept,
    ) {
        let planes = &level.planes[brush.first as usize..(brush.first + brush.count) as usize];
        let swept = swept.against(brush.tag.contents);
        self.record(index, brush::clip(planes, start, end, &swept));
    }

    fn record(&mut self, index: u32, clip: Clip) {
        match clip {
            Clip::Miss => {}
            Clip::StartSolid => self.trace.start_solid = true,
            Clip::AllSolid => {
                self.trace.start_solid = true;
                self.solid = self.solid.min(index);
                self.limit = 0.0;
            }
            Clip::Hit { fraction, plane } => {
                let trace = &mut self.trace;
                if fraction < trace.fraction || (fraction == trace.fraction && index < self.best) {
                    trace.fraction = fraction;
                    trace.plane = plane;
                    self.best = index;
                    self.limit = self.limit.min(fraction);
                }
            }
        }
    }

    fn finish(self, level: &Level, start: [f32; 3], end: [f32; 3]) -> Trace {
        let mut trace = self.trace;
        let owner = if self.solid != u32::MAX {
            trace.all_solid = true;
            trace.fraction = 0.0;
            trace.plane = Plane::default();
            self.solid
        } else {
            self.best
        };
        if let Some(brush) = level.brushes.get(owner as usize) {
            trace.surface = brush.tag.surface;
            trace.contents = brush.tag.contents;
            trace.entity = brush.tag.entity;
        }
        if trace.all_solid {
            trace.end = start;
        } else if trace.fraction < 1.0 {
            trace.end = std::array::from_fn(|i| start[i] + trace.fraction * (end[i] - start[i]));
        }
        trace
    }
}

impl Level {
    /// The same box trace without the tree: every brush, in order. Slow;
    /// it is how `kjk collision probes` checks the tree gives the same
    /// answers.
    pub fn trace_every_brush(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        extents: &Extents,
        mask: Contents,
    ) -> Trace {
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let mut found = Found::new(end);
        let reach = (extents.mins, extents.maxs);
        for (index, brush) in self.brushes.iter().enumerate() {
            if Level::active(brush, mask, &self.states)
                && crate::reaches(start, end, reach, &brush.min, &brush.max)
            {
                found.consider(self, index as u32, brush, start, end, &swept);
            }
        }
        found.finish(self, start, end)
    }
}

impl CollisionWorld for Level {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        self.with_states(&self.states)
            .trace(start, end, extents, mask)
    }

    fn sweep(&self, start: [f32; 3], end: [f32; 3], shape: &Shape, mask: Contents) -> Trace {
        self.with_states(&self.states)
            .sweep(start, end, shape, mask)
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        self.with_states(&self.states).point_contents(point, mask)
    }

    fn level(&self) -> Option<&Level> {
        Some(self)
    }
}

impl CollisionWorld for Stated<'_> {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let reach = (extents.mins, extents.maxs);
        self.level
            .sweep_with(start, end, swept, reach, mask, self.states)
    }

    fn sweep(&self, start: [f32; 3], end: [f32; 3], shape: &Shape, mask: Contents) -> Trace {
        match shape {
            Shape::Box(extents) => self.trace(start, end, extents, mask),
            Shape::Upright(extents) => {
                let reach = (extents.mins, extents.maxs);
                self.level.sweep_with(
                    start,
                    end,
                    Swept::upright(extents),
                    reach,
                    mask,
                    self.states,
                )
            }
            Shape::Capsule(capsule) => self.level.capsule(start, end, capsule, mask, self.states),
        }
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        let level = self.level;
        let mut all = Contents::NONE;
        level.visit(point, point, |_, brush| {
            let planes = &level.planes[brush.first as usize..(brush.first + brush.count) as usize];
            if Level::active(brush, mask, self.states) && brush::contains(planes, point) {
                all = all | brush.tag.contents;
            }
        });
        all
    }

    fn level(&self) -> Option<&Level> {
        Some(self.level)
    }
}

impl Level {
    /// A capsule sweep: the capsule's centre is traced, and the reported
    /// `end` is the origin's again.
    fn capsule(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        capsule: &Capsule,
        mask: Contents,
        states: &[u8],
    ) -> Trace {
        let c = capsule.center;
        let from = [start[0] + c[0], start[1] + c[1], start[2] + c[2]];
        let to = [end[0] + c[0], end[1] + c[1], end[2] + c[2]];
        let swept = Swept::Capsule {
            radius: capsule.radius,
            offset: capsule.half_height - capsule.radius,
        };
        let r = capsule.radius;
        let h = capsule.half_height;
        let reach = ([-r, -r, -h], [r, r, h]);
        let mut trace = self.sweep_with(from, to, swept, reach, mask, states);
        trace.end = if trace.all_solid {
            start
        } else if trace.fraction < 1.0 {
            std::array::from_fn(|i| start[i] + trace.fraction * (end[i] - start[i]))
        } else {
            end
        };
        trace
    }
}
