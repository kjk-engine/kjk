//! Programs built from instructions here rather than compiled from a
//! script: the VM's probes, the routine calls kjk itself runs
//! (`sim::script::calls`), and the setup scripts `kjk original` puts in
//! an original game's `override` (`Expr`, `script`, `conditional`).
//!
//! A call is written as the games' compiler writes it, read off their own
//! scripts: the arguments pushed last first, every default the prototype
//! gives written out (the original games may not fill them in as kjk's VM
//! does), and the return value popped when nothing takes it. An `action`
//! argument (`AssignCommand`'s, `DelayCommand`'s) is `STORESTATE 0, 0`, a
//! `JMP` over its code, the code and a `RETN` (`k_act_cmusic_off` writes
//! `STORESTATE 0, 4` for its one local; these scripts keep none).

use std::collections::BTreeMap;

use fmt_odyssey::ncs::{Const, Instruction, Ncs, Op, Ty};

use crate::catalog::{Catalog, Param, Type};

/// Builds a program from instructions and labelled jumps.
#[derive(Default)]
pub struct Asm {
    pub(crate) code: Vec<(Op, Option<String>)>,
    pub(crate) labels: BTreeMap<String, usize>,
    /// Deferred blocks written so far, to name their labels.
    blocks: usize,
}

impl Asm {
    pub fn op(&mut self, op: Op) -> &mut Asm {
        self.code.push((op, None));
        self
    }

    /// A jump (`Op::Jmp(0)` and the like) to `label`.
    pub fn to(&mut self, jump: fn(i32) -> Op, label: &str) -> &mut Asm {
        self.code.push((jump(0), Some(label.to_string())));
        self
    }

    pub fn label(&mut self, name: &str) -> &mut Asm {
        self.labels.insert(name.to_string(), self.code.len());
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

    /// `expr` as a statement: its value, if any, popped. An `If` is its
    /// condition, a `JZ` past its body, and the body.
    pub fn statement(&mut self, catalog: &Catalog, expr: &Expr) -> Result<&mut Asm, String> {
        if let Expr::If(condition, body) = expr {
            if self.expr(catalog, condition)? != Type::Int {
                return Err("an if's condition is an int".into());
            }
            self.blocks += 1;
            let after = format!("after if {}", self.blocks);
            self.to(Op::Jz, &after);
            for statement in body {
                self.statement(catalog, statement)?;
            }
            self.label(&after);
            return Ok(self);
        }
        let ty = self.expr(catalog, expr)?;
        if ty.cells() > 0 {
            self.op(Op::MovSp(-4 * ty.cells() as i32));
        }
        Ok(self)
    }

    /// Pushes `expr`'s value; its type.
    fn expr(&mut self, catalog: &Catalog, expr: &Expr) -> Result<Type, String> {
        let constant = |asm: &mut Asm, c: Const| {
            asm.op(Op::Const(c));
        };
        Ok(match expr {
            Expr::Int(v) => {
                constant(self, Const::Int(*v));
                Type::Int
            }
            Expr::Float(v) => {
                constant(self, Const::Float(*v));
                Type::Float
            }
            Expr::String(v) => {
                constant(self, Const::String(v.as_bytes().to_vec()));
                Type::String
            }
            Expr::Me => {
                constant(self, Const::Object(0));
                Type::Object
            }
            Expr::Invalid => {
                constant(self, Const::Object(1));
                Type::Object
            }
            Expr::Constant(name) => {
                let found = catalog
                    .constant_named(name)
                    .ok_or(format!("{name}: no such constant in nwscript.nss"))?;
                match &found.value {
                    crate::catalog::Value::Int(v) => self.expr(catalog, &Expr::Int(*v))?,
                    crate::catalog::Value::Float(v) => self.expr(catalog, &Expr::Float(*v))?,
                    crate::catalog::Value::String(v) => {
                        self.expr(catalog, &Expr::String(v.clone()))?
                    }
                }
            }
            Expr::Vector(v) => {
                for c in v {
                    constant(self, Const::Float(*c));
                }
                Type::Vector
            }
            Expr::Call(name, args) => self.call(catalog, name, args)?,
            Expr::Not(condition) => {
                if self.expr(catalog, condition)? != Type::Int {
                    return Err("a not is of an int".into());
                }
                self.op(Op::Not);
                Type::Int
            }
            Expr::Action(_) => return Err("an action is only a routine's argument".into()),
            Expr::If(..) => return Err("an if is only a statement".into()),
        })
    }

    /// A routine called with `args`, the rest its defaults: the last
    /// argument pushed first, so the first is on top.
    fn call(&mut self, catalog: &Catalog, name: &str, args: &[Expr]) -> Result<Type, String> {
        let routine = catalog
            .routine_named(name)
            .ok_or(format!("{name}: no such routine in nwscript.nss"))?;
        if args.len() > routine.params.len() {
            return Err(format!(
                "{name}: {} arguments given, it takes {}",
                args.len(),
                routine.params.len()
            ));
        }
        let mut full = Vec::with_capacity(routine.params.len());
        for (n, param) in routine.params.iter().enumerate() {
            full.push(match args.get(n) {
                Some(arg) => arg.clone(),
                None => default(catalog, param).ok_or(format!(
                    "{name}: argument {} ({}) has no default to write",
                    n + 1,
                    param.name
                ))?,
            });
        }
        for (n, (param, arg)) in routine.params.iter().zip(&full).enumerate().rev() {
            let wrong = |found: &str| {
                format!(
                    "{name}: argument {} ({}) is {}, given {found}",
                    n + 1,
                    param.name,
                    param.ty.name()
                )
            };
            match (param.ty, arg) {
                (Type::Action, Expr::Action(body)) => {
                    // Only the last argument: nothing else is pushed yet,
                    // so the saved stack is empty.
                    if n + 1 != routine.params.len() {
                        return Err(format!("{name}: an action not last is not written"));
                    }
                    self.deferred(catalog, body)?;
                }
                (Type::Action, _) => return Err(wrong("a value")),
                (_, Expr::Action(_)) => return Err(wrong("an action")),
                (want, arg) => {
                    let found = self.expr(catalog, arg)?;
                    if found != want {
                        return Err(wrong(found.name()));
                    }
                }
            }
        }
        let args = u8::try_from(routine.params.len()).map_err(|_| format!("{name}: too many"))?;
        self.op(Op::Action {
            routine: routine.id,
            args,
        });
        Ok(routine.ret)
    }

    /// Code run later: `STORESTATE 0, 0`, a jump over it, its statements
    /// and a `RETN`.
    fn deferred(&mut self, catalog: &Catalog, body: &[Expr]) -> Result<(), String> {
        self.blocks += 1;
        let after = format!("after deferred {}", self.blocks);
        self.op(Op::StoreState { bp: 0, sp: 0 }).to(Op::Jmp, &after);
        for statement in body {
            self.statement(catalog, statement)?;
        }
        self.op(Op::Retn).label(&after);
        Ok(())
    }
}

/// A value as a script writes it: what a setup script is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i32),
    Float(f32),
    String(String),
    Vector([f32; 3]),
    /// `OBJECT_SELF`.
    Me,
    /// `OBJECT_INVALID`.
    Invalid,
    /// A constant `nwscript.nss` names (`TRUE`, a companion's number).
    Constant(String),
    /// A routine by name and its first arguments; the rest take the
    /// prototype's defaults.
    Call(String, Vec<Expr>),
    /// Code run later (`AssignCommand`'s, `DelayCommand`'s): statements.
    Action(Vec<Expr>),
    /// A statement only: its statements, run when the condition (an int)
    /// is not 0.
    If(Box<Expr>, Vec<Expr>),
    /// An int's logical not (`!`): 1 for 0, else 0.
    Not(Box<Expr>),
}

/// A call of routine `name` with `args`.
pub fn call(name: &str, args: impl IntoIterator<Item = Expr>) -> Expr {
    Expr::Call(name.to_string(), args.into_iter().collect())
}

/// Code run later, of `statements`.
pub fn action(statements: impl IntoIterator<Item = Expr>) -> Expr {
    Expr::Action(statements.into_iter().collect())
}

pub fn text(v: &str) -> Expr {
    Expr::String(v.to_string())
}

/// `statements`, run when `condition` holds.
pub fn when(condition: Expr, statements: impl IntoIterator<Item = Expr>) -> Expr {
    Expr::If(Box::new(condition), statements.into_iter().collect())
}

/// `!condition`.
pub fn not(condition: Expr) -> Expr {
    Expr::Not(Box::new(condition))
}

pub fn constant(name: &str) -> Expr {
    Expr::Constant(name.to_string())
}

/// A script that runs `statements` (`void main()`): `JSR main; RETN`,
/// then the statements and a `RETN`.
pub fn script(catalog: &Catalog, statements: &[Expr]) -> Result<Ncs, String> {
    let mut asm = Asm::default();
    asm.to(Op::Jsr, "main").op(Op::Retn).label("main");
    for statement in statements {
        asm.statement(catalog, statement)?;
    }
    asm.op(Op::Retn);
    Ok(asm.build())
}

/// A conditional script (`int StartingConditional()`) that answers
/// `answer`: `RSADDI; JSR main; RETN`, then the answer copied into the
/// reserved cell.
pub fn conditional(catalog: &Catalog, answer: &Expr) -> Result<Ncs, String> {
    let mut asm = Asm::default();
    asm.op(Op::Rsadd(Ty::Int))
        .to(Op::Jsr, "main")
        .op(Op::Retn)
        .label("main");
    if asm.expr(catalog, answer)? != Type::Int {
        return Err("a conditional script answers an int".into());
    }
    asm.op(Op::CpDownSp {
        offset: -8,
        size: 4,
    })
    .op(Op::MovSp(-4))
    .op(Op::Retn);
    Ok(asm.build())
}

/// A parameter's default as the compiler writes it, if it has one.
fn default(catalog: &Catalog, param: &Param) -> Option<Expr> {
    let text = param.default.as_deref()?.trim();
    let number = |text: &str| text.trim().trim_end_matches('f').parse::<f32>().ok();
    Some(match param.ty {
        Type::Object => match text {
            "OBJECT_SELF" => Expr::Me,
            "OBJECT_INVALID" => Expr::Invalid,
            _ => return None,
        },
        Type::Vector => {
            let inner = text.strip_prefix('[')?.strip_suffix(']')?;
            let parts: Vec<f32> = inner.split(',').map(number).collect::<Option<_>>()?;
            Expr::Vector(parts.try_into().ok()?)
        }
        Type::String => Expr::String(text.strip_prefix('"')?.strip_suffix('"')?.to_string()),
        Type::Int => match catalog.constant_named(text) {
            Some(c) => match c.value {
                crate::catalog::Value::Int(v) => Expr::Int(v),
                _ => return None,
            },
            None => Expr::Int(text.parse().ok()?),
        },
        Type::Float => match catalog.constant_named(text) {
            Some(c) => match c.value {
                crate::catalog::Value::Float(v) => Expr::Float(v),
                crate::catalog::Value::Int(v) => Expr::Float(v as f32),
                _ => return None,
            },
            None => Expr::Float(number(text)?),
        },
        _ => return None,
    })
}
