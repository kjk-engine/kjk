//! `kjk net bandwidth`: what a host's snapshots cost a client, from one of
//! its recordings (`inputs.kjkr`) replayed: every `every`th tick's snapshot
//! encoded against the one `back` ticks before it (the client's last
//! confirmation, about a round trip old), as `net::codec` sends it. The
//! cost is split by what changed: the head and tail, the entities that
//! come and go, and each changed record's field groups, a group's share
//! being the delta of the baseline's record with only that group changed
//! (`--sample`: every so many sent snapshots); the frames as sent are
//! packed (`codec::pack`), the split is of the delta unpacked. Writes
//! `facts.json`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use net::codec;
use sim::combat::Fighter;
use sim::{EntityState, Kind, Parts, Recording, SimId, Snapshot, Stash};

use crate::sim_verbs::Installers;

/// Bytes a snapshot frame carries besides its delta: the packet header
/// and the frame header (`FrameHeader`, its hash).
const FRAME_OVERHEAD: usize = 19 + 16;

/// A record's field groups: each puts the current state's group onto the
/// baseline's entity.
type Group = (&'static str, fn(&mut EntityState, &EntityState));

fn fighter(apply: fn(&mut Fighter, &Fighter)) -> impl Fn(&mut EntityState, &EntityState) {
    move |base, now| {
        if let (Some(b), Some(n)) = (base.fighter.as_mut(), now.fighter.as_ref()) {
            apply(b, n);
        }
    }
}

const GROUPS: [Group; 15] = [
    ("place (position, velocity)", |b, n| {
        b.position = n.position;
        b.velocity = n.velocity;
    }),
    ("player state", |b, n| b.player = n.player),
    ("fighter: hp, fp, life", |b, n| {
        fighter(|b, n| {
            b.hp = n.hp;
            b.fp = n.fp;
            b.life = n.life;
        })(b, n)
    }),
    ("fighter: timers", |b, n| {
        fighter(|b, n| {
            b.weapon_wait_ms = n.weapon_wait_ms;
            b.combat_until = n.combat_until;
            b.pain_until = n.pain_until;
        })(b, n)
    }),
    ("fighter: last command", |b, n| {
        fighter(|b, n| b.last_command = n.last_command)(b, n)
    }),
    ("fighter: events", |b, n| {
        fighter(|b, n| {
            b.events = n.events;
            b.next_event = n.next_event;
        })(b, n)
    }),
    ("fighter: saber", |b, n| {
        fighter(|b, n| b.saber = n.saber)(b, n)
    }),
    ("fighter: force", |b, n| {
        fighter(|b, n| b.force = n.force)(b, n)
    }),
    ("fighter: mind", |b, n| {
        fighter(|b, n| b.mind = n.mind)(b, n)
    }),
    ("fighter: pose", |b, n| {
        fighter(|b, n| b.pose = n.pose)(b, n)
    }),
    ("fighter: trails", |b, n| {
        fighter(|b, n| b.trails = n.trails)(b, n)
    }),
    ("fighter: swing", |b, n| {
        fighter(|b, n| b.swing = n.swing)(b, n)
    }),
    ("fighter: grace", |b, n| {
        fighter(|b, n| b.grace = n.grace)(b, n)
    }),
    ("fighter: the rest", |b, n| {
        fighter(|b, n| {
            b.hand = n.hand;
            b.last_attacker = n.last_attacker;
            b.faction = n.faction;
            b.target = n.target;
            b.shooting = n.shooting;
            b.pack = n.pack;
            b.effects = n.effects;
        })(b, n)
    }),
    ("missile, object, sheet, the rest", |b, n| {
        b.missile = n.missile;
        b.object = n.object;
        b.sheet = n.sheet;
        b.life_ms = n.life_ms;
        b.going = n.going;
        b.retarget_ms = n.retarget_ms;
    }),
];

/// The delta's bytes for one record changed from `base` to `now`.
fn record_cost(base: &[u8], now: &[u8]) -> usize {
    let parts = |record: &[u8]| Parts {
        head: Vec::new(),
        entities: vec![(SimId(1), record.to_vec())],
        tail: Vec::new(),
        stash: Stash::default(),
    };
    let mut out = Vec::new();
    codec::encode(Some(&parts(base)), &parts(now), &mut out);
    let mut same = Vec::new();
    codec::encode(Some(&parts(base)), &parts(base), &mut same);
    out.len().saturating_sub(same.len())
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Avatar(_) => "players",
        Kind::Object(sim::module::ObjectKind::Creature, _) => "creatures",
        Kind::Object(..) => "objects (doors, placeables, …)",
        Kind::Missile => "missiles",
        Kind::Wanderer => "wanderers",
    }
}

pub fn bandwidth(
    discovery: &vfs::Discovery,
    file: &Path,
    (every, backs, sample): (u64, &[u64], u64),
) -> ExitCode {
    let recording = match std::fs::read(file)
        .map_err(|error| error.to_string())
        .and_then(|bytes| Recording::decode(&bytes).map_err(|error| error.to_string()))
    {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let mut installers = Installers::default();
    let level = match installers.level_named(
        discovery,
        &recording.level,
        recording.start.place.as_ref(),
        recording.start.seats(),
    ) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let mut snapshots: Vec<Snapshot> = Vec::new();
    let played = recording.play_watched(
        &level,
        &mut |announcement| {
            installers
                .get(discovery, &announcement.game)?
                .level_for(announcement)
        },
        &mut |snapshot| snapshots.push(snapshot.clone()),
    );
    if let Err(divergence) = played {
        eprintln!(
            "{}: the replay diverged at tick {}",
            file.display(),
            divergence.tick
        );
        return ExitCode::FAILURE;
    }
    let parts: Vec<Parts> = snapshots.iter().map(Snapshot::split).collect();
    let seconds = snapshots
        .last()
        .zip(snapshots.first())
        .map_or(1.0, |(last, first)| {
            (last.time_ms - first.time_ms).max(1) as f64 / 1000.0
        });
    let entities = snapshots.last().map_or(0, |s| s.entities.len());
    outln!(
        "{}: {} snapshots ({seconds:.1} s), {entities} entities at the end; a snapshot every {every} ticks",
        file.display(),
        snapshots.len()
    );
    let mut rows = Vec::new();
    for &back in backs {
        let mut total = 0usize;
        let mut raw = 0usize;
        let mut sent = 0usize;
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_group: BTreeMap<&str, usize> = BTreeMap::new();
        let mut sampled = 0usize;
        let mut head_tail = 0usize;
        for at in (back as usize..parts.len()).step_by(every.max(1) as usize) {
            // A baseline 0 ticks back is none: a keyframe.
            let from = (back > 0).then(|| at - back as usize);
            let mut out = Vec::new();
            codec::encode(from.map(|from| &parts[from]), &parts[at], &mut out);
            raw += out.len() + FRAME_OVERHEAD;
            total += codec::pack(&out).len() + FRAME_OVERHEAD;
            sent += 1;
            if !(sent as u64).is_multiple_of(sample.max(1)) {
                continue;
            }
            sampled += 1;
            let old = from.map(|from| &snapshots[from]);
            let mut records = 0usize;
            for entity in &snapshots[at].entities {
                let record = Snapshot::record(entity);
                let held = old.and_then(|old| {
                    let i = old
                        .entities
                        .binary_search_by_key(&entity.id, |e| e.id)
                        .ok()?;
                    Some(&old.entities[i])
                });
                let cost = match held {
                    Some(&held) => {
                        let was = Snapshot::record(&held);
                        if was == record {
                            continue;
                        }
                        for (name, apply) in GROUPS {
                            let mut hybrid = held;
                            apply(&mut hybrid, entity);
                            let cost = record_cost(&was, &Snapshot::record(&hybrid));
                            *by_group.entry(name).or_default() += cost;
                        }
                        record_cost(&was, &record)
                    }
                    None => {
                        *by_group.entry("records new to the client").or_default() += record.len();
                        record.len()
                    }
                };
                records += cost;
                *by_kind.entry(kind_name(entity.kind)).or_default() += cost;
            }
            head_tail += out.len().saturating_sub(records);
        }
        let rate = |bytes: usize, of: usize| {
            bytes as f64 * sent as f64 / of.max(1) as f64 / seconds / 1024.0
        };
        outln!(
            "  baseline {back:>2} ticks back ({} ms): {:.1} KB/s down, {:.0} bytes a snapshot{}",
            back * 8,
            total as f64 / seconds / 1024.0,
            total as f64 / sent.max(1) as f64,
            if back == 0 { " (keyframes)" } else { "" }
        );
        outln!(
            "    the delta unpacked: {:.1} KB/s",
            raw as f64 / seconds / 1024.0
        );
        outln!("    by kind, unpacked (sampled {sampled} snapshots):");
        let mut kinds: Vec<_> = by_kind.iter().collect();
        kinds.sort_by_key(|(_, bytes)| std::cmp::Reverse(**bytes));
        for (kind, bytes) in kinds {
            outln!("      {:>6.1} KB/s  {kind}", rate(*bytes, sampled));
        }
        outln!(
            "      {:>6.1} KB/s  head, tail and the delta's framing",
            rate(head_tail, sampled)
        );
        outln!("    by field group, unpacked (each alone on the baseline's record):");
        let mut groups: Vec<_> = by_group.iter().collect();
        groups.sort_by_key(|(_, bytes)| std::cmp::Reverse(**bytes));
        for (group, bytes) in groups {
            outln!("      {:>6.1} KB/s  {group}", rate(*bytes, sampled));
        }
        rows.push(serde_json::json!({
            "back": back, "every": every, "kb_per_s": total as f64 / seconds / 1024.0,
            "unpacked_kb_per_s": raw as f64 / seconds / 1024.0,
            "bytes_per_snapshot": total as f64 / sent.max(1) as f64,
            "by_kind": by_kind.iter().map(|(k, b)| (k.to_string(), rate(*b, sampled))).collect::<BTreeMap<_, _>>(),
            "by_group": by_group.iter().map(|(k, b)| (k.to_string(), rate(*b, sampled))).collect::<BTreeMap<_, _>>(),
        }));
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "net bandwidth",
        "file": file.display().to_string(),
        "seconds": seconds,
        "rows": rows,
    }));
    ExitCode::SUCCESS
}
