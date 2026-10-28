//! The crate's probes (`kjk sim probes`).

use super::*;

/// A refusal case: its name, the refusal expected, the change made.
type Case<'a> = (
    &'static str,
    Option<&'a str>,
    &'a dyn Fn(&mut sim::Snapshot),
);

/// The proving ground with Carth standing by the entry: a creature named
/// as a guest's companion is.
fn with_carth() -> Result<sim::Level, String> {
    let ground = sim::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let carth = sim::module::ObjectData {
        name: "Carth".into(),
        ..sim::module::ObjectData::unknown("carth", data.entry.position, 0.0)
    };
    data.placements.push(sim::module::Placement::Own(carth));
    Ok(ground.with_module(data))
}

/// `refusal` on a quiet moment and on each thing that refuses a save, made
/// on the proving ground with Carth (`with_carth`; `kjk sim probes`, group
/// "save refusals").
pub fn refusal_probes() -> Vec<(&'static str, Result<String, String>)> {
    let made = with_carth().and_then(|level| {
        let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), level.clone())
            .map_err(|why| why.to_string())?;
        Ok((world.snapshot(), level))
    });
    let (quiet, level) = match made {
        Ok(made) => made,
        Err(why) => return vec![("refusal_world", Err(why))],
    };
    let seat = sim::Seat(0);
    let with = |change: &dyn Fn(&mut sim::Snapshot)| {
        let mut snapshot = quiet.clone();
        change(&mut snapshot);
        refusal(&snapshot, seat, &level)
    };
    // A match `player` plays (`sim::pazaak::Match::player`).
    let table = |player: u32| {
        let deck = [sim::pazaak::Card::of(7).expect("+1"); sim::pazaak::SIDE_DECK];
        let new = sim::pazaak::Match::new(deck, Vec::new(), (0, 0), false, (String::new(), 0));
        sim::pazaak::Match { player, ..new }
    };
    // A player's own conversation `listener` stands in.
    let talk = |listener: u32| {
        let mut own = sim::script::dialogue_probes::conversation();
        own.listener.0 = listener;
        own.joined.clear();
        own.party = false;
        own
    };
    let own = quiet.body(seat).map_or(0, |body| body.id.0);
    let objects = sim::Instances::in_snapshot(&level, &quiet);
    let carth = quiet
        .entities
        .iter()
        .find(|entity| {
            objects
                .of_entity(entity)
                .is_some_and(|facts| facts.name == "Carth")
        })
        .map_or(0, |entity| entity.id.0);
    // Carth driven by a second seat, as a guest's companion is.
    let guest = |s: &mut sim::Snapshot, carth: u32| {
        s.seats_held = 2;
        let fighter = s
            .entities
            .iter_mut()
            .find(|e| e.id.0 == carth)
            .and_then(|e| e.fighter.as_mut());
        if let Some(fighter) = fighter {
            fighter.mind.driver = Some(sim::Seat(1));
        }
    };
    let cases: [Case<'_>; 14] = [
        ("refusal_quiet", None, &|_| {}),
        (
            "refusal_in_combat",
            Some("You cannot save during combat."),
            &|s| s.in_combat = true,
        ),
        ("refusal_combat_timer_alone", None, &|s| {
            let now = s.time_ms;
            for fighter in s.entities.iter_mut().filter_map(|e| e.fighter.as_mut()) {
                fighter.combat_until = now + 1000;
            }
        }),
        (
            "refusal_break",
            Some("You cannot save during a break."),
            &|s| s.paused_by = Some(sim::Seat(1)),
        ),
        (
            "refusal_ready_check",
            Some("You cannot save while the party waits at an exit."),
            &|s| {
                s.ready = Some(sim::ReadyCheck {
                    module: "probe_b".into(),
                    tag: "exit".into(),
                    via: sim::SimId(0),
                    asked_by: sim::Seat(0),
                    ready: 1,
                    since_ms: s.time_ms,
                })
            },
        ),
        (
            "refusal_minigame",
            Some("You cannot save during a minigame."),
            &|s| {
                let rules = sim::minigame::Rules::bare(sim::minigame::Kind::Turret);
                s.minigame = Some(sim::minigame::Course::new(&rules));
            },
        ),
        (
            "refusal_pazaak",
            Some("You cannot save during a minigame."),
            &|s| s.pazaak = vec![table(own)],
        ),
        // Another player's match or conversation (R11): anyone's refuses,
        // in its body's name; one with no name as the saving seat's own.
        (
            "refusal_pazaak_other",
            Some("Carth is playing pazaak."),
            &|s| s.pazaak = vec![table(carth)],
        ),
        (
            "refusal_pazaak_unnamed",
            Some("You cannot save during a minigame."),
            &|s| s.pazaak = vec![table(own + 1000)],
        ),
        (
            "refusal_talk_own",
            Some("You cannot save during a conversation."),
            &|s| s.talk.conversations = vec![talk(own)],
        ),
        (
            "save_refusal_names",
            Some("Carth is in a conversation."),
            &|s| {
                guest(s, carth);
                s.talk.conversations = vec![talk(carth)];
            },
        ),
        // Two others talking, Carth no seat's: in KOTOR's words.
        (
            "refusal_talk_npcs",
            Some("You cannot save during a conversation."),
            &|s| s.talk.conversations = vec![talk(carth)],
        ),
        (
            "refusal_talk_party",
            Some("You cannot save during a conversation."),
            &|s| {
                s.talk.conversations = vec![sim::script::dialogue::Conversation {
                    party: true,
                    ..talk(carth)
                }]
            },
        ),
        (
            "refusal_down",
            Some("You cannot save while you are down."),
            &|s| {
                if let Some(fighter) = s
                    .entities
                    .iter_mut()
                    .find(|e| e.kind == sim::Kind::Avatar(sim::Seat(0)))
                    .and_then(|e| e.fighter.as_mut())
                {
                    fighter.life = sim::combat::fighter::Life::Dead;
                }
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, expect, change)| {
            let got = with(change);
            let detail = format!("{got:?}");
            (
                name,
                if got.as_deref() == expect {
                    Ok(detail)
                } else {
                    Err(detail)
                },
            )
        })
        .collect()
}
