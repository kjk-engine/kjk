//! The VM's probes: programs we write ourselves (no game script), each
//! checked for what it leaves, what it hands the host, or how it faults.
//! `kjk script probes` prints them; the approved test `script_vm` needs
//! every one to pass.

use std::collections::BTreeMap;

use fmt_odyssey::ncs::{Const, Instruction, Ncs, Op, Pair, Ty};

use crate::catalog::{Catalog, Routine};
use crate::vm::{self, Deferred, Fault, Host, ObjectId, Program, Routines, Value};

/// A probe's result.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// Our own routine catalog, in `nwscript.nss`'s form.
const CATALOG: &str = r#"
#define ENGINE_STRUCTURE_0 effect
int TRUE = 1;
int FALSE = 0;
int SEVEN = 7;
// 0
int Record(int nA, int nB = SEVEN, int bC = TRUE);
// 1
void Defer(action aCode);
// 2
vector Scale(vector vIn = [1.0, 2.0, 3.0], float fBy = 2.0f);
// 3
string Name(object oWho = OBJECT_SELF);
// 4
float Half(float f);
"#;

/// Builds a program from instructions and labelled jumps.
#[derive(Default)]
pub struct Asm {
    code: Vec<(Op, Option<&'static str>)>,
    labels: BTreeMap<&'static str, usize>,
}

impl Asm {
    pub fn op(&mut self, op: Op) -> &mut Asm {
        self.code.push((op, None));
        self
    }

    /// A jump (`Op::Jmp(0)` and the like) to `label`.
    pub fn to(&mut self, jump: fn(i32) -> Op, label: &'static str) -> &mut Asm {
        self.code.push((jump(0), Some(label)));
        self
    }

    pub fn label(&mut self, name: &'static str) -> &mut Asm {
        self.labels.insert(name, self.code.len());
        self
    }

    /// The program, jumps resolved.
    pub fn build(&self) -> Ncs {
        let mut ncs = Ncs {
            code: self
                .code
                .iter()
                .map(|(op, _)| Instruction {
                    offset: 0,
                    op: op.clone(),
                })
                .collect(),
        };
        ncs.lay_out();
        let offsets: Vec<u32> = ncs.code.iter().map(|i| i.offset).collect();
        for (at, (_, label)) in self.code.iter().enumerate() {
            let Some(label) = label else { continue };
            let target = self.labels.get(label).map_or(0, |&n| offsets[n]);
            let rel = target as i32 - offsets[at] as i32;
            ncs.code[at].op = match ncs.code[at].op {
                Op::Jmp(_) => Op::Jmp(rel),
                Op::Jsr(_) => Op::Jsr(rel),
                Op::Jz(_) => Op::Jz(rel),
                _ => Op::Jnz(rel),
            };
        }
        ncs
    }
}

/// The probes' host: records each call, returns fixed answers.
#[derive(Default)]
struct Recorder {
    calls: Vec<(String, ObjectId, Vec<Value>)>,
    deferred: Vec<Deferred>,
}

impl Host for Recorder {
    fn action(
        &mut self,
        routine: &Routine,
        caller: ObjectId,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let answer = match routine.name.as_str() {
            "Record" => Value::Int(41),
            "Scale" => match (&args[0], &args[1]) {
                (Value::Vector(v), Value::Float(by)) => Value::Vector(v.map(|c| c * by)),
                _ => return Err("bad arguments".into()),
            },
            "Name" => Value::String(format!("{:?}", args[0]).into()),
            "Half" => match args[0] {
                Value::Float(f) => Value::Float(f / 2.0),
                _ => return Err("bad argument".into()),
            },
            _ => Value::Int(0),
        };
        for arg in &args {
            if let Value::Action(deferred) = arg {
                self.deferred.push((**deferred).clone());
            }
        }
        self.calls.push((routine.name.clone(), caller, args));
        Ok(answer)
    }
}

const CALLER: ObjectId = ObjectId(77);

fn int(v: i32) -> Op {
    Op::Const(Const::Int(v))
}

fn float(v: f32) -> Op {
    Op::Const(Const::Float(v))
}

fn object(v: i32) -> Op {
    Op::Const(Const::Object(v))
}

fn string(v: &str) -> Op {
    Op::Const(Const::String(v.as_bytes().to_vec()))
}

/// Runs a body as a conditional script (`RSADDI; JSR body; RETN`): the
/// body leaves its answer at -4 (`CPDOWNSP -8, 4` of its top) and returns.
fn conditional(body: &[Op]) -> Asm {
    let mut asm = Asm::default();
    asm.op(Op::Rsadd(Ty::Int))
        .to(Op::Jsr, "body")
        .op(Op::Retn)
        .label("body");
    for op in body {
        asm.op(op.clone());
    }
    asm.op(Op::CpDownSp {
        offset: -8,
        size: 4,
    })
    .op(Op::MovSp(-4))
    .op(Op::Retn);
    asm
}

struct Probes {
    routines: Routines,
    checks: Vec<Check>,
}

impl Probes {
    fn run(&self, asm: &Asm) -> (Result<vm::Finished, Fault>, Recorder) {
        let program = Program::new(asm.build());
        let mut host = Recorder::default();
        let outcome = vm::run(&program, &self.routines, CALLER, &mut host);
        (outcome, host)
    }

    fn check(&mut self, name: &'static str, ok: bool, detail: String) {
        self.checks.push(Check { name, ok, detail });
    }

    /// A conditional body's answer is `expected`.
    fn answers(&mut self, name: &'static str, body: &[Op], expected: i32) {
        let (outcome, _) = self.run(&conditional(body));
        let got = outcome.as_ref().map(|f| f.returned);
        self.check(
            name,
            got == Ok(Some(expected)),
            format!("{got:?}, wanted {expected}"),
        );
    }

    /// The program faults with this kind.
    fn faults(&mut self, name: &'static str, asm: &Asm, kind: &str) {
        let (outcome, _) = self.run(asm);
        let got = outcome.as_ref().err().map(Fault::kind);
        self.check(
            name,
            got == Some(kind),
            format!("{outcome:?}, wanted a {kind} fault"),
        );
    }
}

/// Every probe, in order.
pub fn all() -> Vec<Check> {
    let catalog = Catalog::parse(CATALOG).expect("the probes' catalog reads");
    let mut p = Probes {
        routines: Routines::new(std::sync::Arc::new(catalog)),
        checks: Vec::new(),
    };
    use Op::*;
    // Arithmetic, by operand types.
    p.answers(
        "ints wrap",
        &[int(i32::MAX), int(1), Add(Pair::IntInt)],
        i32::MIN,
    );
    p.answers(
        "int arithmetic",
        &[
            int(7),
            int(3),
            Sub(Pair::IntInt),
            int(5),
            Mul(Pair::IntInt),
            int(3),
            Div(Pair::IntInt),
            int(4),
            Mod,
        ],
        2,
    );
    p.answers(
        "mixed float arithmetic, compared",
        &[
            int(3),
            float(0.5),
            Mul(Pair::IntFloat),
            float(1.5),
            Geq(Pair::FloatFloat),
        ],
        1,
    );
    p.answers(
        "float then int",
        &[
            float(5.0),
            int(2),
            Div(Pair::FloatInt),
            float(2.5),
            Equal(Pair::FloatFloat),
        ],
        1,
    );
    p.answers(
        "string concatenation",
        &[
            string("ab"),
            string("cd"),
            Add(Pair::StringString),
            string("abcd"),
            Equal(Pair::StringString),
        ],
        1,
    );
    p.answers(
        "vectors add and scale",
        &[
            float(1.0),
            float(2.0),
            float(3.0),
            float(1.0),
            float(1.0),
            float(1.0),
            Add(Pair::VectorVector),
            float(2.0),
            Mul(Pair::VectorFloat),
            // (4, 6, 8): the struct comparison against it.
            float(4.0),
            float(6.0),
            float(8.0),
            Equal(Pair::Struct(12)),
        ],
        1,
    );
    p.answers("negation and complement", &[int(5), Neg(Ty::Int), Comp], 4);
    p.answers("not", &[int(0), Not, int(9), Not, Add(Pair::IntInt)], 1);
    p.answers(
        "shifts",
        &[
            int(-16),
            int(2),
            ShiftRight,
            int(1),
            ShiftLeft,
            int(-1),
            int(28),
            UShiftRight,
            Add(Pair::IntInt),
        ],
        -8 + 15,
    );
    p.answers(
        "logic and bits",
        &[
            int(2),
            int(0),
            LogOr,
            int(6),
            int(3),
            BoolAnd,
            LogAnd,
            int(5),
            int(3),
            ExcOr,
            IncOr,
        ],
        7,
    );
    p.answers(
        "comparisons",
        &[
            int(1),
            int(2),
            Lt(Pair::IntInt),
            int(2),
            int(2),
            Leq(Pair::IntInt),
            LogAnd,
            float(3.0),
            float(2.0),
            Gt(Pair::FloatFloat),
            LogAnd,
            int(4),
            int(5),
            NotEqual(Pair::IntInt),
            LogAnd,
        ],
        1,
    );
    p.answers(
        "objects compare",
        &[object(0), object(1), Equal(Pair::ObjectObject)],
        0,
    );
    // The stack.
    p.answers(
        "copy down, copy up, destruct",
        &[
            int(1),
            int(2),
            int(3),
            // [1, 2, 3]: copy 3 over 1 → [3, 2, 3].
            CpDownSp {
                offset: -12,
                size: 4,
            },
            // Keep the middle cell of the three: [2].
            Destruct {
                size: 12,
                offset: 4,
                keep: 4,
            },
            CpTopSp {
                offset: -4,
                size: 4,
            },
            Add(Pair::IntInt),
        ],
        4,
    );
    p.answers(
        "increment and decrement",
        &[int(5), IncSp(-4), IncSp(-4), DecSp(-4)],
        6,
    );
    // Globals: two ints below the base pointer, read and bumped.
    {
        let mut asm = Asm::default();
        asm.op(Rsadd(Ty::Int)).op(int(10)).op(int(20)).op(SaveBp)
            .op(IncBp(-4)).op(DecBp(-8))
            // 9 + 21 into the result cell under the globals.
            .op(CpTopBp { offset: -8, size: 4 }).op(CpTopBp { offset: -4, size: 4 })
            .op(Add(Pair::IntInt)).op(CpDownBp { offset: -12, size: 4 }).op(MovSp(-4))
            .op(RestoreBp).op(MovSp(-8)).op(Retn);
        let (outcome, _) = p.run(&asm);
        let got = outcome.as_ref().map(|f| f.returned);
        p.check(
            "globals by the base pointer",
            got == Ok(Some(30)),
            format!("{got:?}"),
        );
    }
    // Control flow.
    {
        let mut asm = conditional(&[]);
        asm.code.truncate(3);
        asm.labels.clear();
        asm.label("body");
        // A loop: counter 0, add 1 until 5, through a subroutine.
        asm.op(int(0))
            .label("loop")
            .op(CpTopSp {
                offset: -4,
                size: 4,
            })
            .op(int(5))
            .op(Lt(Pair::IntInt))
            .to(Jz, "done")
            .to(Jsr, "inc")
            .to(Jmp, "loop")
            .label("done")
            .op(CpTopSp {
                offset: -4,
                size: 4,
            })
            .op(int(5))
            .op(Equal(Pair::IntInt))
            .to(Jnz, "yes")
            .op(int(0))
            .to(Jmp, "end")
            .label("yes")
            .op(int(1))
            .label("end")
            .op(CpDownSp {
                offset: -12,
                size: 4,
            })
            .op(MovSp(-8))
            .op(Retn)
            .label("inc")
            .op(IncSp(-4))
            .op(Retn);
        let (outcome, _) = p.run(&asm);
        let got = outcome.as_ref().map(|f| f.returned);
        p.check(
            "loops, calls, branches",
            got == Ok(Some(1)),
            format!("{got:?}"),
        );
    }
    // Routines: arguments first on top, defaults, extras, vectors.
    {
        let mut asm = Asm::default();
        asm.op(int(3)).op(int(2)).op(int(1))
            // Record(1, 2, 3).
            .op(Action { routine: 0, args: 3 }).op(MovSp(-4))
            // Record(4): the defaults SEVEN and TRUE.
            .op(int(4)).op(Action { routine: 0, args: 1 }).op(MovSp(-4))
            // Record(5, 6, 7) and an extra the prototype lacks.
            .op(int(99)).op(int(7)).op(int(6)).op(int(5)).op(Action { routine: 0, args: 4 }).op(MovSp(-4))
            // Scale() with its default vector; Name() for the caller.
            .op(Action { routine: 2, args: 0 }).op(MovSp(-12))
            .op(Action { routine: 3, args: 0 }).op(MovSp(-4))
            .op(float(9.0)).op(Action { routine: 4, args: 1 })
            .op(float(4.5)).op(Equal(Pair::FloatFloat)).op(Retn);
        let (outcome, host) = p.run(&asm);
        let args = |n: usize| host.calls.get(n).map(|(_, _, args)| args.clone());
        let i = Value::Int;
        p.check(
            "arguments first on top",
            args(0) == Some(vec![i(1), i(2), i(3)]),
            format!("{:?}", args(0)),
        );
        p.check(
            "missing arguments take the defaults",
            args(1) == Some(vec![i(4), i(7), i(1)]),
            format!("{:?}", args(1)),
        );
        p.check(
            "extra arguments are dropped",
            args(2) == Some(vec![i(5), i(6), i(7)]),
            format!("{:?}", args(2)),
        );
        p.check(
            "a vector default",
            args(3) == Some(vec![Value::Vector([1.0, 2.0, 3.0]), Value::Float(2.0)]),
            format!("{:?}", args(3)),
        );
        p.check(
            "OBJECT_SELF is the caller",
            args(4) == Some(vec![Value::Object(CALLER)]),
            format!("{:?}", args(4)),
        );
        let got = outcome.as_ref().map(|f| f.returned);
        p.check(
            "a routine's value is pushed",
            got == Ok(Some(1)),
            format!("{got:?}"),
        );
    }
    // Deferred code: saved globals and locals, resumed later.
    {
        let mut asm = Asm::default();
        asm.op(int(100)).op(SaveBp).op(int(23))
            .op(StoreState { bp: 4, sp: 4 }).to(Jmp, "after")
            // The deferred code: Record(global + local).
            .op(CpTopBp { offset: -4, size: 4 }).op(CpTopSp { offset: -8, size: 4 })
            .op(Add(Pair::IntInt)).op(Action { routine: 0, args: 1 }).op(MovSp(-4)).op(Retn)
            .label("after").op(Action { routine: 1, args: 1 })
            .op(MovSp(-4)).op(RestoreBp).op(MovSp(-4)).op(Retn);
        let ncs = asm.build();
        let program = Program::new(ncs);
        let mut host = Recorder::default();
        let first = vm::run(&program, &p.routines, CALLER, &mut host);
        let saved = host.deferred.first().cloned();
        let later = saved
            .as_ref()
            .map(|deferred| vm::resume(&program, &p.routines, ObjectId(5), deferred, &mut host));
        let record = host.calls.iter().find(|(name, _, _)| name == "Record");
        p.check(
            "deferred code keeps its globals and locals",
            first.is_ok()
                && matches!(later, Some(Ok(_)))
                && record.is_some_and(|(_, caller, args)| {
                    *caller == ObjectId(5) && args.first() == Some(&Value::Int(123))
                }),
            format!("{first:?} {later:?} {record:?}"),
        );
    }
    // Faults.
    p.faults(
        "underflow",
        Asm::default().op(MovSp(-4)).op(Retn),
        "underflow",
    );
    p.faults(
        "a wrong cell type",
        Asm::default().op(float(1.0)).op(Jz(6)).op(Retn),
        "type",
    );
    // A routine's argument of another type: the run stops there, as the
    // executables stop it (TSL's floating mines, `k_fm_die_custom`).
    p.faults(
        "an argument of the wrong type",
        Asm::default()
            .op(object(0))
            .op(Action {
                routine: 4,
                args: 1,
            })
            .op(Retn),
        "argument",
    );
    p.faults(
        "division by zero",
        Asm::default()
            .op(int(1))
            .op(int(0))
            .op(Div(Pair::IntInt))
            .op(Retn),
        "divide by zero",
    );
    p.faults(
        "a jump into nothing",
        Asm::default().op(Jmp(3)).op(Retn),
        "jump",
    );
    p.faults(
        "a script that never ends",
        Asm::default().label("spin").to(Jmp, "spin"),
        "budget",
    );
    // Every probe's program writes back as it reads.
    let sample = conditional(&[int(1), float(2.0), string("x")]).build();
    let again = Ncs::parse(&sample.write());
    p.check(
        "built programs read back",
        again.as_ref() == Ok(&sample),
        format!("{again:?}"),
    );
    p.checks
}

/// What an install's scripts came to (`install`).
#[derive(Debug, Default)]
pub struct InstallReport {
    pub scripts: usize,
    /// Those that failed to decode, wrote back differently, or call a
    /// routine the catalog lacks: name and why.
    pub failures: Vec<String>,
    pub routines: usize,
}

/// Every one of `scripts` (name, bytes) decoded, written back byte for
/// byte, and each call checked against the catalog read from `nwscript`.
pub fn install<'a>(
    scripts: impl IntoIterator<Item = (String, &'a [u8])>,
    nwscript: &str,
) -> InstallReport {
    let mut report = InstallReport::default();
    let catalog = match Catalog::parse(nwscript) {
        Ok(catalog) => catalog,
        Err(error) => {
            report.failures.push(error.to_string());
            return report;
        }
    };
    report.routines = catalog.routines.len();
    for (name, bytes) in scripts {
        report.scripts += 1;
        let ncs = match Ncs::parse(bytes) {
            Ok(ncs) => ncs,
            Err(error) => {
                report.failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        if ncs.write() != bytes {
            report
                .failures
                .push(format!("{name}: written back differently"));
        }
        let unknown = ncs
            .code
            .iter()
            .find_map(|instruction| match instruction.op {
                Op::Action { routine, .. } if catalog.routine_by_id(routine).is_none() => {
                    Some(routine)
                }
                _ => None,
            });
        if let Some(routine) = unknown {
            report
                .failures
                .push(format!("{name}: calls unknown routine {routine}"));
        }
    }
    report
}
