//! `m2`: the saber move table and its queries, dumped whole or asked at
//! random. Directives: `dump WHAT` (`all`, `moves`, `kinds`, `links`,
//! `transitions`, `chain_angles`, `parries`, `bounces`, `broken`,
//! `returns`, `indexes`, `blocked`, `quads`) and `ask QUERY ARGS` (one
//! query: `data M`, `kinds M`, `links M`, `transition Q Q`, `chain_angle M
//! M`, `attack_from_parry Q`, `bounce_for M`, `broken_parry_for M`,
//! `return_from Q`, `from_index I`, `blocked B B`), moves by name or
//! number, quadrants by name.

use super::movement::{Gen, blocked_all};
use super::record::{Record, Trace};
use super::scenario::{Line, Scenario, int};
use crate::combat::saber::moves::{
    attack_from_parry, bounce_for, broken_parry_for, chain_angle, return_from, transition,
};
use crate::combat::saber::{Blocked, Move, Quad};

struct Asker<'a> {
    trace: &'a mut Trace,
    step: u64,
}

impl Asker<'_> {
    fn put(&mut self, record: Record) -> Result<(), String> {
        self.step += 1;
        self.trace.put(&record)
    }

    fn record(&self, op: &str) -> Record {
        Record::new(self.step, op)
    }

    /// A move's row of the table.
    fn data(&mut self, m: Move) -> Result<(), String> {
        let d = m.data();
        let mut r = self.record("data");
        r.s("m", m.name())
            .i("index", m.index())
            .s("anim", d.anim.name())
            .i("anim_index", d.anim.index())
            .d("start", &d.start)
            .d("end", &d.end)
            .d("hold", &d.hold)
            .d("block", &d.block)
            .i("block_n", d.block as u8)
            .s("idle", d.chain_idle.name())
            .s("attack", d.chain_attack.name());
        self.put(r)
    }

    /// A move's kinds.
    fn kinds(&mut self, m: Move) -> Result<(), String> {
        let mut r = self.record("kinds");
        r.s("m", m.name())
            .b("pure", m.pure_attack())
            .b("attack", m.attack())
            .b("special", m.special())
            .b("kata", m.kata())
            .b("kick", m.kick())
            .b("idle", m.idle())
            .b("start", m.start())
            .b("ret", m.ret())
            .b("transition", m.transition())
            .b("between", m.in_between())
            .b("bounce", m.bounce())
            .b("broken", m.broken())
            .b("knockaway", m.knockaway())
            .b("parry", m.parry())
            .b("reflect", m.reflect());
        self.put(r)
    }

    /// The moves a move leads to and from: the attack of a start, the
    /// start and return of a plain attack (only where they are defined),
    /// its bounce, the broken parry for it.
    fn links(&mut self, m: Move) -> Result<(), String> {
        let mut r = self.record("links");
        r.s("m", m.name());
        if m.start() {
            r.s("attack_of_start", m.attack_of_start().name());
        }
        if m.pure_attack() {
            r.s("start_of_attack", m.start_of_attack().name())
                .s("return_of_attack", m.return_of_attack().name());
        }
        r.s("bounce_for", bounce_for(m).name()).s(
            "broken_parry_for",
            broken_parry_for(m).map_or("-", Move::name),
        );
        self.put(r)
    }

    fn transition(&mut self, from: Quad, to: Quad) -> Result<(), String> {
        let mut r = self.record("transition");
        r.d("from", &from)
            .d("to", &to)
            .s("move", transition(from, to).map_or("-", Move::name));
        self.put(r)
    }

    fn chain_angle(&mut self, from: Move, to: Move) -> Result<(), String> {
        let mut r = self.record("chain_angle");
        r.s("from", from.name())
            .s("to", to.name())
            .i("angle", chain_angle(from, to));
        self.put(r)
    }

    fn quad_query(&mut self, op: &str, quad: Quad) -> Result<(), String> {
        let mut r = self.record(op);
        r.d("q", &quad);
        match op {
            "attack_from_parry" => r.s("move", attack_from_parry(quad).map_or("-", Move::name)),
            _ => r.s("move", return_from(quad).name()),
        };
        self.put(r)
    }

    fn by_index(&mut self, index: u16) -> Result<(), String> {
        let mut r = self.record("from_index");
        r.i("i", index)
            .s("move", Move::from_index(index).map_or("-", Move::name));
        self.put(r)
    }

    /// How two `Blocked`s order, each's number, and which are melee
    /// parries.
    fn blocked(&mut self, a: Blocked, b: Blocked) -> Result<(), String> {
        let mut r = self.record("blocked");
        r.d("a", &a)
            .i("a_n", a as u8)
            .b("a_melee", a.melee_parry())
            .d("b", &b)
            .d("cmp", &a.cmp(&b))
            .d("partial", &a.partial_cmp(&b))
            .b("eq", a == b)
            .b("default", a == Blocked::default());
        self.put(r)
    }

    fn dump(&mut self, what: &str) -> Result<(), String> {
        let all = Move::ALL;
        match what {
            "all" => {
                for what in [
                    "quads",
                    "moves",
                    "kinds",
                    "links",
                    "transitions",
                    "chain_angles",
                    "parries",
                    "returns",
                    "indexes",
                    "blocked",
                ] {
                    self.dump(what)?;
                }
            }
            "quads" => {
                for (i, quad) in Quad::ALL.iter().enumerate() {
                    let mut r = self.record("quad");
                    r.i("i", i).d("q", quad).i("n", *quad as u8);
                    self.put(r)?;
                }
                let mut r = self.record("count");
                r.i("moves", all.len());
                self.put(r)?;
            }
            "moves" => all.iter().try_for_each(|m| self.data(*m))?,
            "kinds" => all.iter().try_for_each(|m| self.kinds(*m))?,
            "links" | "bounces" | "broken" => all.iter().try_for_each(|m| self.links(*m))?,
            "transitions" => {
                for from in Quad::ALL {
                    for to in Quad::ALL {
                        self.transition(from, to)?;
                    }
                }
            }
            "chain_angles" => {
                for from in all {
                    for to in all {
                        self.chain_angle(*from, *to)?;
                    }
                }
            }
            "parries" => Quad::ALL
                .iter()
                .try_for_each(|q| self.quad_query("attack_from_parry", *q))?,
            "returns" => Quad::ALL
                .iter()
                .try_for_each(|q| self.quad_query("return_from", *q))?,
            "indexes" => {
                for index in (0..=200).chain([255, 256, 1000, u16::MAX]) {
                    self.by_index(index)?;
                }
            }
            "blocked" => {
                for a in blocked_all() {
                    for b in blocked_all() {
                        self.blocked(a, b)?;
                    }
                }
            }
            other => return Err(format!("no dump {other}")),
        }
        Ok(())
    }

    fn ask(&mut self, words: &[String]) -> Result<(), String> {
        let word = |i: usize| -> Result<&str, String> {
            words
                .get(i)
                .map(String::as_str)
                .ok_or_else(|| "too few words".to_string())
        };
        match word(0)? {
            "data" => self.data(the_move(word(1)?)?),
            "kinds" => self.kinds(the_move(word(1)?)?),
            "links" => self.links(the_move(word(1)?)?),
            "transition" => self.transition(quad(word(1)?)?, quad(word(2)?)?),
            "chain_angle" => self.chain_angle(the_move(word(1)?)?, the_move(word(2)?)?),
            q @ ("attack_from_parry" | "return_from") => self.quad_query(q, quad(word(1)?)?),
            "bounce_for" | "broken_parry_for" => self.links(the_move(word(1)?)?),
            "from_index" => self.by_index(int(word(1)?)?),
            "blocked" => self.blocked(blocked_named(word(1)?)?, blocked_named(word(2)?)?),
            other => Err(format!("no query {other}")),
        }
    }

    fn line(&mut self, line: &Line) -> Result<(), String> {
        let word = line.args().first().map(String::as_str).unwrap_or("");
        match line.verb() {
            "dump" => self.dump(word),
            "ask" => self.ask(line.args()),
            other => Err(format!("no directive {other}")),
        }
    }
}

fn the_move(word: &str) -> Result<Move, String> {
    Move::ALL
        .iter()
        .copied()
        .find(|m| m.name() == word)
        .or_else(|| word.parse::<u16>().ok().and_then(Move::from_index))
        .ok_or_else(|| format!("no move {word}"))
}

fn quad(word: &str) -> Result<Quad, String> {
    Quad::ALL
        .iter()
        .copied()
        .find(|q| format!("{q:?}") == word)
        .ok_or_else(|| format!("no quadrant {word}"))
}

fn blocked_named(word: &str) -> Result<Blocked, String> {
    blocked_all()
        .into_iter()
        .find(|b| format!("{b:?}") == word)
        .ok_or_else(|| format!("no blocked {word}"))
}

pub fn run(scenario: &Scenario, trace: &mut Trace) -> Result<u64, String> {
    let mut asker = Asker { trace, step: 0 };
    for line in &scenario.lines {
        asker
            .line(line)
            .map_err(|e| format!("{e} ({})", line.place()))?;
    }
    Ok(asker.step)
}

/// `n` random queries for `seed`, in scenarios of up to 10 000.
pub fn random(seed: u64, n: u64) -> Vec<Scenario> {
    let mut g = Gen::new(seed, 0x6d32);
    let mut out = Vec::new();
    let mut left = n;
    let mut part = 0;
    let random_move = |g: &mut Gen| Move::ALL[g.below(Move::ALL.len() as u32) as usize].name();
    let random_quad = |g: &mut Gen| format!("{:?}", Quad::ALL[g.below(8) as usize]);
    while left > 0 {
        let count = left.min(10_000);
        let mut s = Scenario::new(format!("random-{seed}-{part}"));
        for _ in 0..count {
            let words: Vec<String> = match g.below(11) {
                0 => vec!["data".into(), random_move(&mut g).into()],
                1 => vec!["kinds".into(), random_move(&mut g).into()],
                2 => vec!["links".into(), random_move(&mut g).into()],
                3 => vec![
                    "transition".into(),
                    random_quad(&mut g),
                    random_quad(&mut g),
                ],
                4 | 5 => vec![
                    "chain_angle".into(),
                    random_move(&mut g).into(),
                    random_move(&mut g).into(),
                ],
                6 => vec!["attack_from_parry".into(), random_quad(&mut g)],
                7 => vec!["return_from".into(), random_quad(&mut g)],
                8 => vec!["from_index".into(), (g.u32() % 300).to_string()],
                9 => vec!["broken_parry_for".into(), random_move(&mut g).into()],
                _ => vec![
                    "blocked".into(),
                    format!("{:?}", blocked_all()[g.below(14) as usize]),
                    format!("{:?}", blocked_all()[g.below(14) as usize]),
                ],
            };
            let mut line = vec!["ask".to_string()];
            line.extend(words);
            s.push_owned(line);
        }
        out.push(s);
        left -= count;
        part += 1;
    }
    out
}
