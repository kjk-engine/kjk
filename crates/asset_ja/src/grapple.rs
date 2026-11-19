//! A grapple finisher's pair lined up: where Jedi Academy's grabber touches
//! its victim, played on JA's own skeleton (`contacts`), how far apart two
//! KOTOR bodies stand so their hands come nearest those touches
//! (`fitted_gap`), and how near they come before and after the view pulls
//! the hands on (`errors`, with `rig::Touch`).
//!
//! The pair's space is the grabber's KOTOR model space: the grabber at the
//! origin facing +Y, the victim `gap` metres along +Y facing back (−Y), Z
//! up. JA's model space faces −Y, so a JA grabber's points turn a half turn
//! about Z into it, and a JA victim facing −Y already faces the grabber's
//! way back, so its points only move `gap` along +Y. A KOTOR victim faces
//! +Y in its own model space and turns a half turn.

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use fmt_ja::ghoul2::Gla;
use glam::{Quat, Vec3};

use crate::rig::{self, Options, Retargeter, Side, Touch};

/// JA's victim bones a grabber's hand may touch, and the KOTOR node that
/// follows each (`rig`'s mapping).
pub const VICTIM_BONES: [(&str, &str); 6] = [
    ("pelvis", "pelvis_g"),
    ("thoracic", "torsoupr_g"),
    ("cervical", "neck_g"),
    ("cranium", "head_g"),
    ("rtalus", "rfoot_g"),
    ("ltalus", "lfoot_g"),
];

/// JA's hand bones and KOTOR's, by side.
const HANDS: [(Side, &str, &str); 2] = [
    (Side::Right, "rhand", "rhand_g"),
    (Side::Left, "lhand", "lhand_g"),
];

/// A half turn about Z: JA's model space to KOTOR's.
const TURN: Quat = Quat::from_xyzw(0.0, 0.0, 1.0, 0.0);

/// One touch of JA's pair: from `from_s` to `to_s` seconds into both clips,
/// the grabber's hand on `side` stays near the victim's `bone` (a KOTOR
/// node, `VICTIM_BONES`), at `offsets` from it at each key (seconds, the
/// pair's space, JA metres): the hand's own path about the bone.
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    pub from_s: f32,
    pub to_s: f32,
    pub side: Side,
    pub bone: &'static str,
    pub offsets: Vec<(f32, Vec3)>,
}

impl Contact {
    /// The hand's offset from the bone at `time` (its nearest key's, the
    /// first or last outside the touch).
    pub fn offset_at(&self, time: f32) -> Vec3 {
        let index = self.offsets.partition_point(|(t, _)| *t < time);
        let before = index.checked_sub(1).and_then(|i| self.offsets.get(i));
        let after = self.offsets.get(index);
        match (before, after) {
            (Some(&(t0, a)), Some(&(t1, b))) if t1 > t0 => a.lerp(b, (time - t0) / (t1 - t0)),
            (_, Some(&(_, b))) => b,
            (Some(&(_, a)), None) => a,
            (None, None) => Vec3::ZERO,
        }
    }
}

/// Points sampled at times (seconds), each a list in a fixed order.
type Track = Vec<(f32, Vec<Vec3>)>;

/// The times (seconds) of `clip`'s keys.
fn key_times(clip: &AnimationIr) -> Vec<f32> {
    clip.tracks.first().map_or_else(Vec::new, |track| {
        track.orientations.iter().map(|k| k.0).collect()
    })
}

/// `track`'s points nearest `time` (the last after its end).
fn at(track: &Track, time: f32) -> Option<&[Vec3]> {
    let index = track.partition_point(|(t, _)| *t < time - 1e-4);
    track
        .get(index.min(track.len().saturating_sub(1)))
        .map(|(_, points)| points.as_slice())
}

/// JA's pair `gap` (metres) apart: the grabber's hands and the victim's
/// bones in the pair's space at each of the grabber clip's keys.
fn ja_pair(
    gla: &Gla,
    clips: (&AnimationIr, &AnimationIr),
    gap: f32,
) -> Vec<(f32, [Vec3; 2], Vec<Vec3>)> {
    let hands: Vec<&str> = HANDS.iter().map(|(_, ja, _)| *ja).collect();
    let bones: Vec<&str> = VICTIM_BONES.iter().map(|(ja, _)| *ja).collect();
    let grabber = rig::ja_points(gla, clips.0, &hands);
    let victim = rig::ja_points(gla, clips.1, &bones);
    // Past the victim's clip JA throws it off: no touch after.
    let end = victim.last().map_or(0.0, |(time, _)| *time);
    grabber
        .iter()
        .filter(|(time, _)| *time <= end)
        .filter_map(|(time, hands)| {
            let bones = at(&victim, *time)?;
            let hands = [
                TURN * hands.first()?.to_owned(),
                TURN * hands.get(1)?.to_owned(),
            ];
            let bones = bones.iter().map(|b| *b + Vec3::Y * gap).collect();
            Some((*time, hands, bones))
        })
        .collect()
}

/// The touches of JA's pair `gap` metres apart (JA's `idealDist`): for
/// each hand, each run of keys where it stays within `within` metres of one
/// of the victim's bones (`VICTIM_BONES`), runs less than `bridge` seconds
/// apart taken as one (the hand let go and taken back at once is held
/// through), on the bone it stays nearest over the run.
pub fn contacts(
    gla: &Gla,
    clips: (&AnimationIr, &AnimationIr),
    gap: f32,
    (within, bridge): (f32, f32),
) -> Vec<Contact> {
    let pair = ja_pair(gla, clips, gap);
    let mut found = Vec::new();
    for (hand, (side, ..)) in HANDS.iter().enumerate() {
        let near: Vec<bool> = pair
            .iter()
            .map(|(_, hands, bones)| bones.iter().any(|b| hands[hand].distance(*b) <= within))
            .collect();
        // Runs of near keys (first, last), bridged.
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for (key, &is) in near.iter().enumerate() {
            if !is {
                continue;
            }
            match runs.last_mut() {
                Some(run) if key == run.1 + 1 || pair[key].0 - pair[run.1].0 < bridge => {
                    run.1 = key
                }
                _ => runs.push((key, key)),
            }
        }
        for (first, last) in runs {
            let keys = &pair[first..=last];
            let mean = |bone: usize| {
                keys.iter()
                    .map(|(_, hands, bones)| hands[hand].distance(bones[bone]))
                    .sum::<f32>()
                    / keys.len() as f32
            };
            let Some(bone) = (0..VICTIM_BONES.len()).min_by(|a, b| mean(*a).total_cmp(&mean(*b)))
            else {
                continue;
            };
            found.push(Contact {
                from_s: keys[0].0,
                to_s: keys[keys.len() - 1].0,
                side: *side,
                bone: VICTIM_BONES[bone].1,
                offsets: keys
                    .iter()
                    .map(|(time, hands, bones)| (*time, hands[hand] - bones[bone]))
                    .collect(),
            });
        }
    }
    found.sort_by(|a, b| a.from_s.total_cmp(&b.from_s));
    found
}

/// Two KOTOR bodies playing a finisher, retargeted once: the grabber's
/// retargeter and JA clip (to pose it key by key, touched or not), and the
/// victim's nodes over its clip (its own model space).
pub struct Pair<'a> {
    grabber: Retargeter<'a>,
    grabber_clip: &'a AnimationIr,
    /// The grabber's hands (`HANDS`' order) at each key, as JA's pose puts
    /// them, unpulled.
    hands: Track,
    victim: Track,
    victim_scale: f32,
    /// How far (metres) the victim's pelvis stands to the grabber's right of
    /// the grabber's at each key of the victim's clip: on JA's pair, and
    /// on the KOTOR bodies.
    ja_sides: Vec<f32>,
    kotor_sides: Vec<f32>,
}

impl<'a> Pair<'a> {
    /// `grabber` playing `clips.0` and `victim` playing `clips.1`, both on
    /// JA's skeleton `gla` (its soles `ja_sole` metres up).
    pub fn new(
        (grabber, victim): (&'a ModelIr, &'a ModelIr),
        gla: &'a Gla,
        clips: (&'a AnimationIr, &'a AnimationIr),
        ja_sole: f32,
    ) -> Pair<'a> {
        let options = Options {
            ground: None,
            grips: Vec::new(),
            off_bolt: None,
            hold: Vec::new(),
            ja_sole,
        };
        let names: Vec<&str> = VICTIM_BONES.iter().map(|(_, kotor)| *kotor).collect();
        let retargeted = rig::retarget_with(victim, gla, clips.1, &options);
        let grabber = Retargeter::new(grabber, gla, &options);
        let hands: Vec<&str> = HANDS
            .iter()
            .map(|(.., kotor)| *kotor)
            .chain(["pelvis_g"])
            .collect();
        let hands: Track = key_times(clips.0)
            .into_iter()
            .enumerate()
            .map(|(key, time)| (time, grabber.touched(clips.0, key, &[], &hands)))
            .collect();
        let victim_track = rig::trace(victim, &retargeted, &names);
        let ja_pelvis = |clip| rig::ja_points(gla, clip, &["pelvis"]);
        let (ja_grabber, ja_victim) = (ja_pelvis(clips.0), ja_pelvis(clips.1));
        let ja_sides = ja_victim
            .iter()
            .filter_map(|(time, victim)| {
                let grabber = TURN * *at(&ja_grabber, *time)?.first()?;
                Some(victim.first()?.x - grabber.x)
            })
            .collect();
        let kotor_sides = victim_track
            .iter()
            .filter_map(|(time, victim)| {
                let grabber = *at(&hands, *time)?.get(2)?;
                Some((TURN * *victim.first()?).x - grabber.x)
            })
            .collect();
        Pair {
            ja_sides,
            kotor_sides,
            grabber,
            grabber_clip: clips.0,
            hands,
            victim: victim_track,
            victim_scale: Retargeter::new(victim, gla, &options).scale(),
        }
    }

    /// Where the victim's pelvis stands across the grabber's facing, at
    /// worst over the victim's clip (metres, + to the grabber's right): on
    /// JA's pair scaled to the bodies' size, and on the KOTOR bodies.
    pub fn sides(&self) -> (f32, f32) {
        let worst = |sides: &[f32]| {
            sides
                .iter()
                .copied()
                .fold(0.0f32, |a, b| if b.abs() > a.abs() { b } else { a })
        };
        (
            worst(&self.ja_sides) * self.scale(),
            worst(&self.kotor_sides),
        )
    }

    /// The mean of the two bodies' scales against JA's (`Retargeter::scale`).
    pub fn scale(&self) -> f32 {
        (self.grabber.scale() + self.victim_scale) / 2.0
    }

    /// The grabber clip's keys within `contact`: (key, seconds).
    fn keys(&self, contact: &Contact) -> Vec<(usize, f32)> {
        key_times(self.grabber_clip)
            .into_iter()
            .enumerate()
            .filter(|(_, t)| (contact.from_s..=contact.to_s).contains(t))
            .collect()
    }

    /// Where `contact` wants the grabber's hand at `time` with the victim
    /// `gap` metres away (the pair's space): the victim's node there, and
    /// JA's offset scaled by the bodies' size.
    pub fn target(&self, contact: &Contact, time: f32, gap: f32) -> Option<Vec3> {
        let bone = VICTIM_BONES
            .iter()
            .position(|(_, kotor)| *kotor == contact.bone)?;
        let node = *at(&self.victim, time)?.get(bone)?;
        Some(TURN * node + Vec3::Y * gap + contact.offset_at(time) * self.scale())
    }

    /// How far (metres) the grabber's hand stands from where each of
    /// `contacts` wants it, at worst over the touch's keys, the victim
    /// `gap` metres away: without the view's pull (`touched` false), or
    /// with every hand pulled whole onto its place (`rig::Touch`).
    pub fn errors(&self, contacts: &[Contact], gap: f32, touched: bool) -> Vec<f32> {
        contacts
            .iter()
            .map(|contact| {
                let side = HANDS.iter().position(|(side, ..)| *side == contact.side);
                let (side, hand) = side.map_or((0, "rhand_g"), |at| (at, HANDS[at].2));
                self.keys(contact)
                    .into_iter()
                    .filter_map(|(key, time)| {
                        let target = self.target(contact, time, gap)?;
                        let at = if touched {
                            let touch = Touch {
                                side: contact.side,
                                target,
                                weight: 1.0,
                            };
                            *self
                                .grabber
                                .touched(self.grabber_clip, key, &[touch], &[hand])
                                .first()?
                        } else {
                            *self.hands.get(key)?.1.get(side)?
                        };
                        Some(at.distance(target))
                    })
                    .fold(0.0, f32::max)
            })
            .collect()
    }

    /// The gap (metres) at which the grabber's hands come nearest JA's
    /// touches: searched `search` metres either side of `ja_gap` (JA's
    /// `idealDist`) times the bodies' scale, every centimetre. Nearest is
    /// first by the touches' worst error once the view pulls the hands on
    /// (where a KOTOR arm, shorter for its body than JA's, falls short), then
    /// by their worst error without the pull: the mean over the touches of
    /// each, the first counted four times over.
    pub fn fitted_gap(&self, contacts: &[Contact], ja_gap: f32, search: f32) -> f32 {
        let start = ja_gap * self.scale();
        let steps = (search * 100.0).round() as i32;
        let mean = |errors: Vec<f32>| errors.iter().sum::<f32>() / errors.len().max(1) as f32;
        let cost = |gap: f32| {
            4.0 * mean(self.errors(contacts, gap, true)) + mean(self.errors(contacts, gap, false))
        };
        (-steps..=steps)
            .map(|step| start + step as f32 / 100.0)
            .map(|gap| (gap, cost(gap)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(start, |(gap, _)| gap)
    }
}
