//! The probes of a player's bloom and kick (`kjk sim probes --only bloom`,
//! `combat::bloom`), no install:
//! - the rifle's bloom grows by its step, stops at its most, holds while
//!   resting and falls away after;
//! - stance multiplies the spread;
//! - the kick stops at its limit, holds, then settles back;
//! - shots stray evenly inside the disc, never beyond it;
//! - putting the gun away clears bloom and kick;
//! - shooting state round-trips, and older snapshots read no bloom.

use crate::combat::bloom::{Stance, Tune, cone, disc, kick_at, on_shot, settled, tune};
use crate::combat::sheet::Blaster;
use crate::combat::specials::Shooting;
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::wire::{Reader, Versions, Writer};

const EPS: f32 = 1e-3;

pub fn probes() -> Checks {
    vec![
        ("bloom_follows_shots", follows_shots()),
        ("cone_multiplies_stance", multiplies_stance()),
        ("kick_caps_and_recovers", kick_caps()),
        ("disc_stays_inside", stays_inside()),
        ("interrupt_clears_bloom", interrupt_clears()),
        ("shooting_round_trips", round_trips()),
        ("old_snapshots_read_no_bloom", old_snapshots()),
    ]
}

fn fired(blaster: Blaster, shots: &[u64]) -> (Shooting, Tune) {
    let tune = tune(blaster);
    let mut shooting = Shooting::default();
    for &at in shots {
        on_shot(&mut shooting, &tune, at, 0.0);
    }
    (shooting, tune)
}

fn follows_shots() -> Result<String, String> {
    let twenty: Vec<u64> = (0..20).map(|n| 1000 + n * 100).collect();
    let rows: [(&str, &[u64], u64, f32); 4] = [
        ("one shot", &[1000], 1000, 0.3),
        ("twenty shots cap", &twenty, 2900, 3.0),
        ("still resting", &[1000], 1450, 0.3),
        ("rested away", &[1000], 1500, 0.0),
    ];
    let seen: Vec<(&str, f32, f32)> = rows
        .iter()
        .map(|&(name, shots, at, expected)| {
            let (shooting, tune) = fired(Blaster::Rifle, shots);
            (name, settled(&shooting, &tune, at), expected)
        })
        .collect();
    let ok = seen.iter().all(|(_, got, want)| (got - want).abs() < EPS);
    check(ok, format!("rifle bloom (got, want): {seen:?}"))
}

fn multiplies_stance() -> Result<String, String> {
    let both = Stance {
        moving: true,
        airborne: true,
        aiming: false,
    };
    let (wide, still) = (cone(1.4, 0.6, both), cone(1.4, 0.6, Stance::default()));
    let ok = (wide - 7.5).abs() < EPS && (still - 2.0).abs() < EPS;
    check(
        ok,
        format!("moving in the air {wide} (7.5), still {still} (2.0)"),
    )
}

fn kick_caps() -> Result<String, String> {
    let shots: Vec<u64> = (0..20).map(|n| 1000 + n * 10).collect();
    let (shooting, _) = fired(Blaster::Bowcaster, &shots);
    let seen: Vec<(u64, f32, f32)> = [(0, -4.0), (80, -4.0), (413, 0.0)]
        .iter()
        .map(|&(after, want)| (after, kick_at(&shooting, 1190 + after)[0], want))
        .collect();
    let ok = seen.iter().all(|(_, got, want)| (got - want).abs() < 0.01);
    check(
        ok,
        format!("bowcaster pitch kick (ms after, got, want): {seen:?}"),
    )
}

fn stays_inside() -> Result<String, String> {
    let mut rng = SimRng::new(7, 9);
    let points: Vec<[f32; 2]> = (0..10_000).map(|_| disc(&mut rng, 2.0)).collect();
    let farthest = points
        .iter()
        .map(|[x, y]| (x * x + y * y).sqrt())
        .fold(0.0f32, f32::max);
    let mean = points
        .iter()
        .fold([0.0f32; 2], |sum, [x, y]| [sum[0] + x, sum[1] + y])
        .map(|sum| sum / points.len() as f32);
    let ok = farthest <= 2.0 + EPS && mean.iter().all(|m| m.abs() < 0.05);
    check(ok, format!("radius 2: farthest {farthest}, mean {mean:?}"))
}

fn interrupt_clears() -> Result<String, String> {
    let (mut shooting, tune) = fired(Blaster::Rifle, &[1000, 1100, 1200]);
    shooting.interrupt();
    let (bloom, kick) = (settled(&shooting, &tune, 1200), kick_at(&shooting, 1200));
    check(
        bloom == 0.0 && kick == [0.0, 0.0],
        format!("after interrupt: bloom {bloom}, kick {kick:?}"),
    )
}

fn sample() -> Shooting {
    Shooting {
        bloom_mdeg: 1234,
        kick_mdeg: [-2500, 300],
        fired_at: 98_765,
        ..Shooting::default()
    }
}

fn round_trips() -> Result<String, String> {
    let mut out = Writer::default();
    sample().encode(&mut out);
    let read = Shooting::decode(&mut Reader::new("probe", &out.0));
    check(read.as_ref() == Ok(&sample()), format!("{read:?}"))
}

fn old_snapshots() -> Result<String, String> {
    let mut out = Writer::default();
    sample().encode(&mut out);
    let versions = Versions {
        snapshot: crate::since::snapshot::FEAT_DYNAMIC_CROSSHAIR.1 - 1,
        ..Versions::THIS_BUILD
    };
    // The bytes an older build wrote: no bloom, kick or shot time.
    let cut = out.0.len() - 2 - 4 - 8;
    let mut input = Reader::new("probe", &out.0[..cut]).with_versions(versions);
    let read = Shooting::decode(&mut input).map(|s| (s.bloom_mdeg, s.kick_mdeg, s.fired_at));
    let ok = read == Ok((0, [0, 0], 0)) && input.finish().is_ok();
    check(ok, format!("older snapshot reads {read:?}"))
}
