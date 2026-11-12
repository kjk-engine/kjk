//! The probes of `battle_music` (`kjk sim probes`, group "battle music").

use super::*;
use sim::combat::sheet::factions::{HOSTILE_1, NEUTRAL, PLAYER};
use sim::module::Repute;
use sim::probe::check;

const ME: u32 = 1;
const CARTH: u32 = 2;
const FOE: u32 = 3;
const BYSTANDER: u32 = 4;
const SECOND: u32 = 5;

fn body(id: u32, x: f32, faction: u8) -> Body {
    Body {
        id,
        at: [x, 0.0, 0.0],
        seat: matches!(id, ME | SECOND),
        party: matches!(id, ME | CARTH | SECOND),
        alive: true,
        faction: Some(faction),
    }
}

/// One step of a scene: its tick (100 ms each), the blows of that step.
struct Scene {
    fight: Fight,
    repute: Repute,
    /// How scripts changed the standings (`sim::Snapshot::standing`).
    adjusted: sim::module::Adjustments,
    bodies: Vec<Body>,
    /// The ticks a conversation runs, from and to (the end excluded).
    talk: Option<(u64, u64)>,
}

impl Scene {
    fn new(bodies: Vec<Body>) -> Scene {
        Scene {
            fight: Fight::default(),
            repute: Repute::at_war([HOSTILE_1, PLAYER]),
            adjusted: sim::module::Adjustments::new(),
            bodies,
            talk: None,
        }
    }

    /// The scene with a conversation running from tick `from` to `to`.
    fn talking(self, from: u64, to: u64) -> Scene {
        Scene {
            talk: Some((from, to)),
            ..self
        }
    }

    fn at(&mut self, tick: u64, blows: &[(u32, u32)], scripted: u8) -> bool {
        let blows: Vec<Blow> = (blows.iter())
            .map(|&(on, by)| Blow { on, by, tick })
            .collect();
        let moment = Moment {
            tick,
            now_ms: tick * 100,
            bodies: &self.bodies,
            blows: &blows,
            scripted,
            talking: self
                .talk
                .is_some_and(|(from, to)| (from..to).contains(&tick)),
        };
        self.fight
            .step(&moment, hostility(Some(self.repute.with(&self.adjusted))))
    }

    /// The flag at each of `ticks`, the blows given at their ticks.
    fn run(
        &mut self,
        ticks: &[u64],
        blows: &[(u64, u32, u32)],
        scripted: &[(u64, u8)],
    ) -> Vec<bool> {
        let mut say = sound::AS_PLACED;
        let last = *ticks.iter().max().unwrap_or(&0);
        let mut seen = Vec::new();
        for tick in 1..=last {
            if let Some((_, state)) = scripted.iter().find(|(at, _)| *at == tick) {
                say = *state;
            }
            let now: Vec<(u32, u32)> = (blows.iter())
                .filter(|(at, ..)| *at == tick)
                .map(|&(_, on, by)| (on, by))
                .collect();
            let on = self.at(tick, &now, say);
            if ticks.contains(&tick) {
                seen.push(on);
            }
        }
        seen
    }
}

pub(super) fn checks() -> sim::probe::Checks {
    let party = |foe_at: f32| {
        vec![
            body(ME, 0.0, PLAYER),
            body(CARTH, 3.0, PLAYER),
            body(FOE, foe_at, HOSTILE_1),
            body(BYSTANDER, 5.0, NEUTRAL),
        ]
    };
    let mut cases = Vec::new();
    let mut case = |name: &'static str, seen: Vec<bool>, want: &[bool]| {
        cases.push((
            name,
            check(seen == want, format!("{seen:?}, want {want:?}")),
        ));
    };
    // Shots and swings at the air land on no one: nothing stirs.
    case(
        "air: shots at nothing play none",
        Scene::new(party(12.0)).run(&[5, 50], &[], &[]),
        &[false, false],
    );
    // A blow on a friend, no foe within 30 m: stirred, no battle music.
    case(
        "a friend struck, no foe near: none",
        Scene::new(party(45.0)).run(&[5, 50], &[(2, CARTH, ME), (3, BYSTANDER, ME)], &[]),
        &[false, false],
    );
    // A foe 12 m off strikes the player: on until 10 s after the last blow.
    case(
        "a foe's blow: on, off 10 s after the last",
        Scene::new(party(12.0)).run(&[3, 60, 101, 102], &[(2, ME, FOE), (2, FOE, ME)], &[]),
        &[true, true, true, false],
    );
    // The player strikes a bystander (a container's stand-in) with a foe
    // 25 m away: the foe's nearness starts it.
    case(
        "a bystander struck with a foe 25 m off: on",
        Scene::new(party(25.0)).run(&[3], &[(2, BYSTANDER, ME)], &[]),
        &[true],
    );
    // A foe's faction a script made peaceable (Tatooine's Sand People with
    // the party in their robes): its nearness starts nothing.
    let mut at_peace = Scene::new(party(25.0));
    sim::module::adjust(
        &at_peace.repute,
        &mut at_peace.adjusted,
        PLAYER,
        HOSTILE_1,
        50,
    );
    case(
        "a bystander struck with a foe made peaceable 25 m off: none",
        at_peace.run(&[3], &[(2, BYSTANDER, ME)], &[]),
        &[false],
    );
    // A foe strikes Carth 3 m from the player: the party's blow stirs it.
    case(
        "Carth struck by a foe near the player: on",
        Scene::new(party(10.0)).run(&[3], &[(2, CARTH, FOE)], &[]),
        &[true],
    );
    // Carth 22 m from the player (past PercepRngDefault) struck by a foe
    // 25 m off: the player is not stirred.
    let far_carth = vec![
        body(ME, 0.0, PLAYER),
        body(CARTH, 22.0, PLAYER),
        body(FOE, 25.0, HOSTILE_1),
    ];
    case(
        "Carth struck 22 m off: none",
        Scene::new(far_carth).run(&[3], &[(2, CARTH, FOE)], &[]),
        &[false],
    );
    // A cutscene's fight between others (a foe and a bystander) near the
    // player: none.
    case(
        "others' fight near the player: none",
        Scene::new(party(8.0)).run(&[3, 20], &[(2, BYSTANDER, FOE), (2, FOE, BYSTANDER)], &[]),
        &[false, false],
    );
    // A script's battle music plays on until the fight it scored ends.
    case(
        "a script's music: on, off with the fight's lapse",
        Scene::new(party(12.0)).run(&[2, 30, 104, 105], &[(5, ME, FOE)], &[(1, sound::PLAYING)]),
        &[true, true, true, false],
    );
    case(
        "a script's music stopped by a script",
        Scene::new(party(60.0)).run(&[2, 10], &[], &[(1, sound::PLAYING), (8, sound::STOPPED)]),
        &[true, false],
    );
    // Two seats: the first's 10 s run out while the second still fights.
    let two = vec![
        body(ME, 0.0, PLAYER),
        body(SECOND, 40.0, PLAYER),
        body(FOE, 10.0, HOSTILE_1),
        body(FOE + 10, 45.0, HOSTILE_1),
    ];
    case(
        "co-op: off with the last seat's lapse",
        Scene::new(two).run(
            &[3, 103, 149, 150],
            &[(2, ME, FOE), (50, SECOND, FOE + 10)],
            &[],
        ),
        &[true, true, true, false],
    );
    // The Sith Academy's duel (issue 322): a conversation starts the battle
    // music and others fight while it runs. It ends at tick 100, and the
    // music stops 10 s later (the kjk rule).
    case(
        "a conversation's battle music: off 10 s after it ends",
        Scene::new(party(8.0)).talking(1, 100).run(
            &[50, 150, 199, 200],
            &[(40, BYSTANDER, FOE), (41, FOE, BYSTANDER)],
            &[(2, sound::PLAYING)],
        ),
        &[true, true, true, false],
    );
    // A conversation that ends in a fight: the player's blows keep the
    // music, which then lapses 10 s after the fight's last blow.
    case(
        "a conversation's battle music: kept by the fight after it",
        Scene::new(party(12.0)).talking(1, 100).run(
            &[150, 199, 200, 249, 250],
            &[(150, ME, FOE)],
            &[(2, sound::PLAYING)],
        ),
        &[true, true, true, true, false],
    );
    // A conversation with no battle music ends, then a script starts the
    // battle music 1 s later while the enemies are still far away (the
    // reviewer's case). The music the conversation did not leave playing
    // plays on with no blow, as in KOTOR. So does music a script starts
    // in the very step the conversation ends.
    case(
        "a script's music after a conversation: on",
        Scene::new(party(60.0)).talking(1, 100).run(
            &[150, 250, 400],
            &[],
            &[(110, sound::PLAYING)],
        ),
        &[true, true, true],
    );
    case(
        "a script's music as a conversation ends: on",
        Scene::new(party(60.0)).talking(1, 100).run(
            &[150, 250, 400],
            &[],
            &[(100, sound::PLAYING)],
        ),
        &[true, true, true],
    );
    // A script's battle music started outside any conversation plays on
    // until a fight of the player's ends, as in KOTOR.
    case(
        "a script's music with no conversation: on",
        Scene::new(party(60.0)).run(&[2, 500], &[], &[(1, sound::PLAYING)]),
        &[true, true],
    );
    cases
}
