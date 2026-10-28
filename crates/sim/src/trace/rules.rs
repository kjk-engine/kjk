//! `m4` and `m5`: the saber rules' pure functions (clashes, power,
//! defence, locks, kicks, the look, the sweep's geometry) and the blade
//! wielder's pacing, asked with inputs made from a seed. Each case's record
//! carries its inputs as well as its outputs, so a spec reads what was
//! asked.
//!
//! Directives: `case KIND SEED` (one case of `KIND` from `SEED`), `cases
//! KIND COUNT SEED` (as many in a row), and for `m5` `duel SEED THINKS
//! [key=value ...]` (a blade wielder's pace over that many thinks against
//! a foe made up each think, but for the `Sight` fields the keys hold
//! think after think; `approach=X` moves the gap by X a think, `step=MS`
//! spaces the thinks, `mask=N` holds the clear sides, `scramble=1` starts
//! the pace with every field made up, `sides=1` gives `finish` keys of any
//! size, walking and attacking too; `ally=flip` changes the foe's side every
//! five thinks; `aggression=N rated=N roam=MS` begin the pace so).
//! `KIND` is one of `KINDS`, or `any` (a kind drawn per case).

use collision::{Body, Contents, UNIT};

use super::movement::{Gen, blocked_all};
use super::record::{Record, Trace};
use super::scenario::{Scenario, float, int, pairs};
use crate::SimRng;
use crate::combat::pace::{self, Arms as FoeArms, Dice, Keys, Pace, Sight};
use crate::combat::saber::throw::{Flight, Thrown};
use crate::combat::saber::{
    Block, Change, Getup, Hand, Lock, Move, SaberState, Style, clash, defence, kick, lock, look,
    power, sweep,
};
use crate::math::Vec3;
use crate::motion::{Anim, AnimInfo, AnimTable, Ground, MotionState, Part};
use crate::skeleton::BoltMatrix;
use crate::snapshot::SimId;
use crate::wire::{Reader, Writer};

/// The kinds of case `m4` asks.
pub const KINDS: &[&str] = &[
    "irand",
    "broken_attack",
    "deflect",
    "resolve",
    "disarm",
    "attack_anim",
    "power",
    "animation_power",
    "can_block",
    "in_arc",
    "parry_for",
    "bolt",
    "can_lock",
    "pick",
    "lock_anims",
    "opening",
    "lock_frames",
    "press_hits",
    "breaks",
    "lock_rolls",
    "shove",
    "kick",
    "floors",
    "facing",
    "segments",
    "sweep_quad",
    "enters",
    "swing",
    "lock_codec",
];

fn styles() -> [Style; 5] {
    [
        Style::Fast,
        Style::Medium,
        Style::Strong,
        Style::Dual,
        Style::Staff,
    ]
}

/// What a case is made from: the generator, the sim stream the rule
/// draws from, and an animation table with lengths of its own.
struct Maker {
    g: Gen,
    rng: SimRng,
    anims: &'static AnimTable,
}

/// The tables a case picks from: every animation 20 frames, each its own
/// length, or each its own and long (1.5 to 9 s); made once.
fn tables() -> &'static [AnimTable; 3] {
    static TABLES: std::sync::OnceLock<[AnimTable; 3]> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let table = |kind: u8| {
            AnimTable::from_lookup(|name| {
                let h = blake3::hash(name.as_bytes());
                let b = h.as_bytes();
                Some(AnimInfo {
                    first_frame: 1 + i32::from(b[0]) * 4,
                    frames: match kind {
                        0 => 20,
                        1 => 2 + i32::from(b[1] % 60),
                        _ => 30 + i32::from(b[1] % 61),
                    },
                    frame_lerp: if kind == 2 {
                        50 + i32::from(b[2] % 50)
                    } else {
                        20 + i32::from(b[2] % 80)
                    },
                    feet: Some([[0.0, 6.0], [0.0, -6.0]]),
                    looping: false,
                })
            })
        };
        [table(0), table(1), table(2)]
    })
}

impl Maker {
    fn new(seed: u64) -> Maker {
        let mut g = Gen::new(seed, 0x6d34);
        let rng = SimRng::new(u64::from(g.u32()), u64::from(g.u32()));
        let anims = &tables()[match g.below(4) {
            0 => 0,
            1 | 2 => 1,
            _ => 2,
        }];
        Maker { g, rng, anims }
    }

    fn style(&mut self) -> Style {
        self.g.pick(&styles())
    }

    fn saber_move(&mut self) -> Move {
        Move::ALL[self.g.below(Move::ALL.len() as u32) as usize]
    }

    /// An animation: any, or a saber move's in some style's group, or a
    /// lock's, a kick's, a kata's.
    fn anim(&mut self) -> Anim {
        match self.g.below(4) {
            0 => Anim::ALL[self.g.below(Anim::ALL.len() as u32) as usize],
            1 | 2 => {
                let base = self.saber_move().data().anim;
                let group = self.g.below(5) as u16;
                Anim::from_index(base.index() + group * 77).unwrap_or(base)
            }
            _ => {
                let names = [
                    "BOTH_BF2LOCK",
                    "BOTH_BF1LOCK",
                    "BOTH_CWCIRCLELOCK",
                    "BOTH_CCWCIRCLELOCK",
                    "BOTH_LK_S_S_T_L_2",
                    "BOTH_LK_DL_DL_S_L_1",
                    "BOTH_LK_ST_ST_T_L_1",
                    "BOTH_LK_S_DL_S_B_1_L",
                    "BOTH_A7_KICK_F",
                    "BOTH_A7_KICK_B",
                    "BOTH_A7_KICK_R",
                    "BOTH_A7_KICK_L",
                    "BOTH_A7_KICK_S",
                    "BOTH_A7_KICK_BF",
                    "BOTH_A7_KICK_RL",
                    "BOTH_A7_KICK_F_AIR",
                    "BOTH_A7_SOULCAL",
                    "BOTH_A6_SABERPROTECT",
                    "BOTH_A1_SPECIAL",
                    "BOTH_A2_SPECIAL",
                    "BOTH_A3_SPECIAL",
                    "BOTH_GETUP_BROLL_F",
                    "BOTH_FORCEWALLRUNFLIP_START",
                    "BOTH_ROLL_F",
                    "BOTH_KNOCKDOWN1",
                    "BOTH_JUMPATTACK6",
                    "BOTH_JUMPATTACK7",
                    "BOTH_A2_STABBACK1",
                    "BOTH_ATTACK_BACK",
                    "BOTH_CROUCHATTACKBACK1",
                    "BOTH_LUNGE2_B__T_",
                    "BOTH_FORCELEAP2_T__B_",
                    "BOTH_JUMPFLIPSLASHDOWN1",
                    "BOTH_JUMPFLIPSTABDOWN",
                    "BOTH_SPINATTACK6",
                    "BOTH_BUTTERFLY_LEFT",
                    "BOTH_BF1BREAK",
                    "BOTH_CWCIRCLEBREAK",
                    "BOTH_FLIP_F",
                    "BOTH_GETUP_BROLL_B",
                ];
                let name = self.g.pick(&names);
                Anim::from_name(name).unwrap_or(Anim::BOTH_STAND1)
            }
        }
    }

    fn vec(&mut self, reach: f32) -> Vec3 {
        [
            self.g.range(-reach, reach),
            self.g.range(-reach, reach),
            self.g.range(-reach, reach),
        ]
    }

    fn part(&mut self) -> Part {
        Part {
            anim: self.anim(),
            timer: self.g.pick(&[0, 0, 1, 8, 50, 150, 400, 1000, 3000]),
            flip: self.g.chance(0.5),
        }
    }

    fn player(&mut self) -> MotionState {
        let mut ps = MotionState::standing(self.vec(80.0), self.g.range(-180.0, 180.0));
        ps.origin[2] = ps.origin[2].abs() * 0.5;
        ps.view[0] = self.g.range(-70.0, 70.0);
        if self.g.chance(0.2) {
            ps.view[2] = self.g.range(-30.0, 30.0);
        }
        if self.g.chance(0.6) {
            ps.velocity = self.vec(300.0);
        }
        ps.legs = self.part();
        ps.torso = if self.g.chance(0.4) {
            ps.legs
        } else {
            self.part()
        };
        ps.flags = (self.g.u32() & 0x7ff) as u16 & if self.g.chance(0.5) { 0 } else { 0x7ff };
        ps.ground = match self.g.below(4) {
            0 => None,
            3 => Some(Ground::Entity(self.g.below(5) | crate::BODY)),
            _ => Some(Ground::World),
        };
        ps
    }

    fn saber(&mut self) -> SaberState {
        let mut s = SaberState {
            on: self.g.chance(0.85),
            style: Some(self.style()),
            current_move: self.saber_move(),
            blocking: self.g.pick(&[Block::No, Block::Tight, Block::Wide]),
            blocked: blocked_all()[self.g.below(14) as usize],
            chain: self.g.below(6) as u8,
            ..SaberState::default()
        };
        if self.g.chance(0.15) {
            s.change = self.g.pick(&[Change::Dropping, Change::Raising]);
        }
        if self.g.chance(0.1) {
            s.hand = self.g.pick(&[Hand::Knockdown, Hand::Risen]);
            s.hand_until = u64::from(self.g.below(5000));
            s.getup = self
                .g
                .pick(&[Getup::Down, Getup::Plain, Getup::Force, Getup::Quick]);
        }
        if self.g.chance(0.1) {
            s.lock_until = u64::from(self.g.below(5000));
        }
        s.bolt_block_until = u64::from(self.g.below(3000));
        if self.g.chance(0.05) {
            s.thrown = Some(Thrown {
                flight: Flight::Out,
                at: self.vec(100.0),
                velocity: self.vec(400.0),
                since: 0,
                aim_at: 0,
                hit_until: 0,
                level: 1,
            });
            // Thrown from ready as often as not (the guard reads it so).
            if self.g.chance(0.5) {
                s.current_move = Move::READY;
            }
        }
        s
    }

    fn fencer(&mut self) -> clash::Fencer {
        let saber = self.saber();
        clash::Fencer {
            saber,
            ps: self.player(),
            style: saber.style.unwrap_or(Style::Medium),
            offence: self.g.int(0, 5) as i32,
            defence: {
                let most = if self.g.chance(0.2) { 6 } else { 3 };
                self.g.int(0, most) as i32
            },
            power: self.g.int(-1, 6) as i32,
            attacking: self.g.chance(0.5),
            anim_power: self.g.int(-2, 8) as i32,
            weapon_wait_ms: self.g.pick(&[-50, 0, 0, 1, 100, 500]),
        }
    }

    fn now(&mut self) -> u64 {
        u64::from(self.g.below(6000))
    }
}

fn rng_hex(record: &mut Record, key: &str, rng: &SimRng) {
    let mut state = Writer::default();
    rng.encode(&mut state);
    record.hex(key, &state.0);
}

/// A player state's fields, keys prefixed by `who`.
fn ps_fields(record: &mut Record, who: &str, ps: &MotionState) {
    let k = |name: &str| format!("{who}.{name}");
    record
        .v3(&k("o"), ps.origin)
        .v3(&k("v"), ps.velocity)
        .v3(&k("view"), ps.view)
        .s(&k("legs"), ps.legs.anim.name())
        .i(&k("lt"), ps.legs.timer)
        .b(&k("lf"), ps.legs.flip)
        .s(&k("torso"), ps.torso.anim.name())
        .i(&k("tt"), ps.torso.timer)
        .b(&k("tf"), ps.torso.flip)
        .i(&k("fl"), ps.flags)
        .d(&k("gr"), &ps.ground)
        .f(&k("g"), ps.gravity);
}

fn saber_fields(record: &mut Record, who: &str, s: &SaberState) {
    let k = |name: &str| format!("{who}.{name}");
    record
        .b(&k("on"), s.on)
        .s(&k("sty"), s.style.map_or("-", Style::name))
        .s(&k("mv"), s.current_move.name())
        .d(&k("blk"), &s.blocking)
        .d(&k("bkd"), &s.blocked)
        .i(&k("chn"), s.chain)
        .d(&k("chg"), &s.change)
        .d(&k("hnd"), &s.hand)
        .i(&k("hu"), s.hand_until)
        .d(&k("gu"), &s.getup)
        .i(&k("lku"), s.lock_until)
        .i(&k("bbu"), s.bolt_block_until)
        .b(&k("thr"), s.thrown.is_some());
}

fn fencer_fields(record: &mut Record, who: &str, f: &clash::Fencer) {
    saber_fields(record, who, &f.saber);
    ps_fields(record, who, &f.ps);
    let k = |name: &str| format!("{who}.{name}");
    record
        .s(&k("style"), f.style.name())
        .i(&k("off"), f.offence)
        .i(&k("def"), f.defence)
        .i(&k("pow"), f.power)
        .b(&k("att"), f.attacking)
        .i(&k("ap"), f.anim_power)
        .i(&k("wms"), f.weapon_wait_ms);
}

/// One case of `kind` from `seed`, recorded.
fn case(kind: &str, seed: u64, step: u64, trace: &mut Trace) -> Result<(), String> {
    let mut m = Maker::new(seed);
    let kind = if kind == "any" { m.g.pick(KINDS) } else { kind };
    let mut r = Record::new(step, kind);
    r.i("seed", seed);
    rng_hex(&mut r, "rng", &m.rng);
    match kind {
        "irand" => {
            let (low, high) = (m.g.int(-5, 20) as i32, m.g.int(-6, 20) as i32);
            let got = clash::roll_between(&mut m.rng, low, high);
            r.i("low", low).i("high", high).i("got", got);
        }
        "broken_attack" => {
            let mv = m.saber_move();
            r.s("mv", mv.name())
                .s("got", clash::broken_attack(mv).name());
        }
        "deflect" | "resolve" => {
            let (mut a, mut d) = (m.fencer(), m.fencer());
            if m.g.chance(0.5) {
                // An attack against a parry or an attack: the tree's
                // common case.
                let attacks = [Move::A_TL2BR, Move::A_T2B, Move::A_L2R, Move::A_BR2TL];
                a.saber.current_move = m.g.pick(&attacks);
                d.saber.current_move = m.g.pick(&[
                    Move::READY,
                    Move::PARRY_UP,
                    Move::A_R2L,
                    Move::S_T2B,
                    Move::R_BL2TR,
                    Move::T1_BR_TL,
                ]);
            }
            if m.g.chance(0.15) {
                // A strong reflect or special against a strong parry held
                // by a better defence: the tree's sixth step refused late.
                let odd: Vec<Move> = Move::ALL
                    .iter()
                    .copied()
                    .filter(|mv| mv.reflect() || mv.special())
                    .collect();
                a.saber.current_move = odd[m.g.below(odd.len() as u32) as usize];
                a.power = m.g.int(3, 5) as i32;
                d.saber.current_move = Move::PARRY_UP;
                d.power = m.g.int(3, 6) as i32;
                d.defence = 6;
            }
            fencer_fields(&mut r, "a", &a);
            fencer_fields(&mut r, "d", &d);
            if kind == "deflect" {
                let got = clash::deflect(&mut a, &d, &mut m.rng);
                r.b("got", got);
            } else {
                let point = m.vec(40.0);
                let met = m.g.chance(0.6);
                let got = clash::resolve(&mut a, &mut d, point, met, &mut m.rng);
                r.v3("point", point)
                    .b("met", met)
                    .b("disarm_a", got.disarm_a)
                    .b("disarm_d", got.disarm_d);
                fencer_fields(&mut r, "D", &d);
            }
            fencer_fields(&mut r, "A", &a);
        }
        "disarm" => {
            let (loser, winner) = (m.style(), m.style());
            r.s("loser", loser.name())
                .s("winner", winner.name())
                .b("got", clash::disarm(loser, winner, &mut m.rng));
        }
        "attack_anim" => {
            let (torso, base) = (m.anim(), m.saber_move().data().anim);
            r.s("torso", torso.name())
                .s("base", base.name())
                .b("got", clash::attack_anim(torso, base));
        }
        "power" => {
            let (style, mv) = (m.style(), m.saber_move());
            let blocked = blocked_all()[m.g.below(14) as usize];
            let weapon_wait_ms = m.g.pick(&[-1, 0, 1, 300]);
            let attacking = m.g.chance(0.5);
            let hilt = m.g.chance(0.3).then(|| m.g.range(0.0, 40.0));
            let anim = m.anim();
            r.s("style", style.name())
                .s("mv", mv.name())
                .d("bkd", &blocked)
                .i("wms", weapon_wait_ms)
                .b("att", attacking)
                .d("hilt", &hilt.map(f32::to_bits))
                .s("anim", anim.name())
                .i("style_level", power::style_level(style))
                .b("attacking", power::attacking(mv, blocked, weapon_wait_ms))
                .b("deflecting", power::deflecting(mv))
                .i("attack", power::attack(style, attacking, hilt))
                .i("style_power", power::style_power(style))
                .b("sb_win", power::final_break_won(anim))
                .b("sb", power::final_break(anim))
                .b("sb_loss", power::final_break_lost(anim))
                .i("kata", power::kata_bonus(anim))
                .i("ja_frame_ms", power::JA_FRAME_MS);
        }
        "animation_power" => {
            let torso = m.anim();
            let length = m.anims.get(torso).length_ms().max(1);
            let timer = if m.g.chance(0.5) {
                m.g.pick(&[0, 1, 50, 200, 500, 999, 2000])
            } else {
                m.g.int(0, i64::from(length)) as i32
            };
            let defending = m.g.chance(0.5);
            r.s("torso", torso.name())
                .i("timer", timer)
                .b("defending", defending)
                .i("got", power::animation(m.anims, torso, timer, defending));
        }
        "can_block" | "in_arc" | "parry_for" => {
            let (mut saber, mut ps) = (m.saber(), m.player());
            let attacking = m.g.chance(0.25);
            if attacking {
                // In a special, out of a saber animation: busy by it.
                saber.on = true;
                saber.thrown = None;
                saber.change = Change::None;
                saber.blocked = crate::combat::saber::Blocked::None;
                let specials: Vec<Move> = Move::ALL
                    .iter()
                    .copied()
                    .filter(|mv| mv.special())
                    .collect();
                saber.current_move = specials[m.g.below(specials.len() as u32) as usize];
                ps.torso.anim = Anim::BOTH_STAND1;
            }
            let guard = defence::Guard {
                saber: &saber,
                ps: &ps,
                weapon_wait_ms: if attacking {
                    300
                } else {
                    m.g.pick(&[-1, 0, 1, 300])
                },
                level: m.g.below(4) as u8,
                attack_held: !attacking && m.g.chance(0.3),
                hand_out: m.g.chance(0.1),
            };
            let mut point = m.vec(60.0);
            point[0] += ps.origin[0];
            point[1] += ps.origin[1];
            point[2] += ps.origin[2];
            let against = m.g.pick(&[
                defence::Against::Bolt,
                defence::Against::Blade,
                defence::Against::Thrown,
            ]);
            let now = m.now();
            saber_fields(&mut r, "s", &saber);
            ps_fields(&mut r, "p", &ps);
            r.i("wms", guard.weapon_wait_ms)
                .i("level", guard.level)
                .b("held", guard.attack_held)
                .b("hand", guard.hand_out)
                .v3("point", point)
                .d("against", &against)
                .i("now", now)
                .b("can_block", defence::can_block(&guard, point, against, now))
                .b(
                    "in_arc",
                    defence::in_arc(ps.origin, ps.view[1], guard.level, point, against),
                )
                .b("ready", defence::ready(&guard, now));
            let bolt = m.g.chance(0.3);
            r.b("bolt", bolt)
                .d("parry", &defence::parry_for(&ps, point, bolt));
        }
        "bolt" => {
            let ps = m.player();
            let (def, deflect) = (m.g.below(5) as u8, m.g.int(-20, 40) as i8);
            let level = defence::bolt_level(def, deflect);
            let back = m.g.chance(0.3);
            let now_level = defence::bolt_level_now(&ps, back, level);
            let (at, shooter, now) = (m.vec(50.0), m.vec(800.0), m.now());
            let (fate, cooldown) = defence::bolt(&ps, now_level, at, shooter, &mut m.rng, now);
            ps_fields(&mut r, "p", &ps);
            r.i("def", def)
                .i("deflect", deflect)
                .i("level", level)
                .b("back", back)
                .i("level_now", now_level)
                .v3("at", at)
                .v3("shooter", shooter)
                .i("now", now)
                .i("cooldown", cooldown);
            match fate {
                defence::BoltFate::Absorbed => r.s("fate", "absorbed"),
                defence::BoltFate::Turned(d) => r.v3("turned", d),
            };
        }
        "can_lock" | "pick" => {
            let (pa, pb) = (m.player(), m.player());
            let mut pb = pb;
            // Near enough to lock, as often as not.
            if m.g.chance(0.6) {
                pb.origin = [
                    pa.origin[0] + m.g.range(-60.0, 60.0),
                    pa.origin[1] + m.g.range(-60.0, 60.0),
                    pa.origin[2] + m.g.range(-10.0, 10.0),
                ];
            }
            let stance = |m: &mut Maker, ps| lock::Stance {
                ps,
                player: m.g.chance(0.5),
                blocking: m.g.pick(&[Block::No, Block::Tight, Block::Wide]),
                weapon_wait_ms: m.g.pick(&[-1, 0, 1, 300]),
                busy: m.g.chance(0.1),
            };
            let (a, b) = (stance(&mut m, &pa), stance(&mut m, &pb));
            ps_fields(&mut r, "a", &pa);
            ps_fields(&mut r, "b", &pb);
            for (who, s) in [("a", &a), ("b", &b)] {
                r.b(&format!("{who}.player"), s.player)
                    .d(&format!("{who}.blk"), &s.blocking)
                    .i(&format!("{who}.wms"), s.weapon_wait_ms)
                    .b(&format!("{who}.busy"), s.busy);
            }
            r.b("can_lock", lock::can_lock(&a, &b))
                .d("pick", &lock::pick(&a, &b, &mut m.rng))
                .b("in_reach", lock::in_reach(pa.origin, pb.origin));
        }
        "lock_anims" => {
            let anim = m.anim();
            r.s("anim", anim.name())
                .b("rises", lock::rises_to_win(anim))
                .b("lock", lock::in_lock_pose(anim))
                .d("never", &lock::NEVER_PLAYED.contains(&anim));
        }
        "opening" => {
            let kinds = [
                lock::Kind::Top,
                lock::Kind::TopRight,
                lock::Kind::TopLeft,
                lock::Kind::BottomRight,
                lock::Kind::BottomLeft,
                lock::Kind::R,
                lock::Kind::L,
            ];
            let (kind, att, def) = (m.g.pick(&kinds), m.style(), m.style());
            let o = lock::opening(kind, att, def);
            r.d("kind", &kind)
                .s("att", att.name())
                .s("def", def.name())
                .s("a_anim", o.attacker.0.name())
                .f("a_frac", o.attacker.1)
                .s("d_anim", o.defender.0.name())
                .f("d_frac", o.defender.1)
                .f("distance", o.distance);
        }
        "lock_frames" => {
            let anim = m.anim();
            let fraction = m.g.range(0.0, 1.0);
            let first = lock::first_frame(m.anims, anim, fraction);
            let held = Lock {
                enemy: SimId(m.g.below(9)),
                anim,
                frame: first + m.g.int(-5, 30) as i32,
                hits: m.g.below(9) as u8,
                advance: m.g.chance(0.5),
                pressed: m.g.chance(0.5),
                checked_ms: m.now(),
            };
            let strength = m.g.int(0, 12) as i32;
            let remaining = m.g.int(0, 40) as i32;
            r.s("anim", anim.name())
                .f("fraction", fraction)
                .i("first", first)
                .i("frame", held.frame)
                .i("strength", strength)
                .d("advance", &lock::advance(m.anims, &held, strength))
                .i("remaining", remaining)
                .i("mirror", lock::mirror(m.anims, anim, remaining));
        }
        "press_hits" => {
            let (style, strong) = (m.style(), m.g.chance(0.4));
            r.s("style", style.name())
                .b("strong", strong)
                .i("got", lock::press_hits(style, strong, &mut m.rng));
        }
        "breaks" => {
            let (winner, loser) = (m.anim(), m.anim());
            let (victory, sb) = (m.g.chance(0.5), m.g.chance(0.3));
            let (w, l) = lock::breaks(winner, loser, victory, sb);
            r.s("winner", winner.name())
                .s("loser", loser.name())
                .b("victory", victory)
                .b("sb", sb);
            for (who, o) in [("w", w), ("l", l)] {
                r.s(&format!("{who}.anim"), o.anim.name())
                    .s(&format!("{who}.mv"), o.saber_move.map_or("-", Move::name))
                    .b(&format!("{who}.defenceless"), o.defenceless);
            }
        }
        "lock_rolls" => {
            let (strength, hits, hp) = (
                m.g.int(0, 12) as i32,
                m.g.below(12) as u8,
                m.g.int(0, 100) as i32,
            );
            r.i("strength", strength)
                .i("hits", hits)
                .i("hp", hp)
                .b("super", lock::super_breaks(strength, hits, &mut m.rng))
                .b("kills", lock::loss_kills(hp, &mut m.rng));
        }
        "shove" => {
            let (from, to, lost) = (m.vec(100.0), m.vec(100.0), m.g.chance(0.5));
            r.v3("from", from)
                .v3("to", to)
                .b("lost", lost)
                .v3("got", lock::shove(from, to, lost))
                .b("in_reach", lock::in_reach(from, to));
        }
        "kick" => {
            let mut ps = m.player();
            let kicks = [
                "BOTH_A7_KICK_F",
                "BOTH_A7_KICK_B",
                "BOTH_A7_KICK_R",
                "BOTH_A7_KICK_L",
                "BOTH_A7_KICK_S",
                "BOTH_A7_KICK_BF",
                "BOTH_A7_KICK_RL",
                "BOTH_A7_KICK_F_AIR",
                "BOTH_A7_KICK_B_AIR",
                "BOTH_A7_KICK_R_AIR",
                "BOTH_A7_KICK_L_AIR",
                "BOTH_GETUP_BROLL_F",
                "BOTH_GETUP_BROLL_B",
                "BOTH_GETUP_FROLL_F",
                "BOTH_A7_HILT",
            ];
            if m.g.chance(0.8) {
                ps.legs.anim = Anim::from_name(m.g.pick(&kicks)).unwrap_or(Anim::BOTH_STAND1);
                let length = m.anims.get(ps.legs.anim).length_ms();
                ps.legs.timer = m.g.int(0, i64::from(length.max(0))) as i32;
            }
            // A hilt bash on the torso, its timer anywhere in its length.
            if m.g.chance(0.25) {
                ps.torso.anim = Anim::BOTH_A7_HILT;
                let length = m.anims.get(ps.torso.anim).length_ms();
                ps.torso.timer = m.g.int(0, i64::from(length.max(0))) as i32;
            }
            let now = m.now();
            ps_fields(&mut r, "p", &ps);
            r.i("now", now);
            match kick::kick(&ps, m.anims, now) {
                None => r.s("kick", "-"),
                Some(k) => r
                    .v3("dir", k.direction)
                    .f("reach", k.reach)
                    .fs("push", &[k.push.0, k.push.1]),
            };
        }
        "floors" => {
            let ps = m.player();
            let (wms, held, down) = (m.g.pick(&[-1, 0, 1, 300]), m.g.chance(0.7), m.g.chance(0.2));
            ps_fields(&mut r, "p", &ps);
            r.i("wms", wms)
                .b("held", held)
                .b("down", down)
                .b("got", kick::floors(&ps, wms, held, down));
        }
        "facing" => {
            let mut ps = m.player();
            ps.push_dir = m.g.below(8) as u8;
            match m.g.below(5) {
                0 => ps.velocity[0] = 0.0,
                1 => ps.velocity[1] = 0.0,
                _ => {}
            }
            let doing = look::Doing {
                anims: [ps.legs.anim, ps.torso.anim],
                special_move: m.g.chance(0.2),
                locked: m.g.chance(0.1),
                alive: m.g.chance(0.9),
            };
            ps_fields(&mut r, "p", &ps);
            // The torso's `Motion` bolt: none, turned anyhow, or an axis
            // straight up or down.
            let bolt = match m.g.below(4) {
                0 => None,
                1 => {
                    let up = if m.g.chance(0.5) { 1.0 } else { -1.0 };
                    Some(BoltMatrix {
                        origin: m.vec(20.0),
                        axes: [[0.0, up, 0.0], [0.0, 0.0, up], [up, 0.0, 0.0]],
                    })
                }
                _ => {
                    let mut axes = [m.vec(1.0), m.vec(1.0), m.vec(1.0)];
                    for axis in &mut axes {
                        crate::math::normalize(axis);
                    }
                    Some(BoltMatrix {
                        origin: m.vec(20.0),
                        axes,
                    })
                }
            };
            r.i("md", ps.push_dir)
                .b("special", doing.special_move)
                .b("locked", doing.locked)
                .b("alive", doing.alive);
            match &bolt {
                None => r.s("bolt", "-"),
                Some(b) => r.v3("bolt.o", b.origin).fs(
                    "bolt.axes",
                    &b.axes.iter().flatten().copied().collect::<Vec<f32>>(),
                ),
            };
            let asked = std::cell::Cell::new(false);
            let facing = look::facing(&ps, doing, || {
                asked.set(true);
                bolt
            });
            let flat: Vec<f32> = facing.look.iter().flatten().copied().collect();
            r.b("asked", asked.get())
                .f("legs_yaw", facing.legs_yaw)
                .fs("look", &flat);
        }
        "segments" | "sweep_quad" => {
            let unit = |m: &mut Maker| {
                let mut d = m.vec(1.0);
                crate::math::normalize(&mut d);
                d
            };
            let unit = |m: &mut Maker| match m.g.below(20) {
                0 => [0.0, 0.0, if m.g.chance(0.5) { 1.0 } else { -1.0 }],
                1 => [0.0, 0.6, if m.g.chance(0.5) { 0.8 } else { -0.8 }],
                _ => unit(m),
            };
            let (b0, d0) = (m.vec(40.0), unit(&mut m));
            let (b1, d1) = match m.g.below(10) {
                0 => (b0, d0),
                1 => (b0, unit(&mut m)),
                _ => (m.vec(40.0), unit(&mut m)),
            };
            let (length, arcs) = (m.g.range(20.0, 48.0), m.g.chance(0.7));
            r.v3("b0", b0)
                .v3("d0", d0)
                .v3("b1", b1)
                .v3("d1", d1)
                .f("length", length)
                .b("arcs", arcs);
            if kind == "segments" {
                let segments = sweep::segments(b0, d0, b1, d1, length, arcs);
                r.i("count", segments.len());
                for (i, s) in segments.iter().enumerate().take(64) {
                    let fraction = m.g.range(0.0, 1.0);
                    r.v3(&format!("s{i}.from"), s.from)
                        .v3(&format!("s{i}.to"), s.to)
                        .f(&format!("s{i}.len"), s.length)
                        .f(&format!("s{i}.frac"), fraction)
                        .f(&format!("s{i}.worth"), s.worth(fraction))
                        .v3(&format!("s{i}.at"), s.at(fraction));
                }
            } else {
                let a = sweep::sweep_quad(b0, d0, b1, d1, length);
                let b =
                    sweep::sweep_quad(m.vec(40.0), unit(&mut m), m.vec(40.0), unit(&mut m), length);
                let flat: Vec<f32> = a.iter().flatten().flatten().copied().collect();
                r.fs("quad", &flat)
                    .b("meet", sweep::quads_meet(&a, &b))
                    .b("tri", sweep::triangles_meet(&a[0], &b[1]));
            }
        }
        "enters" => {
            let segment = sweep::Segment {
                from: m.vec(60.0),
                to: m.vec(60.0),
                length: m.g.range(1.0, 60.0),
            };
            let at = m.vec(30.0);
            let half = m.g.range(4.0, 20.0);
            let body = Body {
                entity: 7 | crate::BODY,
                min: [
                    (at[0] - half) * UNIT,
                    (at[1] - half) * UNIT,
                    (at[2] - 24.0) * UNIT,
                ],
                max: [
                    (at[0] + half) * UNIT,
                    (at[1] + half) * UNIT,
                    (at[2] + 40.0) * UNIT,
                ],
                contents: Contents::BODY,
            };
            let box_half = m.g.range(0.0, 3.0);
            let to = if m.g.chance(0.1) {
                segment.from
            } else {
                segment.to
            };
            r.v3("near", sweep::closest_on_segment(at, segment.from, to))
                .f("dist", sweep::distance_to_segment(at, segment.from, to))
                .f("yaw", sweep::yaw_to(segment.from, at));
            r.v3("from", segment.from)
                .v3("to", segment.to)
                .f("len", segment.length)
                .v3("body_min", body.min)
                .v3("body_max", body.max)
                .f("half", box_half)
                .d(
                    "got",
                    &sweep::enters(&segment, &body, box_half).map(f32::to_bits),
                );
        }
        "swing" => swing(&mut m, &mut r),
        "lock_codec" => lock_codec(&mut m, &mut r),
        other => return Err(format!("no case {other} (one of {})", KINDS.join(", "))),
    }
    rng_hex(&mut r, "rng_after", &m.rng);
    trace.put(&r)
}

/// A swing's bookkeeping over a run of operations: begun, touched,
/// turned, asked, dropped, delivered.
fn swing(m: &mut Maker, r: &mut Record) {
    let mut swing = sweep::Swing::default();
    let mut now = u64::from(m.g.below(1000));
    let ops = m.g.int(1, 40);
    for k in 0..ops {
        now += u64::from(m.g.below(120));
        let victim = SimId(m.g.below(6) + 1);
        let key = |what: &str| format!("{k}.{what}");
        match m.g.below(8) {
            0 => {
                let mv = if m.g.chance(0.7) {
                    m.g.pick(&[Move::A_TL2BR, Move::A_T2B, Move::A_L2R, Move::A_BR2TL])
                } else {
                    m.saber_move()
                };
                let (flip, hold) = (m.g.chance(0.5), m.g.below(900));
                swing = sweep::Swing::begin(&swing, mv, flip, hold, now);
                r.s(&key("begin"), mv.name())
                    .b(&key("flip"), flip)
                    .i(&key("hold"), hold);
            }
            1..=3 => {
                let (worth, point, direction) = (m.g.range(-2.0, 80.0), m.vec(40.0), m.vec(1.0));
                swing.touch(victim, worth, point, direction, now);
                r.i(&key("touch"), victim.0)
                    .f(&key("worth"), worth)
                    .v3(&key("p"), point)
                    .v3(&key("d"), direction);
            }
            4 => {
                let point = m.vec(40.0);
                swing.turn(victim, point, now);
                r.i(&key("turn"), victim.0).v3(&key("p"), point);
            }
            5 => {
                r.i(&key("verdict"), victim.0)
                    .d(&key("got"), &swing.verdict(victim));
                let (mv, flip) = (m.saber_move(), m.g.chance(0.5));
                r.s(&key("is"), mv.name())
                    .b(&key("is_got"), swing.is(mv, flip));
            }
            6 => {
                swing.drop_pending(victim);
                r.i(&key("drop"), victim.0);
            }
            _ => {
                let ending = m.g.chance(0.3);
                let due = swing.due(now, ending);
                r.b(&key("ending"), ending).d(&key("due"), &due);
            }
        }
        r.i(&key("now"), now);
        for c in swing.contacts.iter().flatten() {
            r.d(&format!("{k}.share{}", c.victim.0), &c.due_share());
        }
    }
    r.d("swing", &swing);
    let mut out = Writer::default();
    swing.encode(&mut out);
    let mut bytes = out.0;
    let mut input = Reader::new("swing", &bytes);
    r.hex("bytes", &bytes)
        .b("same", sweep::Swing::decode(&mut input).ok() == Some(swing));
    match m.g.below(3) {
        0 => bytes[0] = 0xff,
        1 => bytes[1] = m.g.int(2, 255) as u8,
        _ => bytes.truncate(m.g.below(bytes.len() as u32) as usize),
    }
    let mut input = Reader::new("swing", &bytes);
    let got = sweep::Swing::decode(&mut input);
    r.s(
        "spoiled",
        &got.map_or_else(|e| e.to_string(), |w| format!("{w:?}")),
    );
}

/// A lock's snapshot form: written and read back, then read from bytes
/// spoiled at one place.
fn lock_codec(m: &mut Maker, r: &mut Record) {
    let held = Lock {
        enemy: SimId(m.g.u32()),
        anim: m.anim(),
        frame: m.g.int(-100, 100) as i32,
        hits: m.g.below(256) as u8,
        advance: m.g.chance(0.5),
        pressed: m.g.chance(0.5),
        checked_ms: u64::from(m.g.u32()),
    };
    let mut out = Writer::default();
    held.encode(&mut out);
    let mut bytes = out.0;
    r.d("lock", &held).hex("bytes", &bytes);
    let mut input = Reader::new("lock", &bytes);
    r.b("same", Lock::decode(&mut input).ok() == Some(held));
    // Spoiled: the animation's index, a flag, or cut short.
    match m.g.below(4) {
        0 => {
            bytes[4] = 0xff;
            bytes[5] = 0xff;
        }
        1 => bytes[11] = m.g.int(2, 255) as u8,
        2 => bytes[12] = m.g.int(2, 255) as u8,
        _ => bytes.truncate(m.g.below(bytes.len() as u32) as usize),
    }
    let mut input = Reader::new("lock", &bytes);
    let got = Lock::decode(&mut input);
    r.hex("spoiled", &bytes).s(
        "decoded",
        &got.map_or_else(|e| e.to_string(), |l| format!("{l:?}")),
    );
}

/// A foe made up for a think.
fn sight(g: &mut Gen, now: u64) -> Sight {
    Sight {
        foe: SimId(g.below(3) + 1),
        gap: g.range(-20.0, 400.0),
        in_view: g.chance(0.8),
        closing: g.chance(0.6).then(|| g.range(-300.0, 300.0)),
        arms: match g.below(5) {
            0 => FoeArms::Gun {
                firing: g.chance(0.5),
            },
            1 => FoeArms::Other,
            _ => FoeArms::Saber { lit: g.chance(0.9) },
        },
        swinging: g.chance(0.3),
        grounded: g.chance(0.85),
        moving: g.chance(0.5),
        coming: g.range(-1.0, 1.0),
        facing: g.range(-1.0, 1.0),
        busy: g.chance(0.25),
        ally: g.chance(0.2),
        health: g.range(0.0, 100.0),
        saber_move: g.below(162) as u8,
        bounced: g.chance(0.1),
        broken: g.chance(0.05),
        parried: g.chance(0.1),
        pain_until: now + u64::from(g.below(800)),
    }
}

fn sight_fields(r: &mut Record, s: &Sight) {
    r.i("foe", s.foe.0)
        .f("gap", s.gap)
        .b("in_view", s.in_view)
        .d("closing", &s.closing.map(f32::to_bits))
        .d("arms", &s.arms)
        .b("swinging", s.swinging)
        .b("grounded", s.grounded)
        .b("moving", s.moving)
        .f("coming", s.coming)
        .f("facing", s.facing)
        .b("busy", s.busy)
        .b("ally", s.ally)
        .f("health", s.health)
        .i("smv", s.saber_move)
        .b("bounced", s.bounced)
        .b("broken", s.broken)
        .b("parried", s.parried)
        .i("pain", s.pain_until);
}

/// A foe held to the script's `key=value`s think after think (the rest
/// made up each think), and how the duel runs.
#[derive(Default)]
struct Script {
    fixed: Vec<(String, String)>,
    /// Added to the gap each think, from the script's or the first gap.
    approach: Option<f32>,
    /// Each think this long after the last, not mostly `THINK_MS`.
    step: Option<u64>,
    /// The clear sides' mask, not one made up each think.
    mask: Option<u32>,
    /// The pace begins with every field made up (timers about now).
    scramble: bool,
    /// The keys `finish` is given carry a side key now and then.
    sides: bool,
    /// The pace begins with this aggression, rating this foe (its id + 1),
    /// roaming this many ms more.
    aggression: Option<i8>,
    rated: Option<u32>,
    roam: Option<u64>,
}

impl Script {
    fn of(words: &[String]) -> Result<Script, String> {
        let mut script = Script::default();
        for (key, value) in pairs(words) {
            match key {
                "approach" => script.approach = Some(float(value)?),
                "step" => script.step = Some(int(value)?),
                "mask" => script.mask = Some(int(value)?),
                "scramble" => script.scramble = value == "1",
                "sides" => script.sides = value == "1",
                "aggression" => script.aggression = Some(int(value)?),
                "rated" => script.rated = Some(int(value)?),
                "roam" => script.roam = Some(int(value)?),
                _ => script.fixed.push((key.to_string(), value.to_string())),
            }
        }
        Ok(script)
    }

    /// `sight` with the script's fields; the gap after `k` thinks.
    fn apply(&self, sight: &mut Sight, now: u64, k: u64, first_gap: f32) -> Result<(), String> {
        let yes = |value: &str| value == "1";
        for (key, value) in &self.fixed {
            let value = value.as_str();
            match key.as_str() {
                "foe" => sight.foe = SimId(int(value)?),
                "gap" => sight.gap = float(value)?,
                "in_view" => sight.in_view = yes(value),
                "closing" => {
                    sight.closing = match value {
                        "none" => None,
                        v => Some(float(v)?),
                    }
                }
                "arms" => {
                    sight.arms = match value {
                        "saber" => FoeArms::Saber { lit: true },
                        "unlit" => FoeArms::Saber { lit: false },
                        "gun" => FoeArms::Gun { firing: false },
                        "firing" => FoeArms::Gun { firing: true },
                        "other" => FoeArms::Other,
                        other => return Err(format!("no foe arms {other}")),
                    }
                }
                "swinging" => sight.swinging = yes(value),
                "grounded" => sight.grounded = yes(value),
                "moving" => sight.moving = yes(value),
                "coming" => sight.coming = float(value)?,
                "facing" => sight.facing = float(value)?,
                "busy" => sight.busy = yes(value),
                // `flip`: a change of side every five thinks.
                "ally" if value == "flip" => sight.ally = (k / 5) % 2 == 1,
                "ally" => sight.ally = yes(value),
                "health" => sight.health = float(value)?,
                "smv" => sight.saber_move = int(value)?,
                "bounced" => sight.bounced = yes(value),
                "broken" => sight.broken = yes(value),
                "parried" => sight.parried = yes(value),
                // `+MS` after this think, or `MS` itself.
                "pain" => {
                    sight.pain_until = match value.strip_prefix('+') {
                        Some(after) => now + int::<u64>(after)?,
                        None => int(value)?,
                    }
                }
                other => return Err(format!("duel: no key {other}")),
            }
        }
        if let Some(approach) = self.approach {
            sight.gap = first_gap + approach * k as f32;
        }
        Ok(())
    }
}

/// A pace with every field made up, its timers about `now`.
fn scrambled(g: &mut Gen, now: u64) -> Pace {
    let time = |g: &mut Gen| match g.below(3) {
        0 => 0,
        1 => now.saturating_sub(u64::from(g.below(3000))),
        _ => now + u64::from(g.below(3000)),
    };
    let mut holds = [0; 6];
    for hold in &mut holds {
        *hold = time(g);
    }
    let mut again = [0; 4];
    for hold in &mut again {
        *hold = time(g);
    }
    Pace {
        keys: Keys {
            forward: g.pick(&[-127, 0, 127]),
            right: g.pick(&[-127, 0, 127]),
            walk: g.chance(0.3),
            attack: g.chance(0.3),
        },
        hunting: g.chance(0.5),
        aggression: g.int(-6, 8) as i8,
        weighed: g.below(5),
        rise_due: time(g),
        circle_left_until: time(g),
        circle_right_until: time(g),
        circle_pause_until: time(g),
        walk_until: time(g),
        holds,
        again,
        guard_until: time(g),
        guard_review_until: time(g),
        seen_move: g.below(162) as u8,
        seen_pain: time(g),
    }
}

/// A pace's snapshot form, written and read back; and read from bytes
/// spoiled at one place.
fn pace_codec(g: &mut Gen, r: &mut Record, pace: &Pace) {
    let mut out = Writer::default();
    pace.encode(&mut out);
    let mut bytes = out.0;
    let mut input = Reader::new("pace", &bytes);
    r.hex("pace_bytes", &bytes)
        .b("pace_same", Pace::decode(&mut input).ok() == Some(*pace));
    match g.below(3) {
        0 => bytes[2] = g.int(8, 255) as u8,
        _ => bytes.truncate(g.below(bytes.len() as u32) as usize),
    }
    let mut input = Reader::new("pace", &bytes);
    let got = Pace::decode(&mut input);
    r.s(
        "pace_spoiled",
        &got.map_or_else(|e| e.to_string(), |p| format!("{p:?}")),
    );
}

/// A blade wielder's pace over `thinks` thinks from `seed` against a foe
/// made up each think but for what `script` holds; the steps.
fn duel(
    seed: u64,
    thinks: u64,
    script: &Script,
    first: u64,
    trace: &mut Trace,
) -> Result<u64, String> {
    let mut g = Gen::new(seed, 0x6d35);
    let mut pace = Pace {
        aggression: g.int(-2, 6) as i8,
        ..Pace::default()
    };
    let id = SimId(g.below(50) + 1);
    let mut now = u64::from(g.below(100_000));
    if script.scramble {
        pace = scrambled(&mut g, now);
    }
    if let Some(aggression) = script.aggression {
        pace.aggression = aggression;
    }
    if let Some(rated) = script.rated {
        pace.weighed = rated;
    }
    if let Some(roam) = script.roam {
        pace.rise_due = now + roam;
    }
    let mut first_gap = None;
    let mut kept: Option<(u64, u8)> = None;
    for k in 0..thinks {
        now += match script.step {
            Some(step) => step,
            None if g.chance(0.9) => pace::THINK_MS,
            None => u64::from(g.below(2000)),
        };
        let mut s = sight(&mut g, now);
        // The foe's pain and move mostly as they were: its events then come
        // now and then, not every think.
        if let Some((pain, mv)) = kept.filter(|_| g.chance(0.85)) {
            s.pain_until = pain;
            s.saber_move = mv;
        }
        kept = Some((s.pain_until, s.saber_move));
        let gap = *first_gap.get_or_insert(s.gap);
        script.apply(&mut s, now, k, gap)?;
        let mask = script.mask.unwrap_or_else(|| g.u32());
        let clear = move |f: i8, r: i8| {
            let bit = (f.signum() + 1) * 3 + (r.signum() + 1);
            mask & (1 << bit) != 0
        };
        let mut dice = Dice::new(now, id);
        let mut r = Record::new(first + k, "think");
        r.i("now", now).i("id", id.0).i("mask", mask & 0x1ff);
        sight_fields(&mut r, &s);
        r.d("before", &pace);
        let way = pace.decide(&s, now, &mut dice, &clear);
        let mut keys = match way {
            pace::Way::Advance => Keys {
                forward: 127,
                ..Keys::default()
            },
            pace::Way::Retreat => Keys {
                forward: -127,
                ..Keys::default()
            },
            pace::Way::Hold => Keys::default(),
        };
        if script.sides {
            // Keys as anyone might give them: any forward and right, small
            // ones among them, walking or attacking.
            if g.chance(0.3) {
                keys.right = g.pick(&[-127, 127, -128, -5, 3, 64]);
            }
            if g.chance(0.2) {
                keys.forward = g.pick(&[-128, -1, 2, 40, 127]);
            }
            keys.walk = g.chance(0.25);
            keys.attack = g.chance(0.25);
        }
        let (turn, from_foe) = (g.range(-90.0, 90.0), g.pick(&[-1.0, 1.0, 0.0]));
        r.d("way", &way).d("keys_in", &keys);
        let keys = pace.finish(keys, &s, now, &mut dice, &clear, turn, from_foe);
        r.f("turn", turn)
            .f("from_foe", from_foe)
            .d("keys", &keys)
            .d("after", &pace);
        if k % 50 == 0 {
            pace_codec(&mut g, &mut r, &pace);
        }
        trace.put(&r)?;
    }
    Ok(thinks)
}

pub fn run(scenario: &Scenario, trace: &mut Trace) -> Result<u64, String> {
    let mut step = 0;
    for line in &scenario.lines {
        let a = line.args();
        let word = |i: usize| -> Result<&str, String> {
            a.get(i)
                .map(String::as_str)
                .ok_or_else(|| format!("too few words: {}", line.place()))
        };
        match line.verb() {
            "case" => {
                case(word(0)?, int(word(1)?)?, step, trace)?;
                step += 1;
            }
            "cases" => {
                let (count, seed): (u64, u64) = (int(word(1)?)?, int(word(2)?)?);
                for k in 0..count {
                    case(
                        word(0)?,
                        crate::rng::mix64(seed.wrapping_add(k)),
                        step,
                        trace,
                    )?;
                    step += 1;
                }
            }
            "duel" => {
                let script = Script::of(a.get(2..).unwrap_or(&[]))?;
                step += duel(int(word(0)?)?, int(word(1)?)?, &script, step, trace)?;
            }
            other => return Err(format!("no directive {other}: {}", line.place())),
        }
    }
    Ok(step)
}

/// `n` cases of every kind for `seed`, in scenarios of up to 10 000.
pub fn random_m4(seed: u64, n: u64) -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut left = n;
    let mut part = 0;
    while left > 0 {
        let count = left.min(10_000);
        let mut s = Scenario::new(format!("random-{seed}-{part}"));
        s.push_owned(vec![
            "cases".into(),
            "any".into(),
            count.to_string(),
            crate::rng::mix64(seed ^ (part << 32)).to_string(),
        ]);
        out.push(s);
        left -= count;
        part += 1;
    }
    out
}

/// `n` thinks for `seed`, in duels of up to 500.
pub fn random_m5(seed: u64, n: u64) -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut left = n;
    let mut part = 0;
    while left > 0 {
        let count = left.min(500);
        let mut s = Scenario::new(format!("random-{seed}-{part}"));
        let mut words: Vec<String> = vec![
            "duel".into(),
            crate::rng::mix64(seed ^ (part << 32)).to_string(),
            count.to_string(),
        ];
        // Every third begun from a made-up pace, every other with side keys.
        if part % 3 == 0 {
            words.push("scramble=1".into());
        }
        if part % 2 == 0 {
            words.push("sides=1".into());
        }
        s.push_owned(words);
        out.push(s);
        left -= count;
        part += 1;
    }
    out
}
