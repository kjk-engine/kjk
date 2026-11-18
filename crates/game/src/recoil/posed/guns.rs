//! Every K1 gun raised on a body, a man's and a woman's: how near the head
//! comes to the gun, where the stock sits in the shoulder, where each hand
//! stands, and where the barrel points. The pose is tuned on the blaster
//! rifle, and these checks hold every other gun to it.

use super::*;

/// The guns measured: a name, the K1 model, its `baseitems.2da` row, and
/// how it is held. Every two-handed gun model of a different shape is here
/// (models whose files are the same size share their shape and differ only
/// in their texture), and each pistol at its base item's model variation 1.
/// The blaster carbine `w_blstrcrbn_005` is the one with the orange
/// magazine and the scope (items `g_w_blstrcrbn005` to `009`).
const GUNS: [(&str, &str, u16, Wield); 24] = [
    ("blaster-rifle", "w_blstrrfl_001", 77, Wield::Rifle),
    ("blaster-rifle-3", "w_blstrrfl_003", 77, Wield::Rifle),
    ("blaster-rifle-6", "w_blstrrfl_006", 77, Wield::Rifle),
    ("blaster-carbine", "w_blstrcrbn_001", 20, Wield::Rifle),
    ("blaster-carbine-2", "w_blstrcrbn_002", 20, Wield::Rifle),
    ("blaster-carbine-3", "w_blstrcrbn_003", 20, Wield::Rifle),
    ("blaster-carbine-4", "w_blstrcrbn_004", 20, Wield::Rifle),
    ("blaster-carbine-5", "w_blstrcrbn_005", 20, Wield::Rifle),
    ("repeating-blaster", "w_rptnblstr_001", 23, Wield::Rifle),
    ("repeating-blaster-2", "w_rptnblstr_002", 23, Wield::Rifle),
    ("repeating-blaster-3", "w_rptnblstr_003", 23, Wield::Rifle),
    (
        "heavy-repeating-blaster",
        "w_hvrptbltr_001",
        24,
        Wield::Rifle,
    ),
    ("disruptor-rifle", "w_dsrptrfl_001", 21, Wield::Rifle),
    ("ion-rifle", "w_ionrfl_001", 18, Wield::Rifle),
    ("ion-rifle-2", "w_ionrfl_002", 18, Wield::Rifle),
    ("sonic-rifle", "w_sonicrfl_001", 22, Wield::Rifle),
    ("bowcaster", "w_bowcstr_001", 19, Wield::Rifle),
    ("blaster-pistol", "w_blstrpstl_001", 12, Wield::Pistol),
    ("heavy-blaster", "w_hvyblstr_001", 13, Wield::Pistol),
    ("hold-out-blaster", "w_hldoblst_001", 14, Wield::Pistol),
    ("ion-blaster", "w_ionblstr_001", 15, Wield::Pistol),
    ("disruptor-pistol", "w_dsrptpstl_001", 16, Wield::Pistol),
    ("sonic-pistol", "w_sonicpstl_001", 17, Wield::Pistol),
    (
        "pair-of-blaster-pistols",
        "w_blstrpstl_001",
        12,
        Wield::Pair,
    ),
];

/// The bodies measured: a name, the K1 body model and its head model.
/// Carth's body, as the other posed checks use, and a player woman's.
const BODIES: [(&str, &str, &str); 2] = [
    ("carth", "p_carthbb", "p_carthh"),
    ("woman", "pfbal", "pfha01"),
];

/// How far (degrees) a raised gun's barrel may point off its target.
const BARREL_OFF_MOST: f32 = 0.1;
/// The least distance (metres) between any gun of `GUNS` and the head, all
/// through a shot. The yellow blaster carbine's scope (`w_blstrcrbn_003`)
/// comes within 4 mm of the eye at the peak of a shot.
const HEAD_CLEAR_EACH: f32 = 0.003;

/// One gun raised on one body, aimed level and fired once.
struct Measured {
    /// How far (metres) the head stands from the gun before the shot, and
    /// the least during it.
    head: (f32, f32),
    /// Where the middle of the stock's rear end stands from the gun-side
    /// shoulder joint before the shot (metres): ahead along the aim, up, and
    /// to the right. Only for a gun held at the shoulder.
    stock: Option<[f32; 3]>,
    /// How far (metres) ahead of the gun-side shoulder joint the gun's grip
    /// (the right hand's weapon node) stands along the aim.
    grip_ahead: f32,
    /// The most (metres) the support palm stands from its grip during the
    /// shot. Only for a gun held in both hands.
    support: Option<f32>,
    /// The least (metres) each forearm, right then left, stands outside the
    /// chest during the shot.
    forearms: [f32; 2],
    /// The angle (degrees) between the barrel and the line from the muzzle
    /// to the target, before the shot.
    barrel_off_deg: f32,
    /// How far (degrees) the right wrist is bent from its rest, and the
    /// right elbow's inner angle (degrees), before the shot.
    wrist_deg: f32,
    elbow_deg: f32,
    /// How far (metres) behind the grip the gun's rear end stands, and how
    /// far ahead of it the muzzle stands, along the barrel.
    behind_grip: f32,
    ahead_of_grip: f32,
}

/// `wield`'s gun, a `blaster`, raised on the scene's body, aimed level and
/// fired once.
fn measure(scene: &Scene, wield: Wield, blaster: Blaster) -> Result<Measured, String> {
    let node = |name: &str| scene.node(name);
    let (gun, shoulder, elbow, hand) = (
        node("rhand")?,
        node("rbicep_g")?,
        node("rforearm_g")?,
        node("rhand_g")?,
    );
    let shape = scene.shape(wield);
    let pose = scene.ja_pose(wield, true)?;
    let shooter = Shooter::new(scene, (wield, blaster), &pose)?;
    let (target, forward, right) = (shooter.targets[0], shooter.forward, shooter.right);
    let firing = shooter.firing(Some(0.0), 1.0);
    let (moments, ..) = fired(scene, (wield, blaster, 1.0), &pose)?;
    let before = &moments[0].1;
    let head_at = |globals: &[(Vec3, Quat)], back: f32| {
        let posed = welded_head(scene, wield, globals, (target, back))?;
        head_points(scene, &posed).map(|(points, _)| points)
    };
    let head_before = gap(scene, before, &head_at(before, 0.0)?)?;
    let mut head_least = f32::INFINITY;
    for (now, globals) in moments.iter().step_by(CLEAR_EVERY) {
        let back = push(&firing, 0, *now).back_m;
        head_least = head_least.min(gap(scene, globals, &head_at(globals, back)?)?);
    }
    let (at, turn) = before[gun];
    let along = |point: &Vec3| point.dot(shape.barrel);
    let rearmost = (scene.rifle_points.iter().map(along)).fold(f32::INFINITY, f32::min);
    let stock = (wield == Wield::Rifle).then(|| {
        let off = at + turn * shape.butt - before[shoulder].0;
        [off.dot(forward), off.z, off.dot(right)]
    });
    let grip_ahead = (at - before[shoulder].0).dot(forward);
    let support = (wield == Wield::Rifle).then(|| {
        let grip = shape.foregrip(RIFLE_SUPPORT);
        (moments.iter())
            .map(|(_, globals)| palm(scene, globals, grip))
            .collect::<Result<Vec<f32>, String>>()
            .map(|gaps| gaps.into_iter().fold(0.0, f32::max))
    });
    let support = support.transpose()?;
    let torso = [
        node("torso_g")?,
        node("torsoupr_g")?,
        node("rcollar_g")?,
        node("lcollar_g")?,
    ];
    let mut forearms = [f32::INFINITY; 2];
    for (_, globals) in moments.iter().step_by(CLEAR_EVERY) {
        for (least, name) in forearms.iter_mut().zip(["rforearm_g", "lforearm_g"]) {
            let clear = limb_clearance(scene, globals, (&[node(name)?], &torso), 0.0);
            *least = least.min(clear);
        }
    }
    let muzzle = at + turn * shape.muzzle;
    let barrel_off_deg = (turn * shape.barrel)
        .angle_between(target - muzzle)
        .to_degrees();
    let rest = &scene.skin.rest;
    let bend = |globals: &[(Vec3, Quat)]| globals[elbow].1.inverse() * globals[hand].1;
    let wrist_deg = bend(before).angle_between(bend(rest)).to_degrees();
    let (s, e, w) = (before[shoulder].0, before[elbow].0, before[hand].0);
    let elbow_deg = (s - e).angle_between(w - e).to_degrees();
    Ok(Measured {
        head: (head_before, head_least),
        stock,
        grip_ahead,
        support,
        forearms,
        barrel_off_deg,
        wrist_deg,
        elbow_deg,
        behind_grip: -rearmost,
        ahead_of_grip: shape.muzzle.dot(shape.barrel),
    })
}

/// How far (metres) the support hand's palm (`aim_pose::support_palm`)
/// stands from `grip` (the gun's space) on the body posed as `globals`.
fn palm(scene: &Scene, globals: &[(Vec3, Quat)], grip: Vec3) -> Result<f32, String> {
    let fingers = |end: char| -> Result<Vec3, String> {
        let mut sum = Vec3::ZERO;
        for finger in ['a', 'b', 'c', 'd'] {
            sum += globals[scene.node(&format!("l{finger}fngr{end}_g"))?].0;
        }
        Ok(sum / 4.0)
    };
    let wrist = globals[scene.node("lhand_g")?].0;
    let palm = crate::aim_pose::support_palm(wrist, fingers('b')?, fingers('t')?);
    let (at, turn) = globals[scene.node("rhand")?];
    Ok(palm.distance(at + turn * grip))
}

/// Every gun of `GUNS` raised on the body `body` of `BODIES`, aimed level
/// and fired once. Checked: the gun stays at least `HEAD_CLEAR_EACH` from
/// the head, and a gun at the shoulder points within `BARREL_OFF_MOST` of
/// its target, with its stock's rear end in front of the shoulder joint.
/// The support palm, the forearms, the wrist and the elbow are measured,
/// not checked: a woman's shorter arm does not reach the far grip of the
/// longest guns, and on Carth's body the trigger forearm of the guns held
/// nearest the shoulder goes 2 to 6 cm into the chest. A pistol's barrel is
/// measured before the game turns it back onto its target (`cast::reaim`),
/// so it is not checked here.
fn each_gun(scene: &mut Scene, body: (&str, &str, &str)) -> Result<String, String> {
    let discovery = crate::discover();
    let (name, model, head) = body;
    scene.rebody(&discovery, (model, head))?;
    let (mut seen, mut failed) = (Vec::new(), false);
    for (gun, model, row, wield) in GUNS {
        let Some(sim::combat::Delivery::Blaster(blaster)) =
            sim::combat::Delivery::of_base_item(row)
        else {
            return Err(format!("{gun}: baseitems.2da row {row} is no blaster"));
        };
        scene.regun(&discovery, (model, wield))?;
        let m = measure(scene, wield, blaster)?;
        let (before, least) = m.head;
        let [right, left] = m.forearms;
        let shouldered = m.stock.is_some();
        failed |= least < HEAD_CLEAR_EACH
            || (shouldered && m.barrel_off_deg > BARREL_OFF_MOST)
            || m.stock.is_some_and(|[ahead, ..]| ahead <= 0.0);
        let stock = m.stock.map_or(String::new(), |[ahead, up, aside]| {
            format!(
                ", the stock's rear end {ahead:.4} m ahead of the shoulder joint, {up:.4} m above it and {aside:.4} m to its right"
            )
        });
        let support = m.support.map_or(String::new(), |gap| {
            format!(", the support palm at most {gap:.4} m from its grip")
        });
        seen.push(format!(
            "{gun} ({model}) on {name}: the head {before:.4} m from the gun before the shot and at least {least:.4} m during it{stock}, the grip {:.4} m ahead of the shoulder joint{support}, the right forearm {right:.4} m and the left {left:.4} m outside the chest, the barrel {:.3} degrees off its target, the right wrist bent {:.1} degrees and the right elbow at {:.1} degrees; the gun ends {:.4} m behind its grip and its muzzle stands {:.4} m ahead of it",
            m.grip_ahead,
            m.barrel_off_deg,
            m.wrist_deg,
            m.elbow_deg,
            m.behind_grip,
            m.ahead_of_grip,
        ));
    }
    check(!failed, seen.join("; "))
}

/// The checks of `each_gun`, one for each body, named `NAMES`.
pub(super) const NAMES: [&str; 2] = ["each_gun_raised_on_carth", "each_gun_raised_on_a_woman"];

pub(super) fn checks(scene: &mut Scene) -> Vec<(&'static str, Result<String, String>)> {
    (NAMES.into_iter().zip(BODIES))
        .map(|(name, body)| (name, each_gun(scene, body)))
        .collect()
}
