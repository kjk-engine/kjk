//! The VM's probes: programs we write ourselves (no game script), each
//! checked for what it leaves, what it hands the host, or how it faults.
//! `kjk script probes` prints them; the approved test `script_vm` needs
//! every one to pass.

use fmt_odyssey::ncs::{Const, Ncs, Op, Pair, Ty};

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

pub use crate::asm::Asm;

/// The catalog the built scripts' probes call: real routines' prototypes,
/// in `nwscript.nss`'s form, their numbers their order here.
const BUILT_CATALOG: &str = r#"
int TRUE = 1;
int FALSE = 0;
int CONVERSATION_TYPE_CINEMATIC = 0;
// 0
object GetFirstPC();
// 1
object GetObjectByTag(string sTag, int nNth = 0);
// 2
void AssignCommand(object oActionSubject, action aActionToAssign);
// 3
void ActionStartConversation(object oObjectToConverse, string sDialogResRef = "", int bPrivateConversation = FALSE, int nConversationType = CONVERSATION_TYPE_CINEMATIC, int bIgnoreStartRange = FALSE, float fDelay = 0.5f);
// 4
object CreateItemOnObject(string sItemTemplate, object oTarget = OBJECT_SELF, int nStackSize = 1);
// 5
void DelayCommand(float fSeconds, action aActionToDelay);
"#;

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
            // Any other routine: the empty value of its return type.
            _ => Value::empty(routine.ret),
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

    fn run_ncs(&self, ncs: Ncs) -> (Result<vm::Finished, Fault>, Recorder) {
        let program = Program::new(ncs);
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
    built(&mut p);
    compiled(&mut p);
    decompiled(&mut p);
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

/// Scripts built from calls by name (`asm::script`), as `kjk original` puts
/// them in an original game's `override`: each written as the games'
/// compiler writes it, and run here.
fn built(p: &mut Probes) {
    use crate::asm::{self, Expr, action, call, constant, not, text, when};
    use Op::*;
    let catalog = Catalog::parse(BUILT_CATALOG).expect("the built probes' catalog reads");
    let routines = Routines::new(std::sync::Arc::new(catalog.clone()));
    let ops = |ncs: &Ncs| ncs.code.iter().map(|i| i.op.clone()).collect::<Vec<Op>>();
    let names = |host: &Recorder| {
        host.calls
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect::<Vec<_>>()
    };
    // A call: its defaults written, the last argument pushed first, its
    // value popped.
    let glow = asm::script(
        &catalog,
        &[call("CreateItemOnObject", [text("g_i_glowrod01")])],
    );
    let wanted = [
        Jsr(8),
        Retn,
        int(1),
        object(0),
        string("g_i_glowrod01"),
        Action {
            routine: 4,
            args: 3,
        },
        MovSp(-4),
        Retn,
    ];
    p.check(
        "a built call writes its defaults and pops its value",
        glow.as_ref().map(ops) == Ok(wanted.to_vec()),
        format!("{glow:?}"),
    );
    // `AssignCommand` with an action: `STORESTATE 0, 0` and a jump over its
    // code; run, then its saved code resumed for the one assigned.
    let assigned = asm::script(
        &catalog,
        &[call(
            "AssignCommand",
            [
                call("GetObjectByTag", [text("Carth")]),
                action([call(
                    "ActionStartConversation",
                    [call("GetFirstPC", []), text("banter")],
                )]),
            ],
        )],
    );
    let laid_out = assigned.as_ref().is_ok_and(|ncs| {
        let code = ops(ncs);
        code.iter()
            .position(|op| *op == StoreState { bp: 0, sp: 0 })
            .is_some_and(|at| {
                let Jmp(over) = code[at + 1] else {
                    return false;
                };
                let landing = ncs.code[at + 1].offset as i32 + over;
                ncs.at(landing as u32).is_some_and(|to| {
                    code[to - 1] == Retn
                        && code[to..]
                            == [
                                int(0),
                                string("Carth"),
                                Action {
                                    routine: 1,
                                    args: 2,
                                },
                                Action {
                                    routine: 2,
                                    args: 2,
                                },
                                Retn,
                            ]
                })
            })
    });
    p.check(
        "a built action is stored state and a jump over its code",
        laid_out,
        format!("{assigned:?}"),
    );
    let (now, later) = match &assigned {
        Ok(ncs) => {
            let program = Program::new(ncs.clone());
            let mut host = Recorder::default();
            let first = vm::run(&program, &routines, CALLER, &mut host);
            let mut then = Recorder::default();
            let resumed = host
                .deferred
                .first()
                .map(|saved| vm::resume(&program, &routines, ObjectId(5), saved, &mut then));
            let fine = first.is_ok() && matches!(resumed, Some(Ok(_)));
            (
                fine.then(|| names(&host)),
                fine.then(|| (names(&then), then.calls)),
            )
        }
        Err(_) => (None, None),
    };
    let talk = later.as_ref().and_then(|(_, calls)| calls.get(1));
    p.check(
        "a built action runs later for the one assigned",
        now.as_deref() == Some(&["GetObjectByTag".to_string(), "AssignCommand".to_string()][..])
            && later.as_ref().map(|(names, _)| names.as_slice())
                == Some(
                    &[
                        "GetFirstPC".to_string(),
                        "ActionStartConversation".to_string(),
                    ][..],
                )
            && talk.is_some_and(|(_, _, args)| {
                args.get(1) == Some(&Value::String("banter".into()))
                    && args.get(5) == Some(&Value::Float(0.5))
            }),
        format!("{now:?} {later:?}"),
    );
    // `if` and `not`: the body runs only where the condition holds.
    let ran = |condition: Expr| {
        let ncs = asm::script(
            &catalog,
            &[
                when(
                    condition,
                    [call("CreateItemOnObject", [text("g_i_glowrod01")])],
                ),
                call("GetFirstPC", []),
            ],
        )
        .ok()?;
        let mut host = Recorder::default();
        vm::run(&Program::new(ncs), &routines, CALLER, &mut host).ok()?;
        Some(names(&host).len())
    };
    let counts = [
        ran(Expr::Int(0)),
        ran(Expr::Int(1)),
        ran(not(Expr::Int(1))),
        ran(not(Expr::Int(0))),
    ];
    p.check(
        "a built if runs its body where its condition holds, and not turns it",
        counts == [Some(1), Some(2), Some(1), Some(2)],
        format!("{counts:?}"),
    );
    // A conditional script answering a constant.
    let answer = asm::conditional(&catalog, &constant("TRUE"));
    let wanted = [
        Rsadd(Ty::Int),
        Jsr(8),
        Retn,
        int(1),
        CpDownSp {
            offset: -8,
            size: 4,
        },
        MovSp(-4),
        Retn,
    ];
    p.check(
        "a built conditional answers its constant",
        answer.as_ref().map(ops) == Ok(wanted.to_vec()),
        format!("{answer:?}"),
    );
    // Wrong types and unknown names are refused while building.
    let wrong = asm::script(&catalog, &[call("GetObjectByTag", [Expr::Int(3)])]);
    p.check(
        "a built call of a wrong type or unknown name is refused",
        wrong.as_ref().err().map(String::as_str)
            == Some("GetObjectByTag: argument 1 (sTag) is string, given int")
            && asm::script(&catalog, &[call("NoSuch", [])]).is_err()
            && asm::script(&catalog, &[call("DelayCommand", [Expr::Float(1.0)])]).is_err(),
        format!("{wrong:?}"),
    );
}

/// Compiled scripts read back into source (`decompile`): the source
/// compiled again is the same program, byte for byte.
fn decompiled(p: &mut Probes) {
    use crate::compile::compile;
    use crate::decompile::{decompile, decompile_checked};
    let catalog = p.routines.catalog().clone();
    let none = |_: &str| -> Option<Vec<u8>> { None };
    let text = |source: &[u8]| -> String { source.iter().map(|&byte| char::from(byte)).collect() };

    // Every statement the compiler writes, with globals, a structure, a
    // vector and functions of its own.
    let source = "int nBase = 3;\n\
         float fHalf = 0.5;\n\
         struct pair { int a; string s; };\n\
         int Twice(int n) { return n * 2; }\n\
         int Inner() { return nBase; }\n\
         void Note(int n, string s) { if (n > 2) { Record(n); return; } Record(0 - n); }\n\
         vector Up(float f) { vector v = [0.0, 0.0, 1.0]; v.z = f; return v; }\n\
         struct pair Made(int n) { struct pair q; q.a = n; q.s = \"x\"; return q; }\n\
         int StartingConditional()\n\
         {\n\
             int nSum = 0;\n\
             int i;\n\
             for (i = 0; i < 5; i++) {\n\
                 if (i == 3) continue;\n\
                 nSum += i;\n\
             }\n\
             struct pair p = Made(4);\n\
             switch (p.a) {\n\
                 case 2: nSum = 0; break;\n\
                 case 4: { int k = Twice(Inner()); nSum += k; } break;\n\
                 default: nSum = -1;\n\
             }\n\
             int j = 0;\n\
             while (TRUE) {\n\
                 j++;\n\
                 if (j >= 3) break;\n\
             }\n\
             do { --j; } while (j > 0 && nSum != 7);\n\
             vector v = Up(fHalf);\n\
             if (j == 0 && nSum > 10 || v.z > 1.0) { Note(nSum, p.s + \"y\"); }\n\
             else if (!j) nSum = ~nSum % 5;\n\
             Defer(Record(nSum + 1));\n\
             return nSum + Record(1) - 41;\n\
         }\n";
    let first = compile("probe", source.as_bytes(), &none, &catalog);
    let back = first.as_ref().map_err(ToString::to_string).and_then(|ncs| {
        decompile_checked("probe", ncs, &catalog).map_err(|error| error.to_string())
    });
    p.check(
        "a decompiled script compiles to the bytes it came from",
        back.is_ok(),
        format!("{:?}", back.as_ref().map(|done| text(&done.source))),
    );
    let read = back
        .map(|done| text(&done.source))
        .unwrap_or_else(|error| error);
    let has = [
        "struct ",
        "vector ",
        "for (",
        "switch (",
        "do {",
        "while (",
        "continue;",
        "Defer(Record(",
    ];
    p.check(
        "a decompiled script keeps its loops, its switch, its structure and its vector",
        has.iter().all(|part| read.contains(part)),
        read.clone(),
    );

    // A function is written where its call is: an argument's before the
    // function it is an argument of.
    let order = decompile(
        &compile(
            "probe",
            b"int Inner() { return 1; }\nint Outer(int n) { return n + 1; }\n\
              void main() { Record(Outer(Inner())); }\n",
            &none,
            &catalog,
        )
        .expect("the probe compiles"),
        &catalog,
    )
    .map(|done| text(&done.source));
    let inner_first = order.as_ref().is_ok_and(|read| {
        match (
            read.find("int sub1() {"),
            read.find("int sub2(int nParam1) {"),
        ) {
            (Some(inner), Some(outer)) => inner < outer,
            _ => false,
        }
    });
    p.check(
        "an argument's function is written before the function it is passed to",
        inner_first,
        format!("{order:?}"),
    );

    // A script compiled when a routine had fewer parameters: it is read
    // back with the routine named, and compiles the same against a catalog
    // cut the same way.
    let old = catalog.cut(&[("Record".to_string(), 1)]);
    let older = compile("probe", b"void main()\n{\n    Record(5);\n}\n", &none, &old);
    let found = older
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|ncs| {
            decompile_checked("probe", ncs, &catalog).map_err(|error| error.to_string())
        })
        .map(|done| done.routines);
    p.check(
        "a script compiled against an older catalog names the routine and its arguments",
        found == Ok(vec![("Record".to_string(), 1)]),
        format!("{found:?}"),
    );
}

/// Scripts compiled from source (`compile`), with an include of our own,
/// against the probes' catalog: each run here, and one laid out instruction
/// by instruction as the games' compiler lays it out.
fn compiled(p: &mut Probes) {
    use crate::compile::compile;
    use Op::*;
    let catalog = p.routines.catalog().clone();
    let include = |name: &str| {
        (name == "probe_inc").then(|| {
            b"int nBase = 3;\n\
              struct pair { int a; int b; };\n\
              int Twice(int n) { return n * 2; }\n"
                .to_vec()
        })
    };
    let build = |source: &str| compile("probe", source.as_bytes(), &include, &catalog);

    // A call: its defaults written, the last argument first, its value
    // popped.
    let call = build("void main()\n{\n    Record(5);\n}\n");
    let wanted = [
        Jsr(8),
        Retn,
        int(1),
        int(7),
        int(5),
        Action {
            routine: 0,
            args: 3,
        },
        MovSp(-4),
        Retn,
    ];
    p.check(
        "a compiled call writes its defaults and pops its value",
        call.as_ref()
            .map(|ncs| ncs.code.iter().map(|i| i.op.clone()).collect::<Vec<_>>())
            == Ok(wanted.to_vec()),
        format!("{call:?}"),
    );

    // Loops, a switch, a struct, a global from the include and a function
    // of it, run to their answer: 0 + 1 + 2 + 4, then 2 * 3, then 5.
    let answer = build(
        "#include \"probe_inc\"\n\
         int StartingConditional()\n\
         {\n\
             int nSum = 0;\n\
             int i;\n\
             for (i = 0; i < 5; i++) {\n\
                 if (i == 3) continue;\n\
                 nSum += i;\n\
             }\n\
             struct pair p;\n\
             p.a = 4;\n\
             p.b = 5;\n\
             switch (p.a) {\n\
                 case 2: nSum = 0; break;\n\
                 case 4: nSum += Twice(nBase); break;\n\
                 default: nSum = -1;\n\
             }\n\
             int j = 0;\n\
             while (TRUE) {\n\
                 j++;\n\
                 if (j >= 3) break;\n\
             }\n\
             if (j == 3 && nSum > 10 || FALSE) nSum += p.b;\n\
             return nSum + Record(1) - 41;\n\
         }\n",
    );
    let got = answer
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|ncs| {
            let (outcome, host) = p.run_ncs(ncs.clone());
            outcome
                .map(|finished| (finished.returned, host.calls.len()))
                .map_err(|fault| format!("{fault:?}"))
        });
    p.check(
        "a compiled conditional with loops, a switch and an include answers",
        got == Ok((Some(18), 1)),
        format!("{got:?}, wanted 18 after one call"),
    );

    // An action argument runs later, its locals saved with it.
    let later = build("void main()\n{\n    int n = 2;\n    Defer(Record(n + 1));\n}\n");
    let resumed = later.as_ref().ok().and_then(|ncs| {
        let program = Program::new(ncs.clone());
        let mut host = Recorder::default();
        vm::run(&program, &p.routines, CALLER, &mut host).ok()?;
        let saved = host.deferred.first()?;
        let mut then = Recorder::default();
        vm::resume(&program, &p.routines, CALLER, saved, &mut then).ok()?;
        Some(
            then.calls
                .into_iter()
                .map(|(name, _, args)| (name, args))
                .collect::<Vec<_>>(),
        )
    });
    p.check(
        "a compiled action argument runs later with its locals",
        resumed
            == Some(vec![(
                "Record".to_string(),
                vec![Value::Int(3), Value::Int(7), Value::Int(1)],
            )]),
        format!("{resumed:?}"),
    );

    // A mistake is named with its line.
    let wrong = build("void main()\n{\n    Record(nNothing);\n}\n");
    p.check(
        "a compiled script's mistake names its line",
        wrong
            .as_ref()
            .err()
            .is_some_and(|error| error.line == 3 && error.what.contains("nNothing")),
        format!("{wrong:?}"),
    );

    // Each operator takes only the pairs of types the games' scripts hold.
    // Every legal pair here runs to its answer, one bit of 255 each.
    let legal = build(
        "int StartingConditional()\n\
         {\n\
             int n = 2;\n\
             float f = 1.5;\n\
             vector v = [1.0, 2.0, 3.0];\n\
             string s = \"a\";\n\
             object o = OBJECT_SELF;\n\
             vector a = v * f;\n\
             vector b = f * v;\n\
             vector c = v / f;\n\
             vector d = v + v - v;\n\
             float g = n + f;\n\
             float h = f * n;\n\
             string t = s + s;\n\
             int r = 0;\n\
             if (a == b) r += 1;\n\
             if (c != v) r += 2;\n\
             if (d == v) r += 4;\n\
             if (g > 3.0) r += 8;\n\
             if (h <= 3.0) r += 16;\n\
             if (t == \"aa\") r += 32;\n\
             if (o == OBJECT_SELF) r += 64;\n\
             if (n >= 2) r += 128;\n\
             return r;\n\
         }\n",
    );
    let got = legal.as_ref().map_err(ToString::to_string).and_then(|ncs| {
        let (outcome, _) = p.run_ncs(ncs.clone());
        outcome
            .map(|finished| finished.returned)
            .map_err(|fault| format!("{fault:?}"))
    });
    p.check(
        "every legal pair of operand types compiles and runs",
        got == Ok(Some(255)),
        format!("{got:?}, wanted 255"),
    );

    // Any other pair is a mistake named when the script is compiled. Before,
    // it compiled and faulted when it ran.
    let refused = [
        "string r = s - s;",
        "string r = s * s;",
        "object r = o + o;",
        "struct pair r = p + p;",
        "vector r = v + f;",
        "vector r = v - f;",
        "vector r = f / v;",
        "int r = s < s;",
        "int r = o > o;",
        "int r = v <= v;",
        "int r = p >= p;",
        "int r = n < f;",
        "int r = f >= n;",
        "int r = n == f;",
        "int r = f != n;",
        "int r = v == f;",
        "int r = n % f;",
    ];
    let compiled: Vec<&str> = (refused.iter())
        .filter(|line| {
            let source = format!(
                "#include \"probe_inc\"\n\
                 void main()\n\
                 {{\n\
                     int n; float f; vector v; string s; object o; struct pair p;\n\
                     {line}\n\
                 }}\n"
            );
            !build(&source).is_err_and(|error| error.what.contains("does not take"))
        })
        .copied()
        .collect();
    p.check(
        "an operator refuses a pair of operand types the games never use",
        compiled.is_empty(),
        format!("compiled, or refused for another reason: {compiled:?}"),
    );

    // Globals: a function that only a global's value calls is written, a
    // global's value reads an earlier global, and a `const` with a minus
    // sign is a constant, which takes no place among the globals. The
    // answer is 3 + (3 + 1) - 1.
    let globals = build(
        "const int NONE = -1;
         int Three() { return 3; }
         int nFirst = Three();
         int nSecond = nFirst + 1;
         int StartingConditional()
         {
             return nFirst + nSecond + NONE;
         }
",
    );
    let got = globals
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|ncs| {
            // Two globals are set: one place reserved for each.
            let reserved = (ncs.code.iter())
                .take_while(|i| i.op != SaveBp)
                .filter(|i| i.op == Rsadd(Ty::Int))
                .count();
            let (outcome, _) = p.run_ncs(ncs.clone());
            outcome
                .map(|finished| (finished.returned, reserved))
                .map_err(|fault| format!("{fault:?}"))
        });
    p.check(
        "globals set from a function and from each other, beside a negative constant",
        // The conditional's own answer is reserved first, then the two
        // globals, then the place for what `Three` returns.
        got == Ok((Some(6), 4)),
        format!("{got:?}, wanted 6 with 4 places reserved"),
    );
    let early = build(
        "int nFirst = nSecond;
int nSecond = 1;
void main()
{
}
",
    );
    p.check(
        "a global's value cannot read a global set further down",
        early
            .as_ref()
            .is_err_and(|error| error.what.contains("nSecond is set further down")),
        format!("{early:?}"),
    );

    // A case of a switch has the switch's own type.
    let cases = ["case \"one\":", "case [1.0, 2.0, 3.0]:", "case 1.0:"];
    let compiled: Vec<&str> = (cases.iter())
        .filter(|case| {
            let source = format!(
                "void main()
{{
    int n = 1;
    switch (n) {{
        {case} break;
    }}
}}
"
            );
            !build(&source).is_err_and(|error| error.what.contains("a case of this switch is a"))
        })
        .copied()
        .collect();
    p.check(
        "a switch refuses a case of another type",
        compiled.is_empty(),
        format!("compiled, or refused for another reason: {compiled:?}"),
    );

    // A continue and a break pop the locals of the blocks they leave: a
    // continue in a loop's body that has a local, a continue in a switch in
    // a loop, and a break out of a switch from a block that has a local.
    // The sums are 0 + 2 + 6 + 8, then 3 and 30 more, then 1 more, and 7.
    let leaving = "int StartingConditional()
         {
             int nSum = 0;
             int i;
             for (i = 0; i < 5; i++) {
                 int n = i * 2;
                 if (n == 4) continue;
                 nSum += n;
             }
             for (i = 0; i < 4; i++) {
                 switch (i) {
                     case 1: { int nSkip = 100; continue; }
                     default: nSum += 1;
                 }
                 nSum += 10;
             }
             switch (nSum) {
                 case 49: { int nOne = 1; nSum += nOne; break; }
                 default: nSum = -1;
             }
             int nAfter = 7;
             return nSum + nAfter;
         }
";
    let answer = |built: Result<Ncs, crate::compile::Error>| {
        built.map_err(|error| error.to_string()).and_then(|ncs| {
            let (outcome, _) = p.run_ncs(ncs);
            outcome
                .map(|finished| finished.returned)
                .map_err(|fault| format!("{fault:?}"))
        })
    };
    let clean = answer(build(leaving));
    // BioWare's compiler pops nothing there, and the same source goes wrong.
    let as_bioware = answer(crate::compile::compile_as_bioware(
        "probe",
        leaving.as_bytes(),
        &include,
        &catalog,
    ));
    p.check(
        "a continue and a break pop the locals of the blocks they leave",
        clean == Ok(Some(57)) && as_bioware != Ok(Some(57)),
        format!("{clean:?}, wanted 57; laid out as BioWare's compiler does: {as_bioware:?}"),
    );

    // Mistakes in what a script declares, each named: a struct that holds
    // itself, a struct that is not declared (as a local and as what a
    // function returns), a return with no value in a function that has
    // one, a function with two bodies, and a body whose parameters are not
    // its prototype's.
    let mistakes = [
        (
            "struct a { struct a next; };\nvoid main()\n{\n}\n",
            "the struct a's next: the struct a is not declared",
        ),
        (
            "void main()\n{\n    struct missing m;\n}\n",
            "the struct missing is not declared",
        ),
        (
            "struct missing Make();\nvoid main()\n{\n}\n",
            "Make: the struct missing is not declared",
        ),
        (
            "int Three()\n{\n    return;\n}\nvoid main()\n{\n    Three();\n}\n",
            "its return needs a value",
        ),
        (
            "int Three() { return 3; }\nint Three() { return 4; }\nvoid main()\n{\n}\n",
            "Three is defined twice",
        ),
        (
            "int Three(int n);\nint Three(float f) { return 3; }\nvoid main()\n{\n}\n",
            "Three is declared twice, with other parameters the second time",
        ),
    ];
    let unnamed: Vec<String> = (mistakes.iter())
        .filter_map(|(source, wanted)| match build(source) {
            Err(error) if error.what.contains(wanted) => None,
            other => Some(format!("{wanted:?}: {other:?}")),
        })
        .collect();
    // A prototype and its body that agree still compile, and the body keeps
    // the prototype's default. What a body returns may differ from its
    // prototype, as in KOTOR 1's `k_inc_ebonhawk`, and the body's is taken.
    let agreed = build(
        "int Twofold(int n = 2);\n\
         void Counted(int n);\n\
         int Twofold(int n) { return n * 2; }\n\
         int Counted(int n) { return n; }\n\
         int StartingConditional()\n\
         {\n\
             return Twofold() + Counted(0);\n\
         }\n",
    );
    let agreed = agreed.map_err(|error| error.to_string()).and_then(|ncs| {
        let (outcome, _) = p.run_ncs(ncs);
        outcome
            .map(|finished| finished.returned)
            .map_err(|fault| format!("{fault:?}"))
    });
    p.check(
        "a mistake in a struct, a return or a function's two declarations is named",
        unnamed.is_empty() && agreed == Ok(Some(4)),
        format!("not named: {unnamed:?}; a prototype with its body: {agreed:?}, wanted 4"),
    );

    // A `#define` is a constant when its value is one number or one
    // string: 2 - 3, the smallest int whole, and a string. One of another
    // kind is no constant, and is named where a script uses it. Unused, it
    // is no mistake.
    let defined = build(
        "#define TWO 2\n\
         #define LESS -3\n\
         #define LEAST -2147483648\n\
         #define NAME \"abc\"\n\
         #define UNUSED 1 + 2\n\
         int StartingConditional()\n\
         {\n\
             string s = NAME;\n\
             if (s == \"abc\" && LEAST < 0) return TWO + LESS;\n\
             return 0;\n\
         }\n",
    );
    let defined = defined.map_err(|error| error.to_string()).and_then(|ncs| {
        let least = ncs.code.iter().any(|i| i.op == int(i32::MIN));
        let (outcome, _) = p.run_ncs(ncs);
        outcome
            .map(|finished| (finished.returned, least))
            .map_err(|fault| format!("{fault:?}"))
    });
    let unread: Vec<&str> = ["SUM 1 + 2", "SUM -\"abc\"", "SUM", "SUM effect"]
        .into_iter()
        .filter(|define| {
            let source = format!("#define {define}\nvoid main()\n{{\n    Record(SUM);\n}}\n");
            !build(&source).is_err_and(|error| error.what.contains("SUM is a #define that is not"))
        })
        .collect();
    p.check(
        "a #define is one number or one string, and any other is named where it is used",
        defined == Ok((Some(-1), true)) && unread.is_empty(),
        format!("{defined:?}, wanted -1 with the smallest int whole; read as a value: {unread:?}"),
    );

    // A debugger's view: the compiler's notes read back as written, and a
    // run watched line by line (`vm::observe`) shows a function's parameter
    // at each call, read where the notes put it.
    let source = "int Twice(int n)\n{\n    return n * 2;\n}\n\
                  void main()\n{\n    int i;\n    for (i = 0; i < 3; i++) {\n        Record(Twice(i));\n    }\n}\n";
    let debugged = crate::compile::compile_debug("probe", source.as_bytes(), &include, &catalog);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<i32>::new()));
    let noted = match &debugged {
        Ok((ncs, debug)) => {
            let again = crate::compile::Debug::parse(&debug.write());
            let program = Program::new(ncs.clone());
            let watched = debug.clone();
            let into = seen.clone();
            let watching = vm::observing(Box::new(move |watch| {
                // At the start of Twice's line 3: its n.
                if watched.line_at(watch.offset) == Some(("probe", 3))
                    && let (Some((start, _)), Some(n)) = (
                        watch.frames.last(),
                        watched.locals.iter().find(|local| local.name == "n"),
                    )
                {
                    let at = *start as i64 + i64::from(n.home) / 4;
                    if let Some(Value::Int(value)) =
                        usize::try_from(at).ok().and_then(|at| watch.stack.get(at))
                    {
                        into.borrow_mut().push(*value);
                    }
                }
            }));
            let mut host = Recorder::default();
            let ran = vm::run(&program, &p.routines, CALLER, &mut host);
            drop(watching);
            again.as_ref() == Ok(debug) && ran.is_ok()
        }
        Err(_) => false,
    };
    let values = seen.borrow().clone();
    p.check(
        "a compiled script's debug notes read back, and a watched run shows a parameter at each call",
        noted && values == [0, 1, 2],
        format!("{:?}, the parameter {values:?}", debugged.as_ref().map(|(_, debug)| &debug.lines)),
    );
    // A debugger reads a global from where the globals end. A script that
    // starts at StartingConditional keeps its answer below its globals, so
    // counted from the stack's bottom its first global read as that answer.
    let source = "int nFirst = 11;\nint nSecond = 22;\n\
                  int StartingConditional()\n{\n    return nFirst + nSecond;\n}\n";
    let debugged = crate::compile::compile_debug("probe", source.as_bytes(), &include, &catalog);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, i32)>::new()));
    let ran = match &debugged {
        Ok((ncs, debug)) => {
            let again = crate::compile::Debug::parse(&debug.write());
            let program = Program::new(ncs.clone());
            let watched = debug.clone();
            let into = seen.clone();
            let watching = vm::observing(Box::new(move |watch| {
                // At the start of the conditional's line 5.
                if watched.line_at(watch.offset) != Some(("probe", 5)) || !into.borrow().is_empty()
                {
                    return;
                }
                for (name, _, home) in &watched.globals {
                    let at = watched.global_cell(*home, watch.bp);
                    if let Some(Value::Int(value)) =
                        usize::try_from(at).ok().and_then(|at| watch.stack.get(at))
                    {
                        into.borrow_mut().push((name.clone(), *value));
                    }
                }
            }));
            let mut host = Recorder::default();
            let ran = vm::run(&program, &p.routines, CALLER, &mut host);
            drop(watching);
            again.as_ref() == Ok(debug) && ran.map(|finished| finished.returned) == Ok(Some(33))
        }
        Err(_) => false,
    };
    let mut values = seen.borrow().clone();
    values.sort();
    p.check(
        "a watched conditional shows each global, read from where the globals end",
        ran && values == [("nFirst".to_string(), 11), ("nSecond".to_string(), 22)],
        format!("{values:?}, wanted nFirst 11 and nSecond 22"),
    );
    // A damaged script is refused by the decompiler with a reason, where
    // it could stop the program before: a stack move by the smallest int, a
    // run of 2000 steps, blocks nested 70 deep, and globals followed by a
    // plain jump where the entry's call belongs.
    let damaged = |ops: &[Op]| {
        let mut asm = Asm::default();
        asm.to(Op::Jsr, "body").op(Op::Retn).label("body");
        for op in ops {
            asm.op(op.clone());
        }
        asm.op(Op::Retn);
        asm.build()
    };
    let mut steps = vec![Op::Rsadd(Ty::Int)];
    steps.extend(std::iter::repeat_n(Op::IncSp(-4), 2000));
    steps.push(Op::MovSp(-4));
    // 70 ifs in each other, laid out as the compiler lays an if out. The
    // compiler itself stops a source at 63.
    let mut nested = Asm::default();
    nested.to(Op::Jsr, "body").op(Op::Retn).label("body");
    for deep in 0..70 {
        nested.op(int(1)).to(Op::Jz, &format!("past {deep}"));
    }
    for deep in (0..70).rev() {
        let past = format!("past {deep}");
        nested.to(Op::Jmp, &past).label(&past);
    }
    nested.op(Op::Retn);
    let mut jumped = Asm::default();
    jumped
        .to(Op::Jsr, "globals")
        .op(Op::Retn)
        .label("globals")
        .op(Op::Rsadd(Ty::Int))
        .op(Op::SaveBp)
        .to(Op::Jmp, "entry")
        .op(Op::RestoreBp)
        .op(Op::MovSp(-4))
        .op(Op::Retn)
        .label("entry")
        .op(Op::Retn);
    let cases = [
        (
            "a stack move is larger than a stack",
            damaged(&[Op::MovSp(i32::MIN)]),
        ),
        (
            "its steps follow each other further than a script's do",
            damaged(&steps),
        ),
        (
            "its blocks are nested deeper than a script's are",
            nested.build(),
        ),
        (
            "the globals are not followed by the entry's call",
            jumped.build(),
        ),
    ];
    let unrefused: Vec<String> = (cases.into_iter())
        .filter_map(|(wanted, ncs)| {
            let read = crate::decompile::decompile(&ncs, &catalog).map_err(|error| error.what);
            match read {
                Err(why) if why.contains(wanted) => None,
                other => Some(format!(
                    "{wanted:?}: {:?}",
                    other.map(|decompiled| decompiled.source.len())
                )),
            }
        })
        .collect();
    // The compiler has limits of its own, for a source that would
    // otherwise run the program's stack out: an `if` with its block 64 deep,
    // an expression in 40 pairs of brackets, and 129 operators in a row.
    // At 63 deep, in 20 pairs and with 128 operators, each compiles. The
    // deepest source that compiles also reads back from its compiled form.
    let blocks = |deep: usize| {
        format!(
            "void main()\n{{\n{}    Record(1);\n{}}}\n",
            "    if (TRUE) {\n".repeat(deep),
            "    }\n".repeat(deep)
        )
    };
    let brackets = |deep: usize| {
        format!(
            "void main()\n{{\n    int n = {}1{};\n}}\n",
            "(".repeat(deep),
            ")".repeat(deep)
        )
    };
    let row = |long: usize| {
        format!(
            "void main()\n{{\n    int n = 1{};\n}}\n",
            " + 1".repeat(long)
        )
    };
    let limits = [
        (blocks(63), None),
        (blocks(64), Some("statements are nested more than 128 deep")),
        (brackets(20), None),
        (
            brackets(40),
            Some("an expression is nested more than 32 deep"),
        ),
        (row(128), None),
        (row(129), Some("more than 128 operators in a row")),
    ];
    let unlimited: Vec<String> = (limits.iter())
        .filter_map(|(source, wanted)| match (build(source), wanted) {
            (Ok(_), None) => None,
            (Err(error), Some(wanted)) if error.what.contains(wanted) => None,
            (other, wanted) => Some(format!(
                "{wanted:?}: {:?}",
                other.map(|ncs| ncs.code.len()).map_err(|error| error.what)
            )),
        })
        .collect();
    let deepest = build(&blocks(63))
        .map_err(|error| error.to_string())
        .and_then(|ncs| {
            crate::decompile::decompile_checked("probe", &ncs, &catalog)
                .map(|_| ())
                .map_err(|error| error.what)
        });
    p.check(
        "the compiler refuses a source nested or strung out past its limits",
        unlimited.is_empty() && deepest == Ok(()),
        format!("not as wanted: {unlimited:?}; the deepest read back: {deepest:?}"),
    );
    p.check(
        "the decompiler refuses a damaged script with a reason",
        unrefused.is_empty(),
        format!("not refused, or for another reason: {unrefused:?}"),
    );
    // A string with bytes that print nothing, and a vector with a negative
    // part, read back from their compiled form and compile to it again. The
    // string holds a carriage return, the byte 1, and the letter 233 of the
    // games' code page, which stays a letter.
    let mut source = b"void main()\n{\n    string s = \"a\\rb\\x01c".to_vec();
    source.push(0xe9);
    source.extend_from_slice(b"d\";\n    vector v = [-1.0, 2.0, -0.5];\n}\n");
    let odd = compile("probe", &source, &include, &catalog);
    let held = odd.as_ref().is_ok_and(|ncs| {
        (ncs.code.iter())
            .any(|i| i.op == Op::Const(fmt_odyssey::ncs::Const::String(b"a\rb\x01c\xe9d".to_vec())))
    });
    let read = odd.map_err(|error| error.to_string()).and_then(|ncs| {
        crate::decompile::decompile_checked("probe", &ncs, &catalog)
            .map(|decompiled| decompiled.source)
            .map_err(|error| error.what)
    });
    let has = |part: &[u8]| {
        (read.as_ref()).is_ok_and(|source| source.windows(part.len()).any(|window| window == part))
    };
    p.check(
        "a string's unprintable bytes and a vector's negative parts read back",
        held && has(b"a\\rb\\x01c\xe9d") && has(b"[-1.0, 2.0, -0.5]"),
        format!(
            "the string compiled whole: {held}; read back: {:?}",
            read.as_ref()
                .map(|source| String::from_utf8_lossy(source).to_string())
        ),
    );
    // What nwscript.nss says of a routine: the comment over its prototype.
    let notes = crate::catalog::notes(
        "// 3: Get the name.\n// * Returns \"\".\nstring Name(object oWho);\n",
    );
    p.check(
        "a routine's notes are the comment over its prototype, its number left out",
        notes.get("Name").map(String::as_str) == Some("Get the name.\n* Returns \"\"."),
        format!("{notes:?}"),
    );
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
