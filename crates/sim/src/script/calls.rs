//! Routine calls made a program without a script of the game's
//! (`script_ncs::asm`): the launcher's probes, and the calls a tool queues
//! in a save for its scripts to run as it loads (the save editor's party
//! changes: a companion made and joined by the routines the games' own
//! scripts call, `make_npc` and all).
//!
//! A queued run is a pending script (`State::pending`) whose name is
//! `QUEUED` and the calls written out (`queued`); no resref holds a `:`,
//! so none is taken for a script of the game's. Its program is assembled
//! from the level's routine catalogue when it runs (`read`).

use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::Catalog;

/// A step of a program: an op, or a routine called with its arguments
/// already pushed (the last argument first).
#[derive(Debug, Clone)]
pub enum Call {
    Op(Op),
    Routine(&'static str, u8),
}

pub fn int(v: i32) -> Call {
    Call::Op(Op::Const(Const::Int(v)))
}

pub fn float(v: f32) -> Call {
    Call::Op(Op::Const(Const::Float(v)))
}

pub fn text(v: &str) -> Call {
    Call::Op(Op::Const(Const::String(v.as_bytes().to_vec())))
}

/// `OBJECT_SELF`.
pub fn me() -> Call {
    Call::Op(Op::Const(Const::Object(0)))
}

/// `GetObjectByTag(tag, nth)`.
pub fn by_tag(tag: &str, nth: i32) -> Vec<Call> {
    vec![int(nth), text(tag), Call::Routine("GetObjectByTag", 2)]
}

/// A program that runs `calls` in order and returns.
pub fn program(catalog: &Catalog, calls: &[Vec<Call>]) -> Result<Ncs, String> {
    assemble(
        catalog,
        calls.iter().flatten().map(|call| match call {
            Call::Op(op) => Step::Op(op.clone()),
            Call::Routine(name, args) => Step::Routine(name, *args),
        }),
    )
}

/// The start of a queued run's name.
pub const QUEUED: &str = "kjk:";

/// A step of a queued run as its name writes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Queued {
    Int(i32),
    Float(f32),
    Text(String),
    Object(i32),
    Routine(String, u8),
}

/// `calls` as a queued run's name: constants and routines only (no other
/// op), texts without spaces (templates and tags).
pub fn queued(calls: &[Vec<Call>]) -> Result<String, String> {
    let mut name = QUEUED.to_string();
    for (n, call) in calls.iter().flatten().enumerate() {
        if n > 0 {
            name.push(' ');
        }
        let token = match call {
            Call::Routine(routine, args) => format!("{routine}/{args}"),
            Call::Op(Op::Const(Const::Int(v))) => format!("i{v}"),
            Call::Op(Op::Const(Const::Float(v))) => format!("f{v}"),
            Call::Op(Op::Const(Const::Object(v))) => format!("o{v}"),
            Call::Op(Op::Const(Const::String(bytes))) => {
                let text = String::from_utf8(bytes.clone()).map_err(|_| "a text not UTF-8")?;
                if text.contains(char::is_whitespace) {
                    return Err(format!("{text:?}: a queued text holds no space"));
                }
                format!("s{text}")
            }
            Call::Op(op) => return Err(format!("{op:?}: only constants and routines queue")),
        };
        name.push_str(&token);
    }
    Ok(name)
}

/// The steps of queued run `name`; `None` when it is not one.
pub fn steps(name: &str) -> Option<Result<Vec<Queued>, String>> {
    let rest = name.strip_prefix(QUEUED)?;
    let step = |token: &str| -> Result<Queued, String> {
        let bad = || format!("{token:?}: not a queued step");
        if let Some((routine, args)) = token.split_once('/') {
            return Ok(Queued::Routine(
                routine.to_string(),
                args.parse().map_err(|_| bad())?,
            ));
        }
        let (kind, value) = token.split_at_checked(1).ok_or_else(bad)?;
        Ok(match kind {
            "i" => Queued::Int(value.parse().map_err(|_| bad())?),
            "f" => Queued::Float(value.parse().map_err(|_| bad())?),
            "o" => Queued::Object(value.parse().map_err(|_| bad())?),
            "s" => Queued::Text(value.to_string()),
            _ => return Err(bad()),
        })
    };
    Some(
        rest.split(' ')
            .filter(|token| !token.is_empty())
            .map(step)
            .collect(),
    )
}

/// Queued run `name`'s program, assembled from `catalog`; `None` when it
/// is not one.
pub fn read(name: &str, catalog: &Catalog) -> Option<Result<Ncs, String>> {
    let steps = match steps(name)? {
        Ok(steps) => steps,
        Err(why) => return Some(Err(why)),
    };
    let op = |constant| Step::Op(Op::Const(constant));
    Some(assemble(
        catalog,
        steps.iter().map(|step| match step {
            Queued::Int(v) => op(Const::Int(*v)),
            Queued::Float(v) => op(Const::Float(*v)),
            Queued::Object(v) => op(Const::Object(*v)),
            Queued::Text(v) => op(Const::String(v.as_bytes().to_vec())),
            Queued::Routine(name, args) => Step::Routine(name, *args),
        }),
    ))
}

/// An op, or a routine by name with its argument count.
enum Step<'a> {
    Op(Op),
    Routine(&'a str, u8),
}

fn assemble<'a>(catalog: &Catalog, steps: impl Iterator<Item = Step<'a>>) -> Result<Ncs, String> {
    let mut asm = script_ncs::asm::Asm::default();
    for step in steps {
        match step {
            Step::Op(op) => {
                asm.op(op);
            }
            Step::Routine(name, args) => {
                let routine = catalog
                    .routine_named(name)
                    .ok_or(format!("{name}: not in nwscript.nss"))?;
                asm.op(Op::Action {
                    routine: routine.id,
                    args,
                });
            }
        }
    }
    asm.op(Op::Retn);
    Ok(asm.build())
}
