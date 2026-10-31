//! What `kjk play` (and the headless runs) set by hand before the first
//! step: a skill's rank (`--skill`), items the party carries (`--item`),
//! script globals (`--global`), scripts run for the player (`--run`), a
//! conversation started (`--talk`), and companions in the party
//! (`--party`).

use std::sync::OnceLock;

use script_ncs::vm::ObjectId;
use vfs::Game;

/// The skills' labels (`skills.2da` `label`), by row, read once from the
/// first KOTOR install found: the rows are NWScript's `SKILL_*`, the same
/// in both games.
fn skill_labels() -> Result<&'static [String], String> {
    static LABELS: OnceLock<Result<Vec<String>, String>> = OnceLock::new();
    LABELS
        .get_or_init(|| {
            let discovery = crate::discover();
            let table = [Game::Kotor1, Game::Kotor2]
                .into_iter()
                .find_map(|game| crate::viewer::resolver(&discovery, game, None).ok())
                .ok_or("no KOTOR install to read skills.2da from")?
                .two_da("skills")
                .ok_or("skills.2da: not in the install")?;
            Ok((0..table.cells.len())
                .map(|row| table.text(row, "label").unwrap_or_else(String::new))
                .collect())
        })
        .as_deref()
        .map_err(Clone::clone)
}

/// `NAME=RANK`, a skill's rank (`--skill computer=8`): NAME is a skill's
/// `skills.2da` label (`ComputerUse`), or the start of one alone.
pub fn parse_skill(text: &str) -> Result<(usize, u8), String> {
    let (name, rank) = text
        .split_once('=')
        .ok_or_else(|| format!("{text}: NAME=RANK"))?;
    let labels = skill_labels()?;
    let name = name.trim().to_ascii_lowercase();
    let starting: Vec<usize> = (0..labels.len())
        .filter(|&row| labels[row].to_ascii_lowercase().starts_with(&name))
        .collect();
    let row = match starting[..] {
        _ if name.is_empty() => None,
        [row] => Some(row),
        _ => starting
            .into_iter()
            .find(|&row| labels[row].eq_ignore_ascii_case(&name)),
    }
    .ok_or_else(|| format!("{name}: one of {}", labels.join(", ")))?;
    let rank = rank
        .trim()
        .parse()
        .map_err(|_| format!("{text}: {rank} is not a rank (0 to 255)"))?;
    Ok((row, rank))
}

/// `TEMPLATE` or `TEMPLATE=COUNT`, items the party carries (`--item`).
pub fn parse_item(text: &str) -> Result<(String, u16), String> {
    let (template, count) = text.split_once('=').unwrap_or((text, "1"));
    let count = count
        .trim()
        .parse()
        .map_err(|_| format!("{text}: {count} is not a count"))?;
    Ok((template.trim().to_ascii_lowercase(), count))
}

/// `sheet` with the ranks `--skill` gives.
pub fn with_skills(mut sheet: sim::combat::Sheet, skills: &[(usize, u8)]) -> sim::combat::Sheet {
    for &(row, rank) in skills {
        sheet.skills[row] = rank;
    }
    sheet
}

/// Items of the templates given made in the party's inventory of
/// `world`'s scripts, as a script's `CreateItemOnObject` makes them.
pub fn give_items(world: &mut sim::SimWorld, items: &[(String, u16)]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let library = world
        .level()
        .scripts()
        .cloned()
        .ok_or("the stage has no scripts")?;
    let mut state = world.script_state().clone();
    for (template, count) in items {
        state
            .make_item(
                library.items(),
                template,
                sim::inventory::PARTY,
                *count,
                None,
            )
            .ok_or(format!("{template}: no such item template"))?;
    }
    world.set_script_state(state);
    Ok(())
}

/// The player talks to the first object tagged `tag`, as using it does
/// (`world::scripts::talk_to`): farther than talking range, the player first
/// stands where a use's walk would bring it (`TALK_STAND` short of the
/// object along the walkmesh's route to it, `walked_up`; with no route, on
/// the line from where it stood), facing it; then the object's
/// dialogue hook runs with listen pattern −1 and the player as speaker (the
/// games' handlers clear the owner's actions, then begin its conversation);
/// one with no hook starts its conversation.
///
/// It is a start by hand, as `set_globals` is, made before the first step:
/// a seat's own talk (`Action::Use` with `Verb::Talk`) is refused out of
/// reach, and the walk that brings a player there is the view's, step by
/// step (`sim::target::Walk`), so the player is put there at once (a
/// jump). The use itself is told to the scripts within a step only
/// (`scripts::Used`, cleared as each step's uses begin), hence its rule
/// made here.
pub fn talk_to(world: &mut sim::SimWorld, tag: &str) -> Result<(), String> {
    let snapshot = world.snapshot();
    let me = ObjectId(
        crate::cast::me(&snapshot, sim::Seat(0))
            .ok_or("no player")?
            .0,
    );
    let objects = sim::Instances::in_snapshot(world.level(), &snapshot);
    let (owner, object, there) = snapshot
        .entities
        .iter()
        .find_map(|entity| {
            let object = objects.of_entity(entity)?;
            object
                .tag
                .eq_ignore_ascii_case(tag)
                .then_some((ObjectId(entity.id.0), object, entity))
        })
        .ok_or(format!("{tag}: no such object"))?;
    // Its soles: a character's position is its box's origin, above them.
    let raised = there
        .player
        .map_or(0.0, |_| sim::motion::SOLE_Z * collision::UNIT);
    let there = there.position;
    let here = snapshot
        .entities
        .iter()
        .find(|entity| entity.id.0 == me.0)
        .map(|entity| entity.position)
        .ok_or("no player")?;
    let hook = sim::script::Hook::Dialogue.of(object).map(str::to_string);
    let dialog = object.conversation.clone();
    let mut state = world.script_state().clone();
    let (dx, dy) = (here[0] - there[0], here[1] - there[1]);
    let apart = dx.hypot(dy);
    if apart > sim::script::CONVERSATION_RANGE {
        let straight = [
            there[0] + dx / apart * TALK_STAND,
            there[1] + dy / apart * TALK_STAND,
            there[2] + raised,
        ];
        let stand = walked_up(
            world,
            &snapshot,
            me,
            [there[0], there[1], there[2] + raised],
        )
        .unwrap_or(straight);
        let (dx, dy) = (stand[0] - there[0], stand[1] - there[1]);
        let facing = (-dy).atan2(-dx).to_degrees();
        state
            .actions
            .entry(me.0)
            .or_default()
            .push(sim::script::actions::Queued {
                action: sim::script::actions::Action::Jump {
                    to: sim::script::actions::Target::Point(stand, facing),
                },
                started_ms: None,
            });
    }
    match hook {
        Some(script) => {
            use sim::script::last;
            state.last.insert((owner.0, last::SPEAKER), me.0);
            let mut pending = sim::script::Pending::new(&script, owner);
            pending.entering = Some(me);
            pending.number = -1;
            pending.asker = Some(me);
            state.pending.push(pending);
        }
        None if dialog.is_empty() => return Err(format!("{tag}: has no conversation")),
        None => state.talk.asked.push(sim::script::dialogue::Start {
            owner,
            pc: me,
            asker: ObjectId::INVALID,
            dialog,
            bark: false,
            cue: None,
        }),
    }
    world.set_script_state(state);
    Ok(())
}

/// How far from the object `--talk` stands the player (metres): inside
/// talking range (`CONVERSATION_RANGE`), at arm's length.
const TALK_STAND: f32 = 1.5;

/// Where a use's walk from the player (`me`) toward soles `there` (metres)
/// first comes within `TALK_STAND` of it, along the walkmesh's route
/// (`sim::walk::route`, the doors as they stand): the player's soles there,
/// else none (no route).
fn walked_up(
    world: &sim::SimWorld,
    snapshot: &sim::Snapshot,
    me: ObjectId,
    there: [f32; 3],
) -> Option<[f32; 3]> {
    use sim::motion::SOLE_Z;
    let unit = collision::UNIT;
    let from = snapshot
        .entities
        .iter()
        .find(|e| e.id.0 == me.0)?
        .player?
        .origin;
    let goal = [there[0] / unit, there[1] / unit, there[2] / unit - SOLE_Z];
    let states = door_states(snapshot);
    let corners = sim::walk::route(world.level(), from, goal, &states)?;
    let flat = |a: [f32; 3]| ((a[0] - goal[0]) * unit).hypot((a[1] - goal[1]) * unit);
    let mut last = from;
    for corner in corners {
        let (near, far) = (flat(corner), flat(last));
        if near <= TALK_STAND {
            // The point on this leg TALK_STAND from the goal (the leg's
            // ends straddle it: far outside, near inside).
            let share = if far > near {
                ((far - TALK_STAND) / (far - near)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let at = sim::math::mad(last, share, sim::math::sub(corner, last));
            return Some([at[0] * unit, at[1] * unit, (at[2] + SOLE_Z) * unit]);
        }
        last = corner;
    }
    None
}

/// Each module object's state in `snapshot` by its index (one the snapshot
/// lacks is out of play, `ObjectState::GONE`): what `sim::walk::route` asks
/// of the doors on the way.
pub fn door_states(snapshot: &sim::Snapshot) -> impl Fn(u32) -> sim::ObjectState + use<> {
    let states: std::collections::BTreeMap<u32, sim::ObjectState> = snapshot
        .entities
        .iter()
        .filter_map(|entity| match (entity.kind, entity.object) {
            (sim::Kind::Object(_, index), Some(object)) => Some((index, object)),
            _ => None,
        })
        .collect();
    move |index: u32| {
        states
            .get(&index)
            .copied()
            .unwrap_or(sim::ObjectState::GONE)
    }
}

/// `NAME=NUMBER`, a script global (`--global`).
pub fn parse_global(text: &str) -> Result<(String, i32), String> {
    let (name, value) = text
        .split_once('=')
        .ok_or_else(|| format!("{text}: NAME=NUMBER"))?;
    let value = value
        .trim()
        .parse()
        .map_err(|_| format!("{text}: {value} is not a whole number"))?;
    Ok((name.trim().to_string(), value))
}

/// Sets script globals (`SetGlobalNumber`'s) in `world`'s scripts.
pub fn set_globals(world: &mut sim::SimWorld, globals: &[(String, i32)]) {
    if globals.is_empty() {
        return;
    }
    let mut state = world.script_state().clone();
    for (name, value) in globals {
        sim::script::set_global(&mut state.numbers, name, *value);
    }
    world.set_script_state(state);
}

/// Queues scripts of `world`'s module (by resref) to run for the first
/// seat's player at the end of the first step: a story's moment by hand
/// (`CreateObject`'s reinforcements, a companion woken), as `set_globals`
/// sets its state.
pub fn queue_scripts(world: &mut sim::SimWorld, scripts: &[String]) -> Result<(), String> {
    if scripts.is_empty() {
        return Ok(());
    }
    let me = crate::cast::me(&world.snapshot(), sim::Seat(0)).ok_or("no player to run them for")?;
    let library = world.level().scripts().ok_or("the stage has no scripts")?;
    let mut state = world.script_state().clone();
    for script in scripts {
        library
            .program(script)
            .ok_or(format!("{script}: not among the module's scripts"))?;
        state
            .pending
            .push(sim::script::Pending::new(script, ObjectId(me.0)));
    }
    world.set_script_state(state);
    Ok(())
}

/// The party of companions made from `templates` (e.g. `p_cand`, two at
/// most), as the story's `AddAvailableNPCByTemplate` and the party
/// selection screen leave it: each available as the `NPC_*` number
/// `npc.2da` gives its template's tag (else the first free), then picked
/// on the screen in the step this takes (`Action::ChooseParty`), which
/// makes each beside the player. Without templates, nothing is done and no
/// step taken.
pub fn party(
    world: &mut sim::SimWorld,
    templates: &[String],
    installer: Option<&crate::lifecycle::Installer>,
) -> Result<(), String> {
    if templates.is_empty() {
        return Ok(());
    }
    if templates.len() > 2 {
        return Err("--party: two companions at most".into());
    }
    let library = world
        .level()
        .scripts()
        .ok_or("the stage has no scripts")?
        .clone();
    let numbers = installer.map_or_else(Vec::new, crate::stage::npc_tags);
    let mut state = world.script_state().clone();
    let mut picks = [u8::MAX; 2];
    for (template, pick) in templates.iter().zip(&mut picks) {
        let template = template.to_ascii_lowercase();
        let tag = library
            .creature(&template)
            .ok_or(format!("--party {template}: no such creature template"))?
            .tag
            .clone();
        let free = |npc: &u8| !state.party.available.contains_key(npc);
        let npc = numbers
            .iter()
            .position(|known| known.eq_ignore_ascii_case(&tag))
            .and_then(|row| u8::try_from(row).ok())
            .filter(free)
            .or_else(|| (0..u8::MAX).find(free))
            .ok_or("--party: no free NPC number")?;
        state.party.available.insert(
            npc,
            sim::script::party::Npc {
                template,
                object: None,
                kept: None,
                template_data: None,
            },
        );
        *pick = npc;
    }
    state.party.selection = Some(sim::script::party::Selection {
        exit: String::new(),
        forced: Vec::new(),
    });
    world.set_script_state(state);
    let mut input = sim::TickInput::default();
    input.act(sim::Seat(0), sim::action::Action::ChooseParty(Some(picks)));
    let tick = world.snapshot().tick + 1;
    let rate = sim::TickRate::default();
    sim::step(
        world,
        tick,
        &input,
        rate.msec(tick),
        sim::StepReason::Authority,
    );
    Ok(())
}

/// A sheet with Jedi Defense at `level` (0 to 3) and nothing else of it.
pub fn with_defence(mut sheet: sim::combat::Sheet, level: usize) -> sim::combat::Sheet {
    for row in sim::combat::sheet::feats::JEDI_DEFENSE {
        sheet.feats.0[usize::from(row / 64)] &= !(1u64 << (row % 64));
    }
    for row in &sim::combat::sheet::feats::JEDI_DEFENSE[..level] {
        sheet.feats.insert(*row);
    }
    sheet
}
