//! The node names the engine looks for on a model, found by name without
//! case (`ModelIr::node`). Hooks and dummies (the executable's strings,
//! and the names reone looks up for creatures, dialogues and animated objects):

/// Where a held weapon hangs.
pub const RIGHT_HAND: &str = "rhand";
pub const LEFT_HAND: &str = "lhand";
/// The hands by side: right, left.
pub const HANDS: [&str; 2] = [RIGHT_HAND, LEFT_HAND];
/// Where a head model hangs from a body.
pub const HEAD_HOOK: &str = "headhook";
/// A camera model's eye (dialogue cameras, the menus' stages).
pub const CAMERA_HOOK: &str = "camerahook";
/// Where an animated cutscene's stunt model puts its actor.
pub const CUTSCENE_DUMMY: &str = "cutscenedummy";
/// A visual effect's attach points on a creature (`visualeffects.2da`'s
/// `*_Impact_Node`, `*_HeadCon_Node`, a power's hand): the executable's
/// names for them on a creature's model.
pub const IMPACT: &str = "impact";
pub const HEAD_CONJURE: &str = "headconjure";
pub const HAND_CONJURE: &str = "handconjure";

/// Bones of the games' humanoid skeletons the view reads.
pub const HEAD: &str = "head_g";
pub const TORSO: &str = "torso_g";
/// The chest and upper back, under `TORSO`: what a body's idles breathe
/// with.
pub const UPPER_TORSO: &str = "torsoupr_g";
pub const PELVIS: &str = "pelvis_g";
pub const ROOT_DUMMY: &str = "rootdummy";
