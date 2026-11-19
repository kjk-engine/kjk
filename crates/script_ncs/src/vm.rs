//! The NWScript VM: runs a compiled script (`fmt_odyssey::ncs`) for a
//! caller, handing each `ACTION` to the host (the sim's engine routines).
//!
//! - **The stack** holds cells, one value each (four bytes in the
//!   bytecode's offsets): int, float, string, object, or an engine value
//!   (effect, event, location, talent) the host keeps behind a handle. A
//!   vector is three float cells.
//! - **The base pointer** marks where the globals end. `SAVEBP` pushes the
//!   old mark and sets it to the stack's height below that cell;
//!   `RESTOREBP` pops it. `CPTOPBP`, `CPDOWNBP`, `INCIBP`, `DECIBP` address
//!   the globals from it.
//! - **Calls**: `JSR` pushes the next instruction on a return stack; a
//!   `RETN` with nothing on it ends the run. The value left on top, an
//!   int, is a conditional script's answer.
//! - **Engine routines**: `ACTION n, k` pops `k` arguments, first
//!   parameter on top, typed by the routine's prototype (a vector three
//!   cells). Missing trailing ones take the prototype's defaults, and
//!   extras (scripts compiled against other prototypes) are dropped.
//!   An `action` parameter is not on the stack: it is the state the last
//!   `STORE_STATE` saved.
//! - **Deferred code** (`DelayCommand`, `AssignCommand`, `ActionDoCommand`):
//!   `STORE_STATE g, l` saves the `g` bytes of globals below the base
//!   pointer, the top `l` bytes of locals, and where to resume (16 bytes on,
//!   past the `JMP` over the deferred code). Running it later rebuilds that
//!   stack and runs from there to its `RETN`.
//! - **Faults** end a run: a stack that runs out, a cell of the wrong
//!   type, a jump into no instruction, a division by zero, or more than
//!   `INSTRUCTION_BUDGET` instructions (a script that never ends, stopped
//!   where KOTOR's machine stops it).
//!
//! Deterministic: integer arithmetic wraps, floats are IEEE `f32`, nothing
//! reads a clock or a random source (the host's routines do, from the sim).

use std::fmt;
use std::sync::Arc;

use fmt_odyssey::ncs::{Const, Ncs, Op, Pair, Ty};

use crate::catalog::{Catalog, Param, Routine, Type};

/// The most instructions one run executes before it faults: KOTOR's own
/// limit, which stops the games' endless loops (`docs/SCRIPTS.md`).
pub const INSTRUCTION_BUDGET: usize = 131_071;
/// The most cells the stack holds.
pub const STACK_CELLS: usize = 65_536;
/// Where execution starts: after the header.
const ENTRY: u32 = 13;
/// `STORE_STATE`'s resume point, bytes past it (itself and the `JMP`).
const RESUME_AFTER: u32 = 16;

/// An object as scripts see it. `CONSTO 0` (OBJECT_SELF) is the caller;
/// `CONSTO 1` (OBJECT_INVALID) is `INVALID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u32);

impl ObjectId {
    /// The engine's OBJECT_INVALID.
    pub const INVALID: ObjectId = ObjectId(0x7f00_0000);
}

/// An engine value: the host's handle (0 for none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub u32);

/// A value on the stack or handed to a routine.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    /// Shared: copies on the stack share one string.
    String(Arc<str>),
    Object(ObjectId),
    Effect(Handle),
    Event(Handle),
    Location(Handle),
    Talent(Handle),
    /// Only as an argument or a return value: three float cells.
    Vector([f32; 3]),
    /// Only as an argument: deferred code (`STORE_STATE`'s).
    Action(Box<Deferred>),
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Object(_) => "object",
            Value::Effect(_) => "effect",
            Value::Event(_) => "event",
            Value::Location(_) => "location",
            Value::Talent(_) => "talent",
            Value::Vector(_) => "vector",
            Value::Action(_) => "action",
        }
    }

    /// The empty value of a type (what `RSADD` reserves).
    pub fn empty(ty: Type) -> Value {
        match ty {
            Type::Float => Value::Float(0.0),
            Type::String => Value::String(Arc::from("")),
            Type::Object => Value::Object(ObjectId::INVALID),
            Type::Effect => Value::Effect(Handle(0)),
            Type::Event => Value::Event(Handle(0)),
            Type::Location => Value::Location(Handle(0)),
            Type::Talent => Value::Talent(Handle(0)),
            Type::Vector => Value::Vector([0.0; 3]),
            Type::Void | Type::Int | Type::Action => Value::Int(0),
        }
    }

    fn of_ty(ty: Ty) -> Value {
        Value::empty(match ty {
            Ty::Int => Type::Int,
            Ty::Float => Type::Float,
            Ty::String => Type::String,
            Ty::Object => Type::Object,
            Ty::Effect => Type::Effect,
            Ty::Event => Type::Event,
            Ty::Location => Type::Location,
            Ty::Talent => Type::Talent,
        })
    }
}

/// Code saved to run later: where, and the stack it runs on.
#[derive(Debug, Clone, PartialEq)]
pub struct Deferred {
    pub resume: u32,
    pub globals: Vec<Value>,
    pub locals: Vec<Value>,
}

/// Why a run stopped short.
#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    /// The stack had too few cells.
    Underflow {
        offset: u32,
    },
    /// The stack grew past `STACK_CELLS`.
    Overflow {
        offset: u32,
    },
    /// A cell held another type than the instruction needs.
    Type {
        offset: u32,
        wanted: &'static str,
        found: &'static str,
    },
    /// A routine handed an argument of another type than its prototype's:
    /// the script's own error, which the executables stop the run at too
    /// (their routines' pops refuse a cell of another type, and the machine
    /// ends the run on the refusal). TSL's floating mines, whose compiled
    /// `break` leaves a local on the stack (`k_fm_die_custom`).
    Argument {
        offset: u32,
        routine: String,
        wanted: &'static str,
        found: &'static str,
    },
    /// A jump or resume point that is no instruction's start.
    Jump {
        offset: u32,
        to: u32,
    },
    DivideByZero {
        offset: u32,
    },
    /// More than `INSTRUCTION_BUDGET` instructions.
    /// Where it was when the budget ran out (a loop's offset).
    Budget {
        offset: u32,
    },
    /// A routine number the catalog lacks.
    UnknownRoutine {
        offset: u32,
        routine: u16,
    },
    /// An offset or size not a multiple of a cell.
    Misaligned {
        offset: u32,
    },
    /// The host's routine failed.
    Host {
        offset: u32,
        routine: String,
        error: String,
    },
}

impl Fault {
    /// The fault's kind, for counting.
    pub fn kind(&self) -> &'static str {
        match self {
            Fault::Underflow { .. } => "underflow",
            Fault::Overflow { .. } => "overflow",
            Fault::Type { .. } => "type",
            Fault::Argument { .. } => "argument",
            Fault::Jump { .. } => "jump",
            Fault::DivideByZero { .. } => "divide by zero",
            Fault::Budget { .. } => "budget",
            Fault::UnknownRoutine { .. } => "unknown routine",
            Fault::Misaligned { .. } => "misaligned",
            Fault::Host { .. } => "host",
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Underflow { offset } => write!(f, "stack underflow at {offset:#x}"),
            Fault::Overflow { offset } => write!(f, "stack overflow at {offset:#x}"),
            Fault::Type {
                offset,
                wanted,
                found,
            } => write!(f, "{wanted} wanted, {found} found at {offset:#x}"),
            Fault::Argument {
                offset,
                routine,
                wanted,
                found,
            } => write!(
                f,
                "{routine}: {wanted} wanted, {found} found at {offset:#x}"
            ),
            Fault::Jump { offset, to } => write!(f, "jump from {offset:#x} to {to:#x}"),
            Fault::DivideByZero { offset } => write!(f, "division by zero at {offset:#x}"),
            Fault::Budget { offset } => {
                write!(
                    f,
                    "more than {INSTRUCTION_BUDGET} instructions (at {offset:08x})"
                )
            }
            Fault::UnknownRoutine { offset, routine } => {
                write!(f, "unknown routine {routine} at {offset:#x}")
            }
            Fault::Misaligned { offset } => write!(f, "misaligned offset at {offset:#x}"),
            Fault::Host {
                offset,
                routine,
                error,
            } => write!(f, "{routine} at {offset:#x}: {error}"),
        }
    }
}

/// The engine: what scripts' routines do.
pub trait Host {
    /// Runs `routine` for `caller` with `args` in the prototype's order
    /// (complete: defaults filled in), returning its value (anything for
    /// `void`).
    fn action(
        &mut self,
        routine: &Routine,
        caller: ObjectId,
        args: Vec<Value>,
    ) -> Result<Value, String>;
}

/// A routine's parameters with their defaults resolved (OBJECT_SELF as
/// `ObjectId(0)`, put in for the caller at the call).
struct Prepared {
    params: Vec<(Type, Option<Value>)>,
}

/// The routines, ready to call: the catalog and each routine's defaults.
pub struct Routines {
    catalog: Arc<Catalog>,
    prepared: Vec<Prepared>,
}

impl Routines {
    pub fn new(catalog: Arc<Catalog>) -> Routines {
        let prepared = catalog
            .routines
            .iter()
            .map(|routine| Prepared {
                params: routine
                    .params
                    .iter()
                    .map(|param| (param.ty, default(&catalog, param)))
                    .collect(),
            })
            .collect();
        Routines { catalog, prepared }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}

/// A parameter's default as a value, if it has one we can read.
fn default(catalog: &Catalog, param: &Param) -> Option<Value> {
    let text = param.default.as_deref()?.trim();
    let number = |text: &str| text.trim().trim_end_matches('f').parse::<f32>().ok();
    match param.ty {
        Type::Object => match text {
            "OBJECT_SELF" => Some(Value::Object(ObjectId(0))),
            _ => Some(Value::Object(ObjectId::INVALID)),
        },
        Type::Vector => {
            let inner = text.strip_prefix('[')?.strip_suffix(']')?;
            let parts: Vec<f32> = inner.split(',').filter_map(number).collect();
            let v: [f32; 3] = std::array::from_fn(|i| parts.get(i).copied().unwrap_or(0.0));
            Some(Value::Vector(v))
        }
        Type::String => Some(Value::String(Arc::from(
            text.strip_prefix('"')?.strip_suffix('"')?,
        ))),
        Type::Int | Type::Float => {
            let constant = catalog.constant_named(text).map(|c| c.value.clone());
            let value = match constant {
                Some(crate::catalog::Value::Int(v)) => Value::Int(v),
                Some(crate::catalog::Value::Float(v)) => Value::Float(v),
                Some(crate::catalog::Value::String(_)) => return None,
                None => match param.ty {
                    Type::Int => Value::Int(text.parse().ok()?),
                    _ => Value::Float(number(text)?),
                },
            };
            Some(match (param.ty, value) {
                (Type::Float, Value::Int(v)) => Value::Float(v as f32),
                (_, value) => value,
            })
        }
        _ => Some(Value::empty(param.ty)),
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    /// The int left on top: a conditional script's answer.
    pub returned: Option<i32>,
    pub instructions: usize,
}

/// A program ready to run: its instructions and where each starts.
pub struct Program {
    pub ncs: Ncs,
}

impl Program {
    pub fn new(ncs: Ncs) -> Program {
        Program { ncs }
    }

    fn index(&self, offset: u32, from: u32) -> Result<usize, Fault> {
        self.ncs.at(offset).ok_or(Fault::Jump {
            offset: from,
            to: offset,
        })
    }
}

/// One run's machine.
struct Machine<'a> {
    program: &'a Program,
    routines: &'a Routines,
    caller: ObjectId,
    stack: Vec<Value>,
    bp: usize,
    returns: Vec<usize>,
    stored: Option<Deferred>,
    offset: u32,
}

/// Runs `program` from its start for `caller`.
pub fn run(
    program: &Program,
    routines: &Routines,
    caller: ObjectId,
    host: &mut dyn Host,
) -> Result<Finished, Fault> {
    let start = program.index(ENTRY, ENTRY)?;
    Machine::new(program, routines, caller, Vec::new(), 0).execute(start, host)
}

/// Runs deferred code (a `STORE_STATE`'s) for `caller`.
pub fn resume(
    program: &Program,
    routines: &Routines,
    caller: ObjectId,
    deferred: &Deferred,
    host: &mut dyn Host,
) -> Result<Finished, Fault> {
    let start = program.index(deferred.resume, deferred.resume)?;
    let mut stack = deferred.globals.clone();
    let bp = stack.len();
    stack.extend(deferred.locals.iter().cloned());
    Machine::new(program, routines, caller, stack, bp).execute(start, host)
}

impl<'a> Machine<'a> {
    fn new(
        program: &'a Program,
        routines: &'a Routines,
        caller: ObjectId,
        stack: Vec<Value>,
        bp: usize,
    ) -> Machine<'a> {
        Machine {
            program,
            routines,
            caller,
            stack,
            bp,
            returns: Vec::new(),
            stored: None,
            offset: ENTRY,
        }
    }

    fn underflow(&self) -> Fault {
        Fault::Underflow {
            offset: self.offset,
        }
    }

    fn cells(&self, bytes: i32) -> Result<usize, Fault> {
        if bytes % 4 != 0 {
            return Err(Fault::Misaligned {
                offset: self.offset,
            });
        }
        Ok(bytes.unsigned_abs() as usize / 4)
    }

    /// The index `bytes` (negative) down from the top.
    fn below_top(&self, bytes: i32) -> Result<usize, Fault> {
        let cells = self.cells(bytes)?;
        if bytes > 0 {
            return Err(Fault::Misaligned {
                offset: self.offset,
            });
        }
        self.stack
            .len()
            .checked_sub(cells)
            .ok_or_else(|| self.underflow())
    }

    /// The index `bytes` (negative) down from the base pointer.
    fn below_bp(&self, bytes: i32) -> Result<usize, Fault> {
        let cells = self.cells(bytes)?;
        if bytes > 0 {
            return Err(Fault::Misaligned {
                offset: self.offset,
            });
        }
        self.bp.checked_sub(cells).ok_or_else(|| self.underflow())
    }

    fn push(&mut self, value: Value) -> Result<(), Fault> {
        if self.stack.len() >= STACK_CELLS {
            return Err(Fault::Overflow {
                offset: self.offset,
            });
        }
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, Fault> {
        self.stack.pop().ok_or_else(|| self.underflow())
    }

    fn wrong(&self, wanted: &'static str, found: &Value) -> Fault {
        Fault::Type {
            offset: self.offset,
            wanted,
            found: found.kind(),
        }
    }

    fn int(&mut self) -> Result<i32, Fault> {
        match self.pop()? {
            Value::Int(v) => Ok(v),
            other => Err(self.wrong("int", &other)),
        }
    }

    fn float(&mut self) -> Result<f32, Fault> {
        match self.pop()? {
            Value::Float(v) => Ok(v),
            other => Err(self.wrong("float", &other)),
        }
    }

    fn string(&mut self) -> Result<Arc<str>, Fault> {
        match self.pop()? {
            Value::String(v) => Ok(v),
            other => Err(self.wrong("string", &other)),
        }
    }

    fn vector(&mut self) -> Result<[f32; 3], Fault> {
        let z = self.float()?;
        let y = self.float()?;
        let x = self.float()?;
        Ok([x, y, z])
    }

    fn push_vector(&mut self, v: [f32; 3]) -> Result<(), Fault> {
        for c in v {
            self.push(Value::Float(c))?;
        }
        Ok(())
    }

    /// Pops two operands of `pair` (the second on top) for a numeric op:
    /// both as floats unless both are ints.
    fn numbers(&mut self, pair: Pair) -> Result<Numbers, Fault> {
        Ok(match pair {
            Pair::IntInt => {
                let b = self.int()?;
                Numbers::Ints(self.int()?, b)
            }
            Pair::IntFloat => {
                let b = self.float()?;
                Numbers::Floats(self.int()? as f32, b)
            }
            Pair::FloatInt => {
                let b = self.int()? as f32;
                Numbers::Floats(self.float()?, b)
            }
            Pair::FloatFloat => {
                let b = self.float()?;
                Numbers::Floats(self.float()?, b)
            }
            _ => return Err(self.wrong("numbers", &Value::Int(0))),
        })
    }

    fn compare(
        &mut self,
        pair: Pair,
        ints: fn(i32, i32) -> bool,
        floats: fn(f32, f32) -> bool,
    ) -> Result<(), Fault> {
        let result = match self.numbers(pair)? {
            Numbers::Ints(a, b) => ints(a, b),
            Numbers::Floats(a, b) => floats(a, b),
        };
        self.push(Value::Int(i32::from(result)))
    }

    /// Whether the two operands of `pair` on top are equal (popping them).
    fn equal(&mut self, pair: Pair) -> Result<bool, Fault> {
        Ok(match pair {
            Pair::Struct(bytes) => {
                let n = self.cells(i32::from(bytes))?;
                let at = self
                    .stack
                    .len()
                    .checked_sub(2 * n)
                    .ok_or_else(|| self.underflow())?;
                let same = self.stack[at..at + n] == self.stack[at + n..];
                self.stack.truncate(at);
                same
            }
            // Strings compare without case, as the games' VM does (TSL's
            // exe compares a string on the stack with `CompareNoCase`, its
            // only string compare); reone and xoreos compare with case. K1's
            // own scripts hold under it: tags lowercased as objects load
            // meet the party include's `"t3m4"` (the Sith base's door) and
            // the swoop pits' `"Bastilla03"` alike.
            Pair::StringString => {
                let b = self.string()?;
                let a = self.string()?;
                a.eq_ignore_ascii_case(&b)
            }
            Pair::IntInt | Pair::FloatFloat | Pair::ObjectObject | Pair::Engine(_) => {
                let b = self.pop()?;
                let a = self.pop()?;
                if std::mem::discriminant(&a) != std::mem::discriminant(&b) {
                    return Err(self.wrong(a.kind(), &b));
                }
                a == b
            }
            _ => return Err(self.wrong("comparable", &Value::Int(0))),
        })
    }

    fn arithmetic(&mut self, op: Arith, pair: Pair) -> Result<(), Fault> {
        let offset = self.offset;
        match pair {
            Pair::StringString if op == Arith::Add => {
                let b = self.string()?;
                let a = self.string()?;
                let joined: String = [&*a, &*b].concat();
                self.push(Value::String(Arc::from(joined)))
            }
            Pair::VectorVector if matches!(op, Arith::Add | Arith::Sub) => {
                let b = self.vector()?;
                let a = self.vector()?;
                self.push_vector(std::array::from_fn(|i| match op {
                    Arith::Add => a[i] + b[i],
                    _ => a[i] - b[i],
                }))
            }
            Pair::VectorFloat | Pair::FloatVector if matches!(op, Arith::Mul | Arith::Div) => {
                let (v, s) = if pair == Pair::VectorFloat {
                    let s = self.float()?;
                    (self.vector()?, s)
                } else {
                    let v = self.vector()?;
                    (v, self.float()?)
                };
                if op == Arith::Div && s == 0.0 {
                    return Err(Fault::DivideByZero { offset });
                }
                self.push_vector(v.map(|c| if op == Arith::Mul { c * s } else { c / s }))
            }
            _ => match self.numbers(pair)? {
                Numbers::Ints(a, b) => {
                    let v = match op {
                        Arith::Add => a.wrapping_add(b),
                        Arith::Sub => a.wrapping_sub(b),
                        Arith::Mul => a.wrapping_mul(b),
                        Arith::Div | Arith::Mod if b == 0 => {
                            return Err(Fault::DivideByZero { offset });
                        }
                        Arith::Div => a.wrapping_div(b),
                        Arith::Mod => a.wrapping_rem(b),
                    };
                    self.push(Value::Int(v))
                }
                Numbers::Floats(a, b) => {
                    let v = match op {
                        Arith::Add => a + b,
                        Arith::Sub => a - b,
                        Arith::Mul => a * b,
                        Arith::Div if b == 0.0 => return Err(Fault::DivideByZero { offset }),
                        Arith::Div => a / b,
                        Arith::Mod => return Err(self.wrong("int", &Value::Float(a))),
                    };
                    self.push(Value::Float(v))
                }
            },
        }
    }

    fn ints(&mut self, op: fn(i32, i32) -> i32) -> Result<(), Fault> {
        let b = self.int()?;
        let a = self.int()?;
        self.push(Value::Int(op(a, b)))
    }

    /// Copies `size` bytes from the top down to `at`.
    fn copy_down(&mut self, at: usize, size: u16) -> Result<(), Fault> {
        let n = self.cells(i32::from(size))?;
        let from = self
            .stack
            .len()
            .checked_sub(n)
            .ok_or_else(|| self.underflow())?;
        if at + n > self.stack.len() {
            return Err(self.underflow());
        }
        for i in 0..n {
            self.stack[at + i] = self.stack[from + i].clone();
        }
        Ok(())
    }

    /// Pushes copies of `size` bytes from `at`.
    fn copy_top(&mut self, at: usize, size: u16) -> Result<(), Fault> {
        let n = self.cells(i32::from(size))?;
        if at + n > self.stack.len() {
            return Err(self.underflow());
        }
        for i in 0..n {
            self.push(self.stack[at + i].clone())?;
        }
        Ok(())
    }

    fn bump(&mut self, at: usize, by: i32) -> Result<(), Fault> {
        match self.stack.get_mut(at) {
            Some(Value::Int(v)) => {
                *v = v.wrapping_add(by);
                Ok(())
            }
            Some(other) => {
                let found = other.kind();
                Err(Fault::Type {
                    offset: self.offset,
                    wanted: "int",
                    found,
                })
            }
            None => Err(self.underflow()),
        }
    }

    /// Pops a routine's arguments, first parameter on top.
    fn arguments(&mut self, routine: &Routine, count: u8) -> Result<Vec<Value>, Fault> {
        let prepared = &self.routines.prepared[usize::from(routine.id)];
        let mut args = Vec::with_capacity(prepared.params.len());
        for n in 0..usize::from(count) {
            let value = match prepared.params.get(n).map(|(ty, _)| *ty) {
                Some(Type::Vector) => Value::Vector(self.vector()?),
                Some(Type::Action) => {
                    Value::Action(Box::new(self.stored.clone().ok_or(Fault::Type {
                        offset: self.offset,
                        wanted: "stored state",
                        found: "none",
                    })?))
                }
                Some(ty) => {
                    let value = self.pop()?;
                    if std::mem::discriminant(&value) != std::mem::discriminant(&Value::empty(ty)) {
                        return Err(Fault::Argument {
                            offset: self.offset,
                            routine: routine.name.clone(),
                            wanted: ty.name(),
                            found: value.kind(),
                        });
                    }
                    value
                }
                // An argument the prototype lacks: dropped.
                None => {
                    self.pop()?;
                    continue;
                }
            };
            args.push(value);
        }
        for (ty, default) in prepared.params.iter().skip(args.len()) {
            args.push(match default {
                Some(Value::Object(ObjectId(0))) => Value::Object(self.caller),
                Some(value) => value.clone(),
                None => Value::empty(*ty),
            });
        }
        Ok(args)
    }

    fn action(&mut self, routine: u16, count: u8, host: &mut dyn Host) -> Result<(), Fault> {
        let catalog = &*self.routines.catalog;
        let routine = catalog
            .routine_by_id(routine)
            .ok_or(Fault::UnknownRoutine {
                offset: self.offset,
                routine,
            })?;
        let args = self.arguments(routine, count)?;
        let value = host
            .action(routine, self.caller, args)
            .map_err(|error| Fault::Host {
                offset: self.offset,
                routine: routine.name.clone(),
                error,
            })?;
        match (routine.ret, value) {
            (Type::Void, _) => Ok(()),
            (Type::Vector, Value::Vector(v)) => self.push_vector(v),
            (ty, value)
                if std::mem::discriminant(&value) == std::mem::discriminant(&Value::empty(ty)) =>
            {
                self.push(value)
            }
            (ty, value) => Err(Fault::Host {
                offset: self.offset,
                routine: routine.name.clone(),
                error: format!("returned {} for {}", value.kind(), ty.name()),
            }),
        }
    }

    fn execute(mut self, start: usize, host: &mut dyn Host) -> Result<Finished, Fault> {
        let program = self.program;
        let code = &program.ncs.code;
        let mut at = start;
        let mut executed = 0;
        loop {
            executed += 1;
            if executed > INSTRUCTION_BUDGET {
                return Err(Fault::Budget {
                    offset: self.offset,
                });
            }
            let instruction = code.get(at).ok_or(Fault::Jump {
                offset: self.offset,
                to: self.offset,
            })?;
            self.offset = instruction.offset;
            let mut next = at + 1;
            let here = self.offset;
            let jump = |rel: i32| program.index(here.wrapping_add(rel as u32), here);
            match &instruction.op {
                Op::CpDownSp { offset, size } => {
                    let to = self.below_top(*offset)?;
                    self.copy_down(to, *size)?;
                }
                Op::Rsadd(ty) => self.push(Value::of_ty(*ty))?,
                Op::CpTopSp { offset, size } => {
                    let from = self.below_top(*offset)?;
                    self.copy_top(from, *size)?;
                }
                Op::Const(constant) => self.push(match constant {
                    Const::Int(v) => Value::Int(*v),
                    Const::Float(v) => Value::Float(*v),
                    Const::String(bytes) => {
                        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
                        Value::String(Arc::from(text))
                    }
                    Const::Object(0) => Value::Object(self.caller),
                    Const::Object(1) => Value::Object(ObjectId::INVALID),
                    Const::Object(v) => Value::Object(ObjectId(*v as u32)),
                })?,
                Op::Action { routine, args } => self.action(*routine, *args, host)?,
                Op::LogAnd => self.ints(|a, b| i32::from(a != 0 && b != 0))?,
                Op::LogOr => self.ints(|a, b| i32::from(a != 0 || b != 0))?,
                Op::IncOr => self.ints(|a, b| a | b)?,
                Op::ExcOr => self.ints(|a, b| a ^ b)?,
                Op::BoolAnd => self.ints(|a, b| a & b)?,
                Op::Equal(pair) => {
                    let same = self.equal(*pair)?;
                    self.push(Value::Int(i32::from(same)))?;
                }
                Op::NotEqual(pair) => {
                    let same = self.equal(*pair)?;
                    self.push(Value::Int(i32::from(!same)))?;
                }
                Op::Geq(pair) => self.compare(*pair, |a, b| a >= b, |a, b| a >= b)?,
                Op::Gt(pair) => self.compare(*pair, |a, b| a > b, |a, b| a > b)?,
                Op::Lt(pair) => self.compare(*pair, |a, b| a < b, |a, b| a < b)?,
                Op::Leq(pair) => self.compare(*pair, |a, b| a <= b, |a, b| a <= b)?,
                Op::ShiftLeft => self.ints(|a, b| a.wrapping_shl(b as u32))?,
                Op::ShiftRight => self.ints(|a, b| a.wrapping_shr(b as u32))?,
                Op::UShiftRight => self.ints(|a, b| (a as u32).wrapping_shr(b as u32) as i32)?,
                Op::Add(pair) => self.arithmetic(Arith::Add, *pair)?,
                Op::Sub(pair) => self.arithmetic(Arith::Sub, *pair)?,
                Op::Mul(pair) => self.arithmetic(Arith::Mul, *pair)?,
                Op::Div(pair) => self.arithmetic(Arith::Div, *pair)?,
                Op::Mod => self.arithmetic(Arith::Mod, Pair::IntInt)?,
                Op::Neg(Ty::Float) => {
                    let v = self.float()?;
                    self.push(Value::Float(-v))?;
                }
                Op::Neg(_) => {
                    let v = self.int()?;
                    self.push(Value::Int(v.wrapping_neg()))?;
                }
                Op::Comp => {
                    let v = self.int()?;
                    self.push(Value::Int(!v))?;
                }
                Op::Not => {
                    let v = self.int()?;
                    self.push(Value::Int(i32::from(v == 0)))?;
                }
                Op::MovSp(offset) => {
                    let keep = self.below_top(*offset)?;
                    self.stack.truncate(keep);
                }
                Op::Jmp(rel) => next = jump(*rel)?,
                Op::Jsr(rel) => {
                    self.returns.push(next);
                    next = jump(*rel)?;
                }
                Op::Jz(rel) => {
                    if self.int()? == 0 {
                        next = jump(*rel)?;
                    }
                }
                Op::Jnz(rel) => {
                    if self.int()? != 0 {
                        next = jump(*rel)?;
                    }
                }
                Op::Retn => match self.returns.pop() {
                    Some(back) => next = back,
                    None => {
                        let returned = match self.stack.last() {
                            Some(Value::Int(v)) => Some(*v),
                            _ => None,
                        };
                        return Ok(Finished {
                            returned,
                            instructions: executed,
                        });
                    }
                },
                Op::Destruct { size, offset, keep } => {
                    let n = self.cells(i32::from(*size))?;
                    let from = self
                        .stack
                        .len()
                        .checked_sub(n)
                        .ok_or_else(|| self.underflow())?;
                    let start = from + self.cells(i32::from(*offset))?;
                    let kept_n = self.cells(i32::from(*keep))?;
                    if start + kept_n > self.stack.len() {
                        return Err(self.underflow());
                    }
                    let kept: Vec<Value> = self.stack[start..start + kept_n].to_vec();
                    self.stack.truncate(from);
                    self.stack.extend(kept);
                }
                Op::DecSp(offset) => {
                    let at = self.below_top(*offset)?;
                    self.bump(at, -1)?;
                }
                Op::IncSp(offset) => {
                    let at = self.below_top(*offset)?;
                    self.bump(at, 1)?;
                }
                Op::CpDownBp { offset, size } => {
                    let to = self.below_bp(*offset)?;
                    self.copy_down(to, *size)?;
                }
                Op::CpTopBp { offset, size } => {
                    let from = self.below_bp(*offset)?;
                    self.copy_top(from, *size)?;
                }
                Op::DecBp(offset) => {
                    let at = self.below_bp(*offset)?;
                    self.bump(at, -1)?;
                }
                Op::IncBp(offset) => {
                    let at = self.below_bp(*offset)?;
                    self.bump(at, 1)?;
                }
                Op::SaveBp => {
                    let old = self.bp;
                    self.bp = self.stack.len();
                    self.push(Value::Int(old as i32))?;
                }
                Op::RestoreBp => {
                    let old = self.int()?;
                    self.bp = usize::try_from(old).map_err(|_| self.underflow())?;
                }
                Op::StoreState { bp, sp } => {
                    let globals = self.cells(*bp as i32)?;
                    let locals = self.cells(*sp as i32)?;
                    let from = self
                        .bp
                        .checked_sub(globals)
                        .ok_or_else(|| self.underflow())?;
                    let top = self
                        .stack
                        .len()
                        .checked_sub(locals)
                        .ok_or_else(|| self.underflow())?;
                    let resume = self.offset + RESUME_AFTER;
                    self.program.index(resume, self.offset)?;
                    self.stored = Some(Deferred {
                        resume,
                        globals: self.stack[from..self.bp].to_vec(),
                        locals: self.stack[top..].to_vec(),
                    });
                }
                Op::Nop => {}
            }
            at = next;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

enum Numbers {
    Ints(i32, i32),
    Floats(f32, f32),
}
