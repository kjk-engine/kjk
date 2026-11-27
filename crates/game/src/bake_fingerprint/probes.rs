//! Checks of a bake's fingerprint, on fingerprints, rooms and models made
//! here (no install): its text reads back, the verdict follows the parts
//! that differ, the exact digest of a lightmap follows only the triangles
//! that draw it, and placeables are matched one by one.

use std::collections::{BTreeMap, BTreeSet};

use asset_odyssey::model::{MeshIr, ModelIr, NodeIr, matrix};
use asset_odyssey::module::RoomIr;

use super::{
    Current, EXACT, FORMAT, Facing, Fingerprint, Placed, RoomModels, Verdict, bits, fit,
    lightmap_approx_digests, lightmap_digests, model_approx_digest, parse, stamp, text, verdict,
};
use crate::probe_kit::{Checks, check};

/// The checks, for `lighting_fingerprints` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("the_text_reads_back", the_text_reads_back()),
        ("a_verdict_follows_the_parts", a_verdict_follows_the_parts()),
        (
            "tolerant_matching_resaved_mdl_and_placeable",
            tolerant_matching_resaved_mdl_and_placeable(),
        ),
    ]
}

/// The checks of the exact lines, for `lighting_fingerprints_by_lightmap`
/// in the approved tests.
pub fn exact_probes() -> Checks {
    vec![
        (
            "the_exact_lines_read_back_in_both_readers",
            the_exact_lines_read_back_in_both_readers(),
        ),
        (
            "a_lightmap_digest_follows_only_its_triangles",
            a_lightmap_digest_follows_only_its_triangles(),
        ),
        (
            "each_lightmap_falls_back_by_itself",
            each_lightmap_falls_back_by_itself(),
        ),
        ("placeables_match_one_by_one", placeables_match_one_by_one()),
    ]
}

/// A fingerprint with no exact lines.
fn plain(area: &str, rooms: &str, placeables: &str) -> Fingerprint {
    Fingerprint {
        area: area.into(),
        format: FORMAT,
        rooms: rooms.into(),
        placeables: placeables.into(),
        exact: None,
    }
}

/// A fingerprint written as the text of its file reads back the same.
fn the_text_reads_back() -> Result<String, String> {
    let print = plain(
        "101per",
        "0123456789abcdef0123456789abcdef",
        "fedcba9876543210fedcba9876543210",
    );
    let read = parse(&text(&print));
    check(
        read.as_ref() == Some(&print),
        format!("{print:?} read back as {read:?}"),
    )
}

/// The same fingerprint gives the whole bake. Other placeables give its
/// lightmaps alone. Other rooms, or another format, refuse it.
fn a_verdict_follows_the_parts() -> Result<String, String> {
    let stamp = plain("a", "r", "p");
    let same = verdict(&stamp, &stamp);
    check(same == Verdict::Whole, format!("the same gives {same:?}"))?;
    let moved = Fingerprint {
        placeables: "q".into(),
        ..stamp.clone()
    };
    let moved = verdict(&stamp, &moved);
    check(
        moved == Verdict::LightmapsOnly,
        format!("other placeables give {moved:?}"),
    )?;
    let rebuilt = Fingerprint {
        rooms: "s".into(),
        ..stamp.clone()
    };
    let rebuilt = verdict(&stamp, &rebuilt);
    check(
        matches!(rebuilt, Verdict::Refused(_)),
        format!("other rooms give {rebuilt:?}"),
    )?;
    let older = Fingerprint {
        format: FORMAT + 1,
        ..stamp.clone()
    };
    let older = verdict(&stamp, &older);
    check(
        matches!(older, Verdict::Refused(_)),
        format!("another format gives {older:?}"),
    )
}

/// The reader of `fingerprint.txt` in kjk v0.21.0, as it was released: the
/// area, the format, `rooms` and `placeables`, each needed, and every other
/// line skipped. It stays here unchanged, so that the check proves what an
/// older kjk reads in a file this kjk writes.
pub(crate) fn read_as_v0_21(text: &str) -> Option<(String, u32, String, String)> {
    let (mut area, mut format, mut rooms, mut placeables) = (None, None, None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "area" => area = Some(value.to_ascii_lowercase()),
            "format" => format = value.parse().ok(),
            "rooms" => rooms = Some(value),
            "placeables" => placeables = Some(value),
            _ => {}
        }
    }
    Some((area?, format?, rooms?, placeables?))
}

/// A node at `at`, with no turn.
fn node(at: [f32; 3]) -> NodeIr {
    NodeIr {
        name: format!("node at {at:?}"),
        parent: None,
        translation: at,
        rotation: [0.0, 0.0, 0.0, 1.0],
        world: matrix(at, [0.0, 0.0, 0.0, 1.0]),
        animated: false,
    }
}

/// A mesh of node `node` with the vertices `corners` (each a place and a
/// lightmap coordinate), the triangles `indices`, the lightmap `lightmap`
/// and the diffuse texture `texture`.
fn mesh(
    node: usize,
    corners: &[([f32; 3], [f32; 2])],
    indices: &[u32],
    lightmap: Option<&str>,
    texture: &str,
) -> MeshIr {
    MeshIr {
        node,
        positions: corners.iter().map(|(at, _)| *at).collect(),
        normals: vec![[0.0, 0.0, 1.0]; corners.len()],
        uv1: vec![[0.0, 0.0]; corners.len()],
        uv2: corners.iter().map(|(_, uv)| *uv).collect(),
        indices: indices.to_vec(),
        texture: Some(texture.into()),
        lightmap: lightmap.map(str::to_string),
        transparency_hint: 0,
        skinned: false,
        shadow: true,
        self_illum: [0.0; 3],
        alpha: 1.0,
        ambient: [1.0; 3],
        diffuse: [1.0; 3],
        uv_scroll: None,
        skin: None,
        dangly: None,
        referenced: false,
    }
}

/// A model of the nodes `nodes` and the meshes `meshes`.
fn model(name: &str, nodes: Vec<NodeIr>, meshes: Vec<MeshIr>) -> ModelIr {
    ModelIr {
        name: name.into(),
        supermodel: None,
        nodes,
        meshes,
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: true,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// A room of model `name` at `at`.
fn room(name: &str, at: [f32; 3]) -> RoomIr {
    RoomIr {
        model: name.into(),
        position: at,
        visible: Vec::new(),
        has_walkmesh: true,
        disable_weather: false,
        force_rating: 0,
    }
}

/// Four corners, each with its place and its lightmap coordinate.
type Corners = [([f32; 3], [f32; 2]); 4];

/// Models by name, as a test builds them.
type Models<'a> = Vec<(&'a str, ModelIr)>;

/// The four corners of a square of 1 m with its lightmap coordinates.
const SQUARE: Corners = [
    ([0.0, 0.0, 0.0], [0.0, 0.0]),
    ([1.0, 0.0, 0.0], [0.5, 0.0]),
    ([1.0, 1.0, 0.0], [0.5, 0.5]),
    ([0.0, 1.0, 0.0], [0.0, 0.5]),
];

/// Where the room's node puts the square.
const NODE_AT: [f32; 3] = [1.0, 2.0, 3.0];

/// The square as one mesh of lightmap `lm0` on a node at `NODE_AT`.
fn square_mesh(corners: Corners) -> MeshIr {
    mesh(0, &corners, &[0, 1, 2, 0, 2, 3], Some("lm0"), "wall01")
}

/// The room model `room01` of the meshes `meshes`. Its node 0 is at
/// `NODE_AT` and its node 1 at the model's origin.
fn room_model(meshes: Vec<MeshIr>) -> ModelIr {
    model("room01", vec![node(NODE_AT), node([0.0; 3])], meshes)
}

/// The models `models` by name, as the exact digests read them.
fn models_of(models: Models) -> RoomModels {
    (models.into_iter())
        .map(|(name, ir)| (name.to_string(), Some(ir)))
        .collect()
}

/// The digest of lightmap `lm0` in the rooms `rooms` of the models `models`.
fn lm0(rooms: &[RoomIr], models: Models) -> Option<String> {
    lightmap_digests(rooms, &models_of(models)).0.remove("lm0")
}

/// The square with corner `at` changed by `change`.
fn square_with(at: usize, change: impl Fn(&mut ([f32; 3], [f32; 2]))) -> Corners {
    let mut corners = SQUARE;
    change(&mut corners[at]);
    corners
}

/// The exact digest of a lightmap stays the same when its room mesh is
/// split in two with its vertices reordered and its texture renamed, when
/// the mesh is moved to another node with the same places in the area,
/// when a mesh without a lightmap, a skinned mesh or a referenced mesh is
/// added, when the room model is renamed, and when a lightmap coordinate of
/// -0 is written as 0. It changes when a corner moves by 1 cm, when the
/// room moves by 1 cm, and when a lightmap coordinate moves by 1/1000. The
/// stored lightmap's texture is not an input of the digest at all, so an
/// upscaled stored lightmap leaves it as it is.
fn a_lightmap_digest_follows_only_its_triangles() -> Result<String, String> {
    let at = [room("room01", [10.0, 20.0, 0.0])];
    let base = lm0(&at, vec![("room01", room_model(vec![square_mesh(SQUARE)]))])
        .ok_or("the square draws no lightmap lm0")?;
    let [v0, v1, v2, v3] = SQUARE;
    let shifted = |corner: ([f32; 3], [f32; 2])| {
        let p = corner.0;
        (
            [p[0] + NODE_AT[0], p[1] + NODE_AT[1], p[2] + NODE_AT[2]],
            corner.1,
        )
    };
    let unlit = mesh(0, &SQUARE, &[0, 1, 2], None, "floor01");
    let mut skinned = mesh(0, &[v0, v1, v3], &[0, 1, 2], Some("lm0"), "wall01");
    skinned.skin = Some(asset_odyssey::model::SkinIr {
        joints: Vec::new(),
        inverse_binds: Vec::new(),
        rest_inverse_binds: Vec::new(),
        vertex_joints: Vec::new(),
        vertex_weights: Vec::new(),
    });
    let mut referenced = mesh(0, &[v0, v1, v3], &[0, 1, 2], Some("lm0"), "wall01");
    referenced.referenced = true;
    let same: Vec<(&str, Models, &str)> = vec![
        (
            "split in two, reordered and renamed",
            vec![(
                "room01",
                room_model(vec![
                    mesh(0, &[v2, v0, v1], &[1, 2, 0], Some("LM0"), "other"),
                    mesh(0, &[v3, v0, v2], &[0, 1, 2], Some("lm0"), "else"),
                ]),
            )],
            "room01",
        ),
        (
            "one half on another node at the same places",
            vec![(
                "room01",
                room_model(vec![
                    mesh(0, &[v0, v1, v2], &[0, 1, 2], Some("lm0"), "wall01"),
                    mesh(
                        1,
                        &[v0, v2, v3].map(shifted),
                        &[0, 1, 2],
                        Some("lm0"),
                        "wall01",
                    ),
                ]),
            )],
            "room01",
        ),
        (
            "an unlit, a skinned and a referenced mesh added",
            vec![(
                "room01",
                room_model(vec![square_mesh(SQUARE), unlit, skinned, referenced]),
            )],
            "room01",
        ),
        (
            "the room model renamed",
            vec![("room99", room_model(vec![square_mesh(SQUARE)]))],
            "room99",
        ),
        (
            "a lightmap coordinate of -0",
            vec![(
                "room01",
                room_model(vec![square_mesh(square_with(0, |corner| {
                    corner.1[0] = -0.0
                }))]),
            )],
            "room01",
        ),
    ];
    let mut wrong = Vec::new();
    for (what, models, name) in same {
        let rooms = [room(name, at[0].position)];
        if lm0(&rooms, models).as_ref() != Some(&base) {
            wrong.push(format!("{what} changed the digest"));
        }
    }
    let moved_room = [room("room01", [10.01, 20.0, 0.0])];
    let differ: Vec<(&str, &[RoomIr], Corners)> = vec![
        (
            "a corner moved by 1 cm",
            &at,
            square_with(1, |corner| corner.0[0] += 0.01),
        ),
        (
            "a lightmap coordinate moved by 1/1000",
            &at,
            square_with(2, |corner| corner.1[1] += 0.001),
        ),
        ("the room moved by 1 cm", &moved_room, SQUARE),
    ];
    for (what, rooms, corners) in differ {
        let found = lm0(
            rooms,
            vec![("room01", room_model(vec![square_mesh(corners)]))],
        );
        if found.as_ref() == Some(&base) {
            wrong.push(format!("{what} kept the digest"));
        }
    }
    check(wrong.is_empty(), wrong.join("; "))
}

/// The player's files of rooms `rooms` and models `models`, with the lit
/// placeables `placed` of the models `placeable_models`.
fn current_of(
    rooms: &[RoomIr],
    models: Models,
    placed: Vec<Placed>,
    placeable_models: &[(&str, &str)],
) -> Current {
    let models = models_of(models);
    let (lightmaps, rooms_of) = lightmap_digests(rooms, &models);
    let lightmaps_approx = lightmap_approx_digests(rooms, &models);
    Current {
        print: plain("101per", "rooms", "placeables"),
        lightmaps,
        lightmaps_approx,
        rooms_of,
        named: super::named_lightmaps(&models),
        placed,
        models: (placeable_models.iter())
            .map(|(name, digest)| ((*name).to_string(), (*digest).to_string()))
            .collect(),
        models_approx: BTreeMap::new(),
    }
}

/// A room with lightmaps `lm0` and `lm1`, and a sky room with lightmap
/// `sky`, from the models given.
fn two_rooms(room_meshes: Vec<MeshIr>, sky: ModelIr) -> Current {
    current_of(
        &[room("room01", [10.0, 20.0, 0.0]), room("sky01", [0.0; 3])],
        vec![("room01", room_model(room_meshes)), ("sky01", sky)],
        Vec::new(),
        &[],
    )
}

/// The second lightmap's mesh: the square raised by `up`.
fn lm1_mesh(up: f32) -> MeshIr {
    let corners = SQUARE.map(|(p, uv)| ([p[0], p[1], p[2] + up], uv));
    mesh(0, &corners, &[0, 1, 2, 0, 2, 3], Some("lm1"), "wall02")
}

/// A sky model whose square is `size` metres wide.
fn sky_model(size: f32) -> ModelIr {
    let corners = SQUARE.map(|(p, uv)| (p.map(|v| v * size), uv));
    model(
        "sky01",
        vec![node([0.0; 3])],
        vec![mesh(0, &corners, &[0, 1, 2, 0, 2, 3], Some("sky"), "stars")],
    )
}

/// A bake of PNGs `lm0` and `lm1` over a room and a sky room. Over the same
/// files every lightmap matches, and the sky's lightmap is a backdrop.
/// Another sky model leaves every baked lightmap matching. A second room
/// mesh moved by 1 m leaves `lm0` matching and `lm1` differing, with the
/// room model that draws it. With both moved, no lightmap matches and the
/// area is not baked. A stamp names the PNGs that no room mesh draws.
fn each_lightmap_falls_back_by_itself() -> Result<String, String> {
    let baked = two_rooms(vec![square_mesh(SQUARE), lm1_mesh(2.0)], sky_model(100.0));
    let pngs: BTreeSet<String> = ["lm0", "lm1", "grass0"].map(String::from).into();
    let (stamped, undrawn) = stamp(&baked, &pngs);
    check(
        undrawn == ["grass0"],
        format!("the stamp names {undrawn:?} as drawn by no room mesh"),
    )?;
    let backdrops = stamped.exact.as_ref().map(|exact| exact.backdrops.clone());
    check(
        backdrops == Some(["sky".to_string()].into()),
        format!("the backdrops are {backdrops:?}"),
    )?;
    let judged = |current: &Current| -> Result<super::Fit, String> {
        fit(&stamped, current).ok_or("the stamp has no exact lines")?
    };
    let all: BTreeSet<String> = pngs.clone();
    let cases: Vec<(&str, Current, BTreeSet<String>, Vec<&str>)> = vec![
        ("the same files", baked.clone(), all.clone(), vec![]),
        (
            "another sky model",
            two_rooms(vec![square_mesh(SQUARE), lm1_mesh(2.0)], sky_model(50.0)),
            all.clone(),
            vec![],
        ),
        (
            "lm1's mesh moved by 1 m",
            two_rooms(vec![square_mesh(SQUARE), lm1_mesh(3.0)], sky_model(100.0)),
            ["lm0", "grass0"].map(String::from).into(),
            vec!["lm1"],
        ),
        (
            "both meshes moved by 1 m",
            two_rooms(
                vec![
                    square_mesh(SQUARE.map(|(p, uv)| ([p[0], p[1], p[2] + 1.0], uv))),
                    lm1_mesh(3.0),
                ],
                sky_model(100.0),
            ),
            ["grass0"].map(String::from).into(),
            vec!["lm0", "lm1"],
        ),
    ];
    let mut wrong = Vec::new();
    for (what, current, matching, differing) in cases {
        let found = judged(&current)?;
        let differ: Vec<&str> = found.differing.keys().map(String::as_str).collect();
        if found.matching != matching || differ != differing {
            wrong.push(format!(
                "{what}: matching {:?}, differing {differ:?}",
                found.matching
            ));
        }
        if let Some(rooms) = found.differing.get("lm1")
            && *rooms != BTreeSet::from(["room01".to_string()])
        {
            wrong.push(format!("{what}: lm1 is drawn by {rooms:?}"));
        }
    }
    // An area where only the grass lightmap matches still counts as
    // baked, since nothing draws that lightmap on a room. Without it, an
    // area whose lightmaps all differ is not baked.
    let none = two_rooms(vec![lm1_mesh(5.0)], sky_model(100.0));
    let (only_rooms, _) = stamp(&baked, &["lm0", "lm1"].map(String::from).into());
    let judged_none = fit(&only_rooms, &none).ok_or("the stamp has no exact lines")??;
    if judged_none.baked() {
        wrong.push(format!(
            "an area with no matching lightmap is baked: {judged_none:?}"
        ));
    }
    let older = Fingerprint {
        format: FORMAT + 1,
        ..stamped.clone()
    };
    if !matches!(fit(&older, &baked), Some(Err(_))) {
        wrong.push("another format is not refused".into());
    }
    if fit(&plain("101per", "rooms", "placeables"), &baked).is_some() {
        wrong.push("a stamp with no exact lines is judged by them".into());
    }
    check(wrong.is_empty(), wrong.join("; "))
}

/// A lit placeable `instance` of model `model` at `x` along X, at the
/// bearing `bearing`.
fn crate_at(instance: usize, model: &str, x: f32, bearing: f32) -> Placed {
    Placed {
        instance,
        model: model.into(),
        position: [x, 5.0, 0.0].map(bits),
        facing: Facing::Bearing(bits(bearing)),
    }
}

/// The player's lit placeables `placed`, with the crate's model of digest
/// `crate_digest`.
fn with_placed(placed: Vec<Placed>, crate_digest: &str) -> Current {
    current_of(
        &[],
        Vec::new(),
        placed,
        &[("crate01", crate_digest), ("table01", "t")],
    )
}

/// Player instance numbers, each with the baked instance number it matched.
type Pairs = Vec<(usize, usize)>;

/// A placeable a content mod adds shifts the instance numbers of those
/// after it, and they keep their baked light under their old numbers. A
/// placeable turned in place, or one whose model's lit meshes differ, loses
/// its baked light. The same placeables match all and differ in nothing.
fn placeables_match_one_by_one() -> Result<String, String> {
    let baked = with_placed(
        vec![
            crate_at(0, "crate01", 1.0, 0.0),
            crate_at(1, "table01", 2.0, 0.5),
            crate_at(2, "crate01", 3.0, -0.0),
        ],
        "c",
    );
    let (stamped, _) = stamp(&baked, &BTreeSet::new());
    let cases: Vec<(&str, Current, Pairs, bool)> = vec![
        (
            "the same placeables",
            baked.clone(),
            vec![(0, 0), (1, 1), (2, 2)],
            false,
        ),
        (
            "a crate added first",
            with_placed(
                vec![
                    crate_at(0, "crate01", 9.0, 0.0),
                    crate_at(1, "crate01", 1.0, 0.0),
                    crate_at(2, "table01", 2.0, 0.5),
                    crate_at(3, "crate01", 3.0, 0.0),
                ],
                "c",
            ),
            vec![(1, 0), (2, 1), (3, 2)],
            true,
        ),
        (
            "the table turned in place",
            with_placed(
                vec![
                    crate_at(0, "crate01", 1.0, 0.0),
                    crate_at(1, "table01", 2.0, 0.6),
                    crate_at(2, "crate01", 3.0, 0.0),
                ],
                "c",
            ),
            vec![(0, 0), (2, 2)],
            true,
        ),
        (
            "the crate's model changed",
            with_placed(
                vec![
                    crate_at(0, "crate01", 1.0, 0.0),
                    crate_at(1, "table01", 2.0, 0.5),
                    crate_at(2, "crate01", 3.0, 0.0),
                ],
                "d",
            ),
            vec![(1, 1)],
            true,
        ),
    ];
    let mut wrong = Vec::new();
    for (what, current, want, differ) in cases {
        let found = fit(&stamped, &current).ok_or("the stamp has no exact lines")??;
        let pairs: Vec<(usize, usize)> = found.placeables.into_iter().collect();
        if pairs != want || found.placeables_differ != differ {
            wrong.push(format!(
                "{what}: matched {pairs:?}, differ {}",
                found.placeables_differ
            ));
        }
    }
    check(wrong.is_empty(), wrong.join("; "))
}

/// A fingerprint with exact lines, among them a placeable at -0 and one
/// facing along a vector, reads back the same from its text. kjk v0.21.0's
/// reader reads the same area, format and digests from it. Exact lines of
/// another version, or exact lines that do not read, are left out.
fn the_exact_lines_read_back_in_both_readers() -> Result<String, String> {
    let mut placed = vec![
        crate_at(3, "crate01", -0.0, 1.25),
        crate_at(7, "table01", 2.5e-7, 0.0),
    ];
    placed[1].facing = Facing::Vector([bits(0.6), bits(-0.8)]);
    let exact = super::Exact {
        lightmaps: BTreeMap::from([
            ("lm0".into(), "a".repeat(32)),
            ("lm1".into(), "b".repeat(32)),
        ]),
        lightmaps_approx: BTreeMap::new(),
        backdrops: ["sky".to_string()].into(),
        placed,
        models: BTreeMap::from([("crate01".into(), "c".repeat(32))]),
        models_approx: BTreeMap::new(),
    };
    let print = Fingerprint {
        exact: Some(exact),
        ..plain("101per", &"0".repeat(32), &"f".repeat(32))
    };
    let written = text(&print);
    let read = parse(&written);
    check(
        read.as_ref() == Some(&print),
        format!("{print:?} read back as {read:?}"),
    )?;
    let old = read_as_v0_21(&written);
    let want = Some((
        print.area.clone(),
        print.format,
        print.rooms.clone(),
        print.placeables.clone(),
    ));
    check(old == want, format!("kjk v0.21.0 reads {old:?}"))?;
    let other = written.replace(&format!("exact {EXACT}"), &format!("exact {}", EXACT + 1));
    let broken = written.replace("bearing 1.25", "bearing x");
    let skipped: Vec<_> = [other, broken]
        .iter()
        .map(|text| parse(text).map(|print| print.exact.is_some()))
        .collect();
    check(
        skipped == [Some(false), Some(false)],
        format!("another version and a broken line read as having exact lines: {skipped:?}"),
    )
}

/// Re-saved room & placeable MDLs with sub-millimetre / sub-UV float drift
/// or reordered/merged meshes keep their bake match via `*-approx` digests
/// and small GIT tolerance (`5 mm`, `0.01 rad`), while larger changes fail.
fn tolerant_matching_resaved_mdl_and_placeable() -> Result<String, String> {
    let crate_ir = |drift: f32, reversed: bool| -> ModelIr {
        let mut corners = [
            ([0.0 + drift, 0.0, 0.0], [0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 0.0]),
            ([1.0, 1.0, 0.0], [0.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0]),
        ];
        if reversed {
            corners.reverse();
            model(
                "crate01",
                vec![node([0.0; 3])],
                vec![mesh(0, &corners, &[3, 2, 1, 3, 1, 0], None, "crate")],
            )
        } else {
            model(
                "crate01",
                vec![node([0.0; 3])],
                vec![mesh(0, &corners, &[0, 1, 2, 0, 2, 3], None, "crate")],
            )
        }
    };
    let baked_crate = crate_ir(0.0, false);
    let resaved_crate = crate_ir(0.0002, true);
    let shifted_crate = crate_ir(0.05, false);

    let mut baked = current_of(
        &[room("room01", [10.0, 20.0, 0.0])],
        vec![("room01", room_model(vec![square_mesh(SQUARE)]))],
        vec![crate_at(0, "crate01", 1.0, 0.5)],
        &[("crate01", &super::model_digest(&baked_crate))],
    );
    baked
        .models_approx
        .insert("crate01".into(), model_approx_digest(&baked_crate));
    let (stamped, _) = stamp(&baked, &BTreeSet::from(["lm0".to_string()]));
    let reparsed = parse(&text(&stamped)).ok_or("reparse failed")?;

    let drifted_square = SQUARE.map(|(p, uv)| ([p[0] + 0.0002, p[1], p[2]], [uv[0] + 0.0001, uv[1]]));
    let mut resaved = current_of(
        &[room("room01", [10.0, 20.0, 0.0])],
        vec![("room01", room_model(vec![square_mesh(drifted_square)]))],
        vec![crate_at(4, "crate01", 1.002, 0.503)],
        &[("crate01", &super::model_digest(&resaved_crate))],
    );
    resaved
        .models_approx
        .insert("crate01".into(), model_approx_digest(&resaved_crate));
    let matched = fit(&reparsed, &resaved).ok_or("no exact")??;
    check(
        matched.matching == BTreeSet::from(["lm0".to_string()])
            && matched.placeables.get(&4) == Some(&0)
            && !matched.placeables_differ,
        format!("expected drifted MDL to match cleanly: {matched:?}"),
    )?;

    // Legacy fingerprint without *-approx lines must still parse & match exact.
    let mut legacy = reparsed.clone();
    if let Some(exact) = legacy.exact.as_mut() {
        exact.lightmaps_approx.clear();
        exact.models_approx.clear();
    }
    let legacy_matched = fit(&legacy, &baked).ok_or("no exact")??;
    check(
        legacy_matched.matching.contains("lm0") && legacy_matched.placeables.get(&0) == Some(&0),
        format!("legacy match failed: {legacy_matched:?}"),
    )?;

    // Large wall shift (10 cm) and shifted/rotated crate must fail.
    let moved_square = SQUARE.map(|(p, uv)| ([p[0] + 0.1, p[1], p[2]], uv));
    let mut broken = current_of(
        &[room("room01", [10.0, 20.0, 0.0])],
        vec![("room01", room_model(vec![square_mesh(moved_square)]))],
        vec![crate_at(4, "crate01", 1.2, 1.5)],
        &[("crate01", &super::model_digest(&shifted_crate))],
    );
    broken
        .models_approx
        .insert("crate01".into(), model_approx_digest(&shifted_crate));
    let refused = fit(&reparsed, &broken).ok_or("no exact")??;
    check(
        refused.matching.is_empty() && refused.placeables.is_empty() && refused.placeables_differ,
        format!("expected moved geometry/placeable to fail: {refused:?}"),
    )?;

    // Directional companion maps (`*.dir.png`) must never enter `set_lightmaps`.
    let temp_dir = std::env::temp_dir().join(format!("kjk-dir-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    std::fs::write(temp_dir.join("room_lm0.klm"), b"klm").map_err(|e| e.to_string())?;
    std::fs::write(temp_dir.join("room_lm0.dir.png"), b"dir").map_err(|e| e.to_string())?;
    std::fs::write(temp_dir.join("room_lm1.png"), b"png").map_err(|e| e.to_string())?;
    let maps = super::set_lightmaps(&temp_dir);
    let _ = std::fs::remove_dir_all(&temp_dir);
    check(
        maps == BTreeSet::from(["room_lm0".to_string(), "room_lm1".to_string()]),
        format!("unexpected set_lightmaps: {maps:?}"),
    )
}
