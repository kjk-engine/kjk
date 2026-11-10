//! The arena's prop hall (`arena::HALL`): one of every placeable the
//! running game has that destruction breaks or damages
//! (`data/destruction/props.csv`), standing in rows on the hall's floor,
//! grouped by class and material, each facing the doorway, each given the
//! room its model's bounding box and its walkmesh (`<model>.pwk`) take. A
//! prop collides as its walkmesh covers, as one box; one without a walkmesh
//! collides as nothing.

use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use sim::arena::Prop;
use vfs::resolver::Resolver;

use crate::arena::HALL;

/// What destruction makes of every placeable kind: a line per
/// `placeables.2da` row of each game.
const CATALOGUE: &str = include_str!("../../../data/destruction/props.csv");

/// The room between props, and between them and the hall's walls (metres).
const GAP: f32 = 2.0;
/// The room between props when each kind stands more than once (metres):
/// packed, for a blast to catch many.
const PACKED_GAP: f32 = 0.4;

/// How many of each kind the hall lays (`KJK_PROP_HALL_COPIES`, 1 by
/// default): more make it destruction's stress test.
fn copies() -> usize {
    std::env::var("KJK_PROP_HALL_COPIES")
        .ok()
        .and_then(|copies| copies.parse().ok())
        .unwrap_or(1)
        .max(1)
}

/// The one class the hall lays (`KJK_PROP_HALL_CLASS`: `break` or
/// `damage`), if it is told one: all that breaks, packed, for the stress
/// test.
fn only_class() -> Option<String> {
    std::env::var("KJK_PROP_HALL_CLASS").ok()
}

/// How far in from the doorway the first row stands (metres).
const FIRST_ROW: f32 = 5.0;
/// The most room a model's bounding box may ask for across (metres): some
/// reach far past what they draw (a hanging vine's).
const MOST_ROOM: f32 = 8.0;
/// The way every prop faces: towards the doorway (degrees from +X).
const FACING: f32 = 270.0;

/// One catalogue line the hall shows.
struct Kind<'a> {
    appearance: u16,
    label: &'a str,
    class: &'a str,
    material: &'a str,
    size: &'a str,
}

/// The catalogue's kinds of `game` (`k1`, `tsl`) that break or show damage,
/// by class, material, size and row.
fn kinds(game: &str) -> Vec<Kind<'_>> {
    let mut kinds: Vec<Kind> = CATALOGUE
        .lines()
        .filter(|line| !line.starts_with('#'))
        .skip(1)
        .filter_map(|line| {
            let [of, appearance, label, class, material, size] =
                line.split(',').collect::<Vec<_>>()[..]
            else {
                return None;
            };
            let shown = matches!(class, "break" | "damage");
            (of == game && shown).then_some(Kind {
                appearance: appearance.parse().ok()?,
                label,
                class,
                material,
                size,
            })
        })
        .collect();
    kinds.sort_by_key(|kind| (kind.class, kind.material, kind.size, kind.appearance));
    kinds
}

/// The room a prop takes when nothing says (metres), by its size.
fn room_of(size: &str) -> f32 {
    match size {
        "small" => 0.8,
        "large" => 3.0,
        _ => 1.6,
    }
}

/// The box round row `appearance`'s model's meshes at rest (least and
/// greatest corners, its own frame, metres), each side at most `MOST_ROOM`,
/// if it draws anything.
fn model_box(
    resolver: &Resolver,
    table: &fmt_odyssey::twoda::TwoDa,
    appearance: u16,
) -> Option<[[f32; 3]; 2]> {
    let model = table.resref(usize::from(appearance), "modelname")?;
    let parsed = crate::viewer::parsed(resolver, &model).ok()??;
    let ir = asset_odyssey::model::ModelIr::from_model(&parsed);
    let [x0, y0, z0, x1, y1, z1] = ir.rest_bounds()?;
    let half = MOST_ROOM / 2.0;
    let clamp = |least: f32, most: f32| (least.max(-half), most.min(half));
    let ((x0, x1), (y0, y1)) = (clamp(x0, x1), clamp(y0, y1));
    (x0 < x1 && y0 < y1).then_some([[x0, y0, z0], [x1, y1, z1]])
}

/// The box holding both `a` and `b`.
fn union([a0, a1]: [[f32; 3]; 2], [b0, b1]: [[f32; 3]; 2]) -> [[f32; 3]; 2] {
    let mut out = [a0, a1];
    for axis in 0..3 {
        out[0][axis] = a0[axis].min(b0[axis]);
        out[1][axis] = a1[axis].max(b1[axis]);
    }
    out
}

/// Row `appearance`'s walkmesh as one box (least and greatest corners, its
/// own frame, metres), if it has one.
fn walkmesh_box(
    resolver: &Resolver,
    table: &fmt_odyssey::twoda::TwoDa,
    appearance: u16,
) -> Option<[[f32; 3]; 2]> {
    let model = table.resref(usize::from(appearance), "modelname")?;
    let bytes = resolver.read(&model.to_ascii_lowercase(), ResType::PWK)?;
    let walkmesh = Walkmesh::parse(&bytes).ok()?;
    walkmesh.vertices.iter().fold(None, |found, vertex| {
        let [mut least, mut most] = found.unwrap_or([*vertex, *vertex]);
        for axis in 0..3 {
            least[axis] = least[axis].min(vertex[axis]);
            most[axis] = most[axis].max(vertex[axis]);
        }
        Some([least, most])
    })
}

/// `corners` (own frame) turned by `FACING`, as the least and greatest x and
/// y they then reach.
fn turned([least, most]: [[f32; 3]; 2]) -> [[f32; 2]; 2] {
    let (sin, cos) = sim::math::sin_cos_deg(FACING);
    let mut reach = [[f32::MAX; 2], [f32::MIN; 2]];
    for (x, y) in [
        (least[0], least[1]),
        (most[0], least[1]),
        (least[0], most[1]),
        (most[0], most[1]),
    ] {
        let at = [x * cos - y * sin, x * sin + y * cos];
        for axis in 0..2 {
            reach[0][axis] = reach[0][axis].min(at[axis]);
            reach[1][axis] = reach[1][axis].max(at[axis]);
        }
    }
    reach
}

/// The hall's props for `game`, from `resolver`'s `placeables.2da` and
/// walkmeshes, laid out in rows from the doorway; an error when they do not
/// fit the hall.
pub fn props(resolver: &Resolver, game: &str) -> Result<Vec<Prop>, String> {
    let table = resolver.two_da("placeables").ok_or("no placeables.2da")?;
    let [[x0, y0], [x1, y1]] = HALL;
    let (mut x, mut y, mut row_depth) = (x0 + GAP, y0 + FIRST_ROW, 0.0f32);
    let mut props = Vec::new();
    let copies = copies();
    let gap = if copies > 1 { PACKED_GAP } else { GAP };
    let only = only_class();
    let kinds: Vec<Kind> = (kinds(game).into_iter())
        .filter(|kind| only.as_deref().is_none_or(|class| kind.class == class))
        .collect();
    for kind in kinds.iter().flat_map(|kind| std::iter::repeat_n(kind, copies)) {
        let walkmesh = walkmesh_box(resolver, &table, kind.appearance);
        let half = room_of(kind.size) / 2.0;
        let model = model_box(resolver, &table, kind.appearance);
        let room = [walkmesh, model]
            .into_iter()
            .flatten()
            .reduce(union)
            .unwrap_or([[-half, -half, 0.0], [half, half, 0.0]]);
        let [least, most] = turned(room);
        let (wide, deep) = (most[0] - least[0], most[1] - least[1]);
        if x + wide > x1 - GAP {
            (x, y, row_depth) = (x0 + GAP, y + row_depth + gap, 0.0);
        }
        if y + deep > y1 - GAP {
            return Err(format!(
                "the prop hall is too small for {game}'s props: {} of them stand",
                props.len()
            ));
        }
        let label = table
            .text(usize::from(kind.appearance), "label")
            .unwrap_or_else(|| kind.label.to_string());
        props.push(Prop {
            appearance: kind.appearance,
            name: format!("{label} ({}, {})", kind.class, kind.material),
            at: [x - least[0], y - least[1], 0.0],
            yaw: FACING,
            footprint: walkmesh.map_or_else(Vec::new, |[least, most]| vec![least, most]),
            destructible: sim::destruction::catalogue(game, kind.appearance),
            model_box: model.map(|[min, max]| sim::module::Bulk { min, max }),
        });
        x += wide + gap;
        row_depth = row_depth.max(deep);
    }
    Ok(props)
}
