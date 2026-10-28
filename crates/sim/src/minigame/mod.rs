//! KOTOR's minigames: the swoop race and the Ebon Hawk's turret
//! (`docs/MINIGAMES.md`). A module whose area's
//! ARE has a `MiniGame` struct installs as any other, and its level carries
//! the minigame's `Rules`; the install starts a `Course`, which the step
//! moves (`Course::step`) before the scripts run, raising the hooks the ARE
//! names. The games' own scripts decide the rest through the `SWMG_*`
//! routines (`script::minigame`), and leave with `StartNewModule`.

mod course;
#[cfg(feature = "probes")]
pub mod probes;
mod rules;
#[cfg(feature = "probes")]
pub mod tsl_probes;

pub(crate) use course::play_into;
pub use course::{
    Anim, Bullet, Course, Fact, Gunner, JUMP_GRAVITY, LEAVE_WAIT_MS, Leaving, Mover, Overlay,
    Raised, Still, Voice,
};
pub use rules::{Bank, Follower, Hook, Hooks, Kind, Obstacle, Path, Rules, targets};

/// The object id of the minigame's thing `n` (the player, the enemies, then
/// the obstacles) in run 0: a block of its own beside the module's and the
/// area's, so the scripts' `OBJECT_SELF` in a hook is an ordinary object.
/// The block holds spans of `RUN_SPAN` ids: one for each run of a co-op
/// race (`Course::runs`, run 0 the course itself), then one for each of the
/// turret's gunners, its lens on run 0 (`Course::gunners`, M11).
pub const FIRST_ID: u32 = 0x7e01_0000;
pub const RUN_SPAN: u32 = 0x1000;
/// The most runs a race keeps apart in ids: seats past it watch the first.
pub const RUNS: u32 = 16;
/// The spans: the runs', then the gunners' (`u8::MAX` at most).
const SPANS: u32 = RUNS + u8::MAX as u32;

/// Thing `n` in span `span` (a run, or `RUNS` on a gunner's lens) as an
/// object id.
pub fn id(span: usize, n: usize) -> u32 {
    FIRST_ID + span as u32 * RUN_SPAN + n as u32
}

/// The span of gunner `gunner` (from 1) on the turret.
pub fn lens(gunner: usize) -> usize {
    RUNS as usize + gunner - 1
}

/// The minigame's thing number of object `id` within its span, if it is
/// one.
pub fn thing(id: u32) -> Option<usize> {
    let at = id.checked_sub(FIRST_ID)?;
    (at < SPANS * RUN_SPAN).then_some((at % RUN_SPAN) as usize)
}

/// The span of object `id`, if it is a minigame's thing.
pub fn span(id: u32) -> Option<usize> {
    thing(id)?;
    Some(((id - FIRST_ID) / RUN_SPAN) as usize)
}

/// The run of object `id`, if it is a minigame's thing: a gunner's lens is
/// on run 0.
pub fn run(id: u32) -> Option<usize> {
    span(id).map(|span| if span < RUNS as usize { span } else { 0 })
}

/// The gunner (from 1) whose lens object `id` is in, if any.
pub fn gunner(id: u32) -> Option<usize> {
    span(id)?
        .checked_sub(RUNS as usize)
        .map(|gunner| gunner + 1)
}

/// The module whose turret's bolts meet the area's walls, creatures and
/// placeables (Peragus' hangar: `world::minigame`). TSL's executable
/// names it, comparing the module's name ignoring case.
pub const WORLD_TURRET: &str = "107per";

/// Whether, in module `module`, a bolt spent before its time stays where it
/// hit playing its `explode` (drawn). TSL's executable names the modules
/// where one goes at once: `505OND` and `module000`, a bolt that hit a
/// follower; `WORLD_TURRET`, any.
pub fn bolts_burst(module: &str) -> bool {
    !["505ond", "module000", WORLD_TURRET]
        .iter()
        .any(|named| module.eq_ignore_ascii_case(named))
}

/// The script a raised hook runs, queued as the scripts' other events are:
/// its thing as the caller, its hit as the event's number.
pub fn pending(rules: &Rules, raised: Raised) -> Option<crate::script::Pending> {
    let script = rules.hooks(usize::from(raised.who))?.get(raised.hook)?;
    let mut pending = crate::script::Pending::new(
        script,
        script_ncs::vm::ObjectId(id(usize::from(raised.span), usize::from(raised.who))),
    );
    pending.number = raised.fact;
    Some(pending)
}
