//! The program a script's items make (`compile`'s layout, described there).

use std::collections::{BTreeMap, BTreeSet};

use fmt_odyssey::ncs::{Const, Ncs, Op, Pair, Ty as Cell};

use super::parse::{Expr, Function, Item, Located, Param, Stmt, SwitchPart, Ty};
use super::{Error, Leaving};
use crate::asm::Asm;
use crate::catalog::{self, Catalog};

/// A value's size on the stack, bytes.
fn size(ty: &Ty, structs: &BTreeMap<String, Vec<(Ty, String)>>) -> i32 {
    match ty {
        Ty::Void | Ty::Action => 0,
        Ty::Vector => 12,
        Ty::Struct(name) => structs.get(name).map_or(0, |fields| {
            fields.iter().map(|(ty, _)| size(ty, structs)).sum()
        }),
        _ => 4,
    }
}

/// What is wrong with `ty` when it is a struct that is not declared. Such
/// a struct used to count as nothing at all, and a variable of it took no
/// place on the stack.
fn undeclared(ty: &Ty, structs: &BTreeMap<String, Vec<(Ty, String)>>) -> Option<String> {
    match ty {
        Ty::Struct(name) if !structs.contains_key(name) => {
            Some(format!("the struct {name} is not declared"))
        }
        _ => None,
    }
}

/// The cell type of a value one cell long.
fn cell(ty: &Ty) -> Option<Cell> {
    Some(match ty {
        Ty::Int => Cell::Int,
        Ty::Float => Cell::Float,
        Ty::String => Cell::String,
        Ty::Object => Cell::Object,
        Ty::Effect => Cell::Effect,
        Ty::Event => Cell::Event,
        Ty::Location => Cell::Location,
        Ty::Talent => Cell::Talent,
        _ => return None,
    })
}

fn of_catalog(ty: catalog::Type) -> Ty {
    match ty {
        catalog::Type::Void => Ty::Void,
        catalog::Type::Int => Ty::Int,
        catalog::Type::Float => Ty::Float,
        catalog::Type::String => Ty::String,
        catalog::Type::Object => Ty::Object,
        catalog::Type::Vector => Ty::Vector,
        catalog::Type::Effect => Ty::Effect,
        catalog::Type::Event => Ty::Event,
        catalog::Type::Location => Ty::Location,
        catalog::Type::Talent => Ty::Talent,
        catalog::Type::Action => Ty::Action,
    }
}

/// Where a variable lives: a local's start on the stack (bytes from the
/// frame's base; parameters below 0), or a global's in the globals.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Home {
    Local(i32),
    Global(i32),
}

#[derive(Debug, Clone)]
struct Var {
    ty: Ty,
    home: Home,
}

/// A loop's or a switch's labels and the stack top at its start, for
/// `break` and `continue`.
struct Exit {
    top: i32,
    broken: String,
    /// `None` for a switch, which has no `continue`.
    continued: Option<String>,
}

/// Everything known of the script's file scope.
struct Scope<'a> {
    file: String,
    catalog: &'a Catalog,
    structs: BTreeMap<String, Vec<(Ty, String)>>,
    /// The user's constants (`const` globals with a literal value, and
    /// `#define`s), by name.
    constants: BTreeMap<String, Expr>,
    globals: BTreeMap<String, Var>,
    globals_size: i32,
    functions: BTreeMap<String, Function>,
    /// How a `continue` and a `break` leave their blocks.
    leaving: Leaving,
    /// The `#define`s whose value is not one number or one string.
    unread_defines: BTreeSet<String>,
}

/// One function being written.
struct Writer<'s, 'a> {
    scope: &'s Scope<'a>,
    asm: &'s mut Asm,
    /// The stack's top, bytes from the frame's base.
    top: i32,
    locals: Vec<Vec<(String, Var)>>,
    exits: Vec<Exit>,
    ret: Ty,
    /// Where the function's result is reserved (below its parameters).
    ret_at: i32,
    end: String,
    line: u32,
    labels: usize,
    /// The user's functions this one calls, in the order first called.
    calls: Vec<String>,
    /// The file and the function being written, and for each open scope
    /// the debug notes of its locals (`compile::Debug`), ended as it closes.
    file: String,
    function: String,
    noted: Vec<Vec<usize>>,
    /// How many expressions are being written inside each other (`expr`).
    expressions: usize,
    /// Whether this is the code that sets the globals. There a global is
    /// read only once it is set (`program`).
    setting_globals: bool,
}

/// A type's name in a debug note: one word.
fn ty_name(ty: &Ty) -> String {
    match ty {
        Ty::Struct(name) => format!("struct:{name}"),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

type Done<T> = Result<T, Error>;

impl Writer<'_, '_> {
    fn error<T>(&self, what: impl Into<String>) -> Done<T> {
        Err(Error {
            file: self.scope.file.clone(),
            line: self.line,
            what: what.into(),
        })
    }

    fn size(&self, ty: &Ty) -> i32 {
        size(ty, &self.scope.structs)
    }

    fn op(&mut self, op: Op) {
        self.top += match &op {
            Op::Rsadd(_) | Op::Const(_) => 4,
            Op::CpTopSp { size, .. } | Op::CpTopBp { size, .. } => i32::from(*size),
            Op::MovSp(by) => *by,
            _ => 0,
        };
        self.asm.op(op);
    }

    fn label(&mut self, stem: &str) -> String {
        self.labels += 1;
        format!("{stem} {}", self.labels)
    }

    fn jump(&mut self, jump: fn(i32) -> Op, label: &str) {
        // A conditional jump pops the int it tests.
        if !matches!(jump(0), Op::Jmp(_)) {
            self.top -= 4;
        }
        self.asm.to(jump, label);
    }

    fn mark(&mut self, label: &str) {
        self.asm.label(label);
    }

    fn movsp(&mut self, by: i32) {
        if by != 0 {
            self.op(Op::MovSp(by));
        }
    }

    /// Reserves a value of `ty`: a cell for each of its parts.
    fn reserve(&mut self, ty: &Ty) -> Done<()> {
        match ty {
            Ty::Vector => {
                for _ in 0..3 {
                    self.op(Op::Rsadd(Cell::Float));
                }
            }
            Ty::Struct(name) => {
                let Some(fields) = self.scope.structs.get(name).cloned() else {
                    return self.error(format!("the struct {name} is not declared"));
                };
                for (ty, _) in fields {
                    self.reserve(&ty)?;
                }
            }
            Ty::Void | Ty::Action => {}
            other => {
                let Some(cell) = cell(other) else {
                    return self.error(format!("a {other:?} cannot be reserved"));
                };
                self.op(Op::Rsadd(cell));
            }
        }
        Ok(())
    }

    fn find(&self, name: &str) -> Option<Var> {
        for scope in self.locals.iter().rev() {
            if let Some((_, var)) = scope.iter().rev().find(|(own, _)| own == name) {
                return Some(var.clone());
            }
        }
        self.scope.globals.get(name).cloned()
    }

    /// Copies `size` bytes from `at` within `home` to the top.
    fn copy_up(&mut self, home: Home, at: i32, size: i32) {
        let size = size as u16;
        match home {
            Home::Local(start) => {
                let offset = start + at - self.top;
                self.op(Op::CpTopSp { offset, size });
            }
            Home::Global(start) => {
                let offset = start + at - self.scope.globals_size;
                self.op(Op::CpTopBp { offset, size });
            }
        }
    }

    /// Copies the top `size` bytes down to `at` within `home`.
    fn copy_down(&mut self, home: Home, at: i32, size: i32) {
        let size = size as u16;
        match home {
            Home::Local(start) => {
                let offset = start + at - self.top;
                self.op(Op::CpDownSp { offset, size });
            }
            Home::Global(start) => {
                let offset = start + at - self.scope.globals_size;
                self.op(Op::CpDownBp { offset, size });
            }
        }
    }

    /// A place a value can be put: a variable, or a field of one. Its home,
    /// the field's place within it and the field's type.
    fn place(&self, expr: &Expr) -> Done<(Home, i32, Ty)> {
        match expr {
            Expr::Name(name) => match self.find(name) {
                Some(var) => Ok((var.home, 0, var.ty)),
                None => self.error(format!("{name} is not a variable")),
            },
            Expr::Field(base, field) => {
                let (home, at, ty) = self.place(base)?;
                let (offset, field_ty) = self.field(&ty, field)?;
                Ok((home, at + offset, field_ty))
            }
            _ => self.error("only a variable or its field takes a value"),
        }
    }

    /// A field's place within a value of `ty`, and its type.
    fn field(&self, ty: &Ty, field: &str) -> Done<(i32, Ty)> {
        match ty {
            Ty::Vector => match field {
                "x" => Ok((0, Ty::Float)),
                "y" => Ok((4, Ty::Float)),
                "z" => Ok((8, Ty::Float)),
                _ => self.error(format!("a vector has no {field}")),
            },
            Ty::Struct(name) => {
                let fields = self
                    .scope
                    .structs
                    .get(name)
                    .cloned()
                    .unwrap_or_else(Vec::new);
                let mut at = 0;
                for (ty, own) in fields {
                    if own == field {
                        return Ok((at, ty));
                    }
                    at += self.size(&ty);
                }
                self.error(format!("struct {name} has no {field}"))
            }
            other => self.error(format!("a {other:?} has no fields")),
        }
    }

    fn constant(&mut self, constant: Const, ty: Ty) -> Ty {
        self.op(Op::Const(constant));
        ty
    }

    /// Pushes `expr`'s value and gives its type.
    ///
    /// An expression of operators in a row is written as deep as it is
    /// long. The parser lets each row be `MOST_OPERATORS` long, and rows of
    /// different operators lie in each other, so the depth is limited here
    /// as well.
    fn expr(&mut self, expr: &Expr) -> Done<Ty> {
        if self.expressions >= 2 * super::parse::MOST_OPERATORS {
            return self.error("the expression is too long to write: split it into two");
        }
        self.expressions += 1;
        let ty = self.expr_of(expr);
        self.expressions -= 1;
        ty
    }

    fn expr_of(&mut self, expr: &Expr) -> Done<Ty> {
        Ok(match expr {
            Expr::Int(v) => self.constant(Const::Int(*v), Ty::Int),
            Expr::Float(v) => self.constant(Const::Float(*v), Ty::Float),
            Expr::Str(v) => self.constant(Const::String(v.clone()), Ty::String),
            Expr::Vector(v) => {
                for value in v {
                    self.op(Op::Const(Const::Float(*value)));
                }
                Ty::Vector
            }
            Expr::Name(name) => {
                if let Some(var) = self.find(name) {
                    if self.setting_globals && matches!(var.home, Home::Global(_)) {
                        return self.error(format!(
                            "{name} is set further down, so it has no value to read here"
                        ));
                    }
                    let size = self.size(&var.ty);
                    self.copy_up(var.home, 0, size);
                    return Ok(var.ty);
                }
                if let Some(value) = self.scope.constants.get(name).cloned() {
                    return self.expr(&value);
                }
                match name.as_str() {
                    "OBJECT_SELF" => return Ok(self.constant(Const::Object(0), Ty::Object)),
                    "OBJECT_INVALID" => return Ok(self.constant(Const::Object(1), Ty::Object)),
                    _ => {}
                }
                let Some(constant) = self.scope.catalog.constant_named(name) else {
                    if self.scope.unread_defines.contains(name) {
                        return self.error(format!(
                            "{name} is a #define that is not one number or one string"
                        ));
                    }
                    return self.error(format!("{name} is not known"));
                };
                match &constant.value {
                    catalog::Value::Int(v) => self.constant(Const::Int(*v), Ty::Int),
                    catalog::Value::Float(v) => self.constant(Const::Float(*v), Ty::Float),
                    catalog::Value::String(v) => {
                        self.constant(Const::String(v.as_bytes().to_vec()), Ty::String)
                    }
                }
            }
            Expr::Call(name, args) => self.call(name, args)?,
            Expr::Field(base, field) => {
                // The whole value is pushed, and all but the field popped.
                let whole = self.expr(base)?;
                let (offset, ty) = self.field(&whole, field)?;
                let (whole_size, keep) = (self.size(&whole), self.size(&ty));
                self.asm.op(Op::Destruct {
                    size: whole_size as u16,
                    offset: offset as i16,
                    keep: keep as u16,
                });
                self.top -= whole_size - keep;
                ty
            }
            Expr::Unary(mark, operand) => {
                let ty = self.expr(operand)?;
                match (*mark, &ty) {
                    ("-", Ty::Int) => self.asm.op(Op::Neg(Cell::Int)),
                    ("-", Ty::Float) => self.asm.op(Op::Neg(Cell::Float)),
                    ("!", Ty::Int) => self.asm.op(Op::Not),
                    ("~", Ty::Int) => self.asm.op(Op::Comp),
                    _ => return self.error(format!("{mark} does not take a {ty:?}")),
                };
                ty
            }
            Expr::Binary(mark, left, right) => self.binary(mark, left, right)?,
            Expr::Assign(mark, target, value) => {
                let (home, at, ty) = self.place(target)?;
                let size = self.size(&ty);
                if *mark == "=" {
                    let given = self.expr(value)?;
                    if given != ty {
                        return self.error(format!("a {given:?} cannot be put in a {ty:?}"));
                    }
                } else {
                    let op = &mark[..mark.len() - 1];
                    let done = self.binary(op, target, value)?;
                    if done != ty {
                        return self.error(format!("{mark} makes a {done:?} of a {ty:?}"));
                    }
                }
                self.copy_down(home, at, size);
                ty
            }
            Expr::Step(mark, before, target) => {
                let (home, at, ty) = self.place(target)?;
                if ty != Ty::Int {
                    return self.error(format!("{mark} takes an int, not a {ty:?}"));
                }
                let step = |writer: &mut Self| {
                    let op = match (home, *mark) {
                        (Home::Local(start), "++") => Op::IncSp(start + at - writer.top),
                        (Home::Local(start), _) => Op::DecSp(start + at - writer.top),
                        (Home::Global(start), "++") => {
                            Op::IncBp(start + at - writer.scope.globals_size)
                        }
                        (Home::Global(start), _) => {
                            Op::DecBp(start + at - writer.scope.globals_size)
                        }
                    };
                    writer.op(op);
                };
                if *before {
                    step(self);
                    self.copy_up(home, at, 4);
                } else {
                    self.copy_up(home, at, 4);
                    step(self);
                }
                Ty::Int
            }
            Expr::Choose(condition, then, otherwise) => {
                if self.expr(condition)? != Ty::Int {
                    return self.error("a choice's condition is an int");
                }
                let (other, end) = (self.label("else"), self.label("end of choice"));
                self.jump(Op::Jz, &other);
                let ty = self.expr(then)?;
                self.jump(Op::Jmp, &end);
                self.top -= self.size(&ty);
                self.mark(&other);
                let second = self.expr(otherwise)?;
                if second != ty {
                    return self.error("a choice's two values differ in type");
                }
                self.mark(&end);
                ty
            }
        })
    }

    fn binary(&mut self, mark: &str, left: &Expr, right: &Expr) -> Done<Ty> {
        if mark == "&&" || mark == "||" {
            if self.expr(left)? != Ty::Int {
                return self.error(format!("{mark} takes ints"));
            }
            let end = self.label("end of logic");
            self.op(Op::CpTopSp {
                offset: -4,
                size: 4,
            });
            if mark == "&&" {
                self.jump(Op::Jz, &end);
            } else {
                // BioWare's `||`: a false left side goes to the right side,
                // and a true one is copied and tested again with a `JZ`,
                // which it never takes, so the right side is always worked
                // out. The `JZ` would land on the `LOGORII`.
                let right_side = self.label("right of or");
                self.jump(Op::Jz, &right_side);
                self.op(Op::CpTopSp {
                    offset: -4,
                    size: 4,
                });
                self.jump(Op::Jz, &end);
                self.mark(&right_side);
            }
            if self.expr(right)? != Ty::Int {
                return self.error(format!("{mark} takes ints"));
            }
            if mark == "&&" {
                self.asm.op(Op::LogAnd);
                self.mark(&end);
            } else {
                self.mark(&end);
                self.asm.op(Op::LogOr);
            }
            self.top -= 4;
            return Ok(Ty::Int);
        }
        let lt = self.expr(left)?;
        let rt = self.expr(right)?;
        let (ls, rs) = (self.size(&lt), self.size(&rt));
        use Ty::{Float as F, Int as I, String as S, Vector as V};
        let pair = match (&lt, &rt) {
            (I, I) => Pair::IntInt,
            (F, F) => Pair::FloatFloat,
            (I, F) => Pair::IntFloat,
            (F, I) => Pair::FloatInt,
            (S, S) => Pair::StringString,
            (Ty::Object, Ty::Object) => Pair::ObjectObject,
            (V, V) if matches!(mark, "==" | "!=") => Pair::Struct(12),
            (V, V) => Pair::VectorVector,
            (V, F) => Pair::VectorFloat,
            (F, V) => Pair::FloatVector,
            (Ty::Struct(a), Ty::Struct(b)) if a == b => Pair::Struct(ls as u16),
            (a, b) if a == b => match cell(a) {
                Some(cell) => Pair::Engine(cell),
                None => return self.error(format!("{mark} does not take {lt:?} and {rt:?}")),
            },
            _ => return self.error(format!("{mark} does not take {lt:?} and {rt:?}")),
        };
        // Each operator takes only the pairs that the games' compiled
        // scripts hold and the VM runs (`vm::Vm::arithmetic`, `compare`,
        // `equal`). Any other pair is refused here. It used to compile and
        // then fault when it ran, and some pairs were given the wrong
        // result, which put every later place on the stack off.
        //
        // An int and a float mix in arithmetic and give a float. They do
        // not mix in a comparison: none of the 380,000 comparisons in the
        // two games' scripts compares an int with a float.
        let ints = matches!(pair, Pair::IntInt);
        let numbers = matches!(
            pair,
            Pair::IntInt | Pair::FloatFloat | Pair::IntFloat | Pair::FloatInt
        );
        let number = if ints { I } else { F };
        let (op, result) = match (mark, pair) {
            (
                "==" | "!=",
                Pair::IntInt
                | Pair::FloatFloat
                | Pair::StringString
                | Pair::ObjectObject
                | Pair::Struct(_)
                | Pair::Engine(_),
            ) => {
                let op = if mark == "==" {
                    Op::Equal(pair)
                } else {
                    Op::NotEqual(pair)
                };
                (op, I)
            }
            ("<", Pair::IntInt | Pair::FloatFloat) => (Op::Lt(pair), I),
            (">", Pair::IntInt | Pair::FloatFloat) => (Op::Gt(pair), I),
            ("<=", Pair::IntInt | Pair::FloatFloat) => (Op::Leq(pair), I),
            (">=", Pair::IntInt | Pair::FloatFloat) => (Op::Geq(pair), I),
            ("+", _) if numbers => (Op::Add(pair), number),
            ("+", Pair::StringString) => (Op::Add(pair), S),
            ("+", Pair::VectorVector) => (Op::Add(pair), V),
            ("-", _) if numbers => (Op::Sub(pair), number),
            ("-", Pair::VectorVector) => (Op::Sub(pair), V),
            ("*", _) if numbers => (Op::Mul(pair), number),
            ("*", Pair::VectorFloat | Pair::FloatVector) => (Op::Mul(pair), V),
            ("/", _) if numbers => (Op::Div(pair), number),
            ("/", Pair::VectorFloat) => (Op::Div(pair), V),
            ("%", Pair::IntInt) => (Op::Mod, I),
            ("&", Pair::IntInt) => (Op::BoolAnd, I),
            ("|", Pair::IntInt) => (Op::IncOr, I),
            ("^", Pair::IntInt) => (Op::ExcOr, I),
            ("<<", Pair::IntInt) => (Op::ShiftLeft, I),
            (">>", Pair::IntInt) => (Op::ShiftRight, I),
            (">>>", Pair::IntInt) => (Op::UShiftRight, I),
            _ => return self.error(format!("{mark} does not take {lt:?} and {rt:?}")),
        };
        self.asm.op(op);
        self.top -= ls + rs;
        self.top += self.size(&result);
        Ok(result)
    }

    /// An action argument: the stack saved, and a jump over the code the
    /// action runs.
    fn action(&mut self, expr: &Expr) -> Done<()> {
        let over = self.label("after action");
        self.asm.op(Op::StoreState {
            bp: self.scope.globals_size as u32,
            // The function's result and parameters are saved with its
            // locals.
            sp: (self.top - self.ret_at) as u32,
        });
        self.jump(Op::Jmp, &over);
        let top = self.top;
        let ty = self.expr(expr)?;
        let size = self.size(&ty);
        self.movsp(-size);
        self.asm.op(Op::Retn);
        self.top = top;
        self.mark(&over);
        Ok(())
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Done<Ty> {
        if let Some(function) = self.scope.functions.get(name).cloned() {
            let ret = function.ret.clone();
            self.reserve(&ret)?;
            let before = self.top;
            self.arguments(name, &function.params, args)?;
            // A function is written where its call is, and a call's
            // arguments are written before it: in `f(g())`, `g` comes first.
            if !self.calls.iter().any(|called| called == name) {
                self.calls.push(name.to_string());
            }
            self.asm.to(Op::Jsr, &format!("function {name}"));
            self.top = before;
            return Ok(ret);
        }
        let Some(routine) = self.scope.catalog.routine_named(name).cloned() else {
            return self.error(format!("{name} is no function"));
        };
        let params: Vec<Param> = routine
            .params
            .iter()
            .map(|param| Param {
                ty: of_catalog(param.ty),
                name: param.name.clone(),
                default: param
                    .default
                    .as_deref()
                    .and_then(|text| default_of(text, param.ty)),
            })
            .collect();
        let before = self.top;
        self.arguments(name, &params, args)?;
        self.asm.op(Op::Action {
            routine: routine.id,
            args: params.len() as u8,
        });
        let ret = of_catalog(routine.ret);
        self.top = before + self.size(&ret);
        Ok(ret)
    }

    /// Pushes a call's arguments, the last first, with the parameters'
    /// defaults for those not given.
    fn arguments(&mut self, name: &str, params: &[Param], args: &[Expr]) -> Done<()> {
        if args.len() > params.len() {
            return self.error(format!("{name} takes {} arguments", params.len()));
        }
        for (at, param) in params.iter().enumerate().rev() {
            let arg = match args.get(at) {
                Some(arg) => arg.clone(),
                None => match &param.default {
                    Some(default) => default.clone(),
                    None => {
                        return self.error(format!("{name} has no value for its {}", param.name));
                    }
                },
            };
            if param.ty == Ty::Action {
                self.action(&arg)?;
                continue;
            }
            let given = self.expr(&arg)?;
            if given != param.ty {
                return self.error(format!(
                    "{name}'s {} is a {:?}, not a {given:?}",
                    param.name, param.ty
                ));
            }
        }
        Ok(())
    }

    fn statements(&mut self, statements: &[Located<Stmt>]) -> Done<()> {
        for statement in statements {
            self.statement(statement)?;
        }
        Ok(())
    }

    /// Opens a block's scope.
    fn open(&mut self) {
        self.locals.push(Vec::new());
        self.noted.push(Vec::new());
    }

    /// Closes a block's scope: its locals popped.
    fn close(&mut self) {
        let here = self.asm.code.len();
        for at in self.noted.pop().unwrap_or_else(Vec::new) {
            if let Some(local) = self.asm.debug.locals.get_mut(at) {
                local.5 = here;
            }
        }
        let scope = self.locals.pop().unwrap_or_else(Vec::new);
        let size: i32 = scope.iter().map(|(_, var)| self.size(&var.ty)).sum();
        self.movsp(-size);
    }

    fn statement(&mut self, statement: &Located<Stmt>) -> Done<()> {
        self.line = statement.line;
        let here = self.asm.code.len();
        // A statement written as no code (an empty one, a block's opening)
        // starts no line of its own.
        if !matches!(statement.item, Stmt::Block(_) | Stmt::Empty) {
            self.asm
                .debug
                .lines
                .push((here, self.file.clone(), statement.line));
        }
        self.stmt(&statement.item)
    }

    fn stmt(&mut self, statement: &Stmt) -> Done<()> {
        match statement {
            Stmt::Empty => {}
            Stmt::Block(statements) => {
                self.open();
                self.statements(statements)?;
                self.close();
            }
            Stmt::Declare(ty, names) => {
                for (name, value) in names {
                    self.reserve(ty)?;
                    let size = self.size(ty);
                    let start = self.top - size;
                    let var = Var {
                        ty: ty.clone(),
                        home: Home::Local(start),
                    };
                    self.locals
                        .last_mut()
                        .expect("a function's scope")
                        .push((name.clone(), var));
                    let noted = self.asm.debug.locals.len();
                    self.asm.debug.locals.push((
                        self.function.clone(),
                        name.clone(),
                        ty_name(ty),
                        start,
                        self.asm.code.len(),
                        usize::MAX,
                    ));
                    if let Some(scope) = self.noted.last_mut() {
                        scope.push(noted);
                    }
                    if let Some(value) = value {
                        let given = self.expr(value)?;
                        if given != *ty {
                            return self.error(format!("{name} is a {ty:?}, not a {given:?}"));
                        }
                        self.copy_down(Home::Local(start), 0, size);
                        self.movsp(-size);
                    }
                }
            }
            Stmt::Expr(expr) => {
                let ty = self.expr(expr)?;
                let size = self.size(&ty);
                self.movsp(-size);
            }
            Stmt::If(condition, then, otherwise) => {
                if self.expr(condition)? != Ty::Int {
                    return self.error("an if's condition is an int");
                }
                let (other, end) = (self.label("else"), self.label("end of if"));
                self.jump(Op::Jz, &other);
                self.scoped(then)?;
                self.jump(Op::Jmp, &end);
                self.mark(&other);
                if let Some(otherwise) = otherwise {
                    self.scoped(otherwise)?;
                }
                self.mark(&end);
            }
            Stmt::While(condition, body) => {
                let (start, next, end) = (
                    self.label("while"),
                    self.label("while's next"),
                    self.label("end of while"),
                );
                self.mark(&start);
                if self.expr(condition)? != Ty::Int {
                    return self.error("a while's condition is an int");
                }
                self.jump(Op::Jz, &end);
                self.exits.push(Exit {
                    top: self.top,
                    broken: end.clone(),
                    continued: Some(next.clone()),
                });
                self.scoped(body)?;
                self.exits.pop();
                self.mark(&next);
                self.jump(Op::Jmp, &start);
                self.mark(&end);
            }
            Stmt::DoWhile(body, condition) => {
                let (start, test, end) = (
                    self.label("do"),
                    self.label("do's test"),
                    self.label("end of do"),
                );
                self.mark(&start);
                self.exits.push(Exit {
                    top: self.top,
                    broken: end.clone(),
                    continued: Some(test.clone()),
                });
                self.scoped(body)?;
                self.exits.pop();
                self.mark(&test);
                if self.expr(condition)? != Ty::Int {
                    return self.error("a do loop's condition is an int");
                }
                self.jump(Op::Jz, &end);
                self.jump(Op::Jmp, &start);
                self.mark(&end);
            }
            Stmt::For(init, condition, step, body) => {
                if let Some(init) = init {
                    self.stmt(&Stmt::Expr(init.clone()))?;
                }
                let (start, next, end) = (
                    self.label("for"),
                    self.label("for's step"),
                    self.label("end of for"),
                );
                self.mark(&start);
                if let Some(condition) = condition {
                    if self.expr(condition)? != Ty::Int {
                        return self.error("a for's condition is an int");
                    }
                    self.jump(Op::Jz, &end);
                }
                self.exits.push(Exit {
                    top: self.top,
                    broken: end.clone(),
                    continued: Some(next.clone()),
                });
                self.scoped(body)?;
                self.exits.pop();
                self.mark(&next);
                if let Some(step) = step {
                    self.stmt(&Stmt::Expr(step.clone()))?;
                }
                self.jump(Op::Jmp, &start);
                self.mark(&end);
            }
            Stmt::Switch(value, parts) => {
                let ty = self.expr(value)?;
                let pair = match ty {
                    Ty::Int => Pair::IntInt,
                    Ty::String => Pair::StringString,
                    _ => return self.error("a switch takes an int or a string"),
                };
                let end = self.label("end of switch");
                let mut cases = Vec::new();
                let mut default = None;
                for part in parts {
                    match &part.item {
                        SwitchPart::Case(case) => {
                            let label = self.label("case");
                            self.op(Op::CpTopSp {
                                offset: -4,
                                size: 4,
                            });
                            // A case of another type was compared as the
                            // switch's own type, which stopped the script
                            // when it ran. One of three cells, a vector,
                            // also put every later place on the stack off.
                            let case_ty = self.expr(case)?;
                            if case_ty != ty {
                                return self.error(format!(
                                    "a case of this switch is a {case_ty:?}, not a {ty:?}"
                                ));
                            }
                            self.asm.op(Op::Equal(pair));
                            self.top -= 4;
                            self.jump(Op::Jnz, &label);
                            cases.push(label);
                        }
                        SwitchPart::Default => default = Some(self.label("default")),
                        SwitchPart::Stmt(_) => {}
                    }
                }
                self.jump(Op::Jmp, default.as_deref().unwrap_or(&end));
                self.exits.push(Exit {
                    top: self.top,
                    broken: end.clone(),
                    continued: None,
                });
                let mut next_case = cases.into_iter();
                self.open();
                for part in parts {
                    self.line = part.line;
                    match &part.item {
                        SwitchPart::Case(_) => {
                            let label = next_case.next().expect("one for each case");
                            self.mark(&label);
                        }
                        SwitchPart::Default => {
                            let label = default.clone().expect("named above");
                            self.mark(&label);
                        }
                        SwitchPart::Stmt(statement) => self.stmt(statement)?,
                    }
                }
                self.close();
                self.exits.pop();
                self.mark(&end);
                self.movsp(-4);
            }
            Stmt::Break | Stmt::Continue => {
                let breaks = matches!(statement, Stmt::Break);
                // What the jump leaves: a break the innermost loop or
                // switch, a continue the innermost loop, with any switch
                // inside it.
                let left = if breaks {
                    self.exits.last()
                } else {
                    (self.exits.iter().rev()).find(|exit| exit.continued.is_some())
                };
                let Some(left) = left else {
                    return self.error(if breaks {
                        "a break is outside a loop or a switch"
                    } else {
                        "a continue is outside a loop"
                    });
                };
                let in_loop = left.continued.is_some();
                let target = match (&left.continued, breaks) {
                    (Some(next), false) => next.clone(),
                    _ => left.broken.clone(),
                };
                let (top, depth) = (self.top, left.top);
                let pops = match self.scope.leaving {
                    // The locals of the blocks it leaves are popped, and a
                    // continue out of a switch pops the switch's value too.
                    // Left on the stack, they put every later place of the
                    // function off.
                    Leaving::Clean => true,
                    // BioWare's compiler pops them only for a break out of
                    // a loop. A continue, and a break out of a switch, pop
                    // nothing.
                    Leaving::AsBioWare => breaks && in_loop,
                };
                if depth != top && pops {
                    self.asm.op(Op::MovSp(depth - top));
                }
                self.asm.to(Op::Jmp, &target);
            }
            Stmt::Return(value) => {
                let top = self.top;
                match value {
                    Some(value) => {
                        let ty = self.expr(value)?;
                        let size = self.size(&ty);
                        if ty != self.ret {
                            return self.error(format!("the function returns a {:?}", self.ret));
                        }
                        let at = self.ret_at;
                        self.copy_down(Home::Local(at), 0, size);
                        let all = self.top;
                        if all != 0 {
                            self.asm.op(Op::MovSp(-all));
                        }
                        let end = self.end.clone();
                        self.asm.to(Op::Jmp, &end);
                        // What follows is written as if the value were popped.
                        self.top = top + size;
                        self.movsp(-size);
                    }
                    None => {
                        if self.ret != Ty::Void {
                            return self.error(format!(
                                "the function returns a {:?}, so its return needs a value",
                                self.ret
                            ));
                        }
                        if top != 0 {
                            self.asm.op(Op::MovSp(-top));
                        }
                        let end = self.end.clone();
                        self.asm.to(Op::Jmp, &end);
                    }
                }
                self.top = top;
            }
        }
        Ok(())
    }

    /// A statement in a scope of its own when it is a block.
    fn scoped(&mut self, statement: &Located<Stmt>) -> Done<()> {
        self.statement(statement)
    }
}

/// A routine's default (`nwscript.nss`'s text) as a value.
fn default_of(text: &str, ty: catalog::Type) -> Option<Expr> {
    let text = text.trim();
    let number = |text: &str| text.trim().trim_end_matches('f').parse::<f32>().ok();
    Some(match ty {
        catalog::Type::Object => Expr::Name(text.to_string()),
        catalog::Type::Vector => {
            let inner = text.strip_prefix('[')?.strip_suffix(']')?;
            let parts: Vec<f32> = inner.split(',').map(number).collect::<Option<_>>()?;
            Expr::Vector(parts.try_into().ok()?)
        }
        catalog::Type::String => Expr::Str(
            text.strip_prefix('"')?
                .strip_suffix('"')?
                .as_bytes()
                .to_vec(),
        ),
        catalog::Type::Int => match text.parse::<i32>() {
            Ok(value) => Expr::Int(value),
            Err(_) => match text
                .strip_prefix('-')
                .and_then(|rest| rest.parse::<i32>().ok())
            {
                Some(value) => Expr::Int(-value),
                None => Expr::Name(text.to_string()),
            },
        },
        catalog::Type::Float => match number(text) {
            Some(value) => Expr::Float(value),
            None => Expr::Name(text.to_string()),
        },
        _ => return None,
    })
}

/// A `#define`'s value as an expression, where it is one number or one
/// string and nothing more. A number may have a minus sign before it, which
/// is made part of the number: the decompiler writes such a `#define` for a
/// negative number a compiled script holds whole (`decompile`).
///
/// Any other value gives `None`. Before, only the first part was read:
/// `1 + 2` was 1, and a minus sign before a string was dropped. The
/// smallest int negated also stopped a debug build.
fn define_value(text: &str) -> Option<Expr> {
    use super::lexer::Tok;
    let tokens = super::lexer::lex(text.as_bytes(), "").ok()?;
    let negative = matches!(tokens.first()?.tok, Tok::Mark("-"));
    let at = usize::from(negative);
    let value = match &tokens.get(at)?.tok {
        Tok::Int(v) => Expr::Int(if negative { v.wrapping_neg() } else { *v }),
        Tok::Float(v) => Expr::Float(if negative { -v } else { *v }),
        Tok::Str(v) if !negative => Expr::Str(v.clone()),
        _ => return None,
    };
    matches!(tokens.get(at + 1)?.tok, Tok::End).then_some(value)
}

/// The program of `items` (the script's and its includes', in order).
pub(super) fn program(
    name: &str,
    items: &[(String, Item)],
    catalog: &Catalog,
    leaving: Leaving,
) -> Result<(Ncs, crate::compile::Debug), Error> {
    let mut scope = Scope {
        file: name.to_string(),
        catalog,
        structs: BTreeMap::new(),
        constants: BTreeMap::new(),
        globals: BTreeMap::new(),
        globals_size: 0,
        functions: BTreeMap::new(),
        leaving,
        unread_defines: BTreeSet::new(),
    };
    // The globals' values, and the file each function is defined in.
    let mut globals: Vec<(String, Ty, Option<Expr>)> = Vec::new();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for (file, item) in items {
        // A mistake in what the file declares. An item keeps no line.
        let wrong = |what: String| Error {
            file: file.clone(),
            line: 0,
            what,
        };
        match item {
            Item::Include(_) => {}
            Item::Define(name, value) => match define_value(value) {
                Some(value) => {
                    scope.constants.insert(name.clone(), value);
                }
                // Named where a script uses it, so that a file may hold a
                // `#define` of another kind that nothing uses.
                None => {
                    scope.unread_defines.insert(name.clone());
                }
            },
            Item::Struct(name, fields) => {
                // A field that is a struct is one declared before this one.
                // So no struct holds itself, in a field or in a field's
                // field. The size of one that did was worked out without
                // end, and the compiler ran out of stack.
                for (ty, field) in fields {
                    if let Some(why) = undeclared(ty, &scope.structs) {
                        return Err(wrong(format!("the struct {name}'s {field}: {why}")));
                    }
                }
                scope.structs.insert(name.clone(), fields.clone());
            }
            Item::Global {
                constant,
                ty,
                names,
            } => {
                for (name, value) in names {
                    // A number with a minus sign is a literal too. The
                    // parser keeps the sign apart from the number
                    // (`Parser::unary`), so `const int NONE = -1;` used to
                    // become a global variable. The constant keeps the
                    // sign apart as well, and each use of it is written as
                    // `-1` is written anywhere else.
                    let literal = match value {
                        Some(Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Vector(_)) => {
                            true
                        }
                        Some(Expr::Unary("-", number)) => {
                            matches!(**number, Expr::Int(_) | Expr::Float(_))
                        }
                        _ => false,
                    };
                    if *constant && literal {
                        scope
                            .constants
                            .insert(name.clone(), value.clone().expect("a literal"));
                    } else {
                        globals.push((name.clone(), ty.clone(), value.clone()));
                    }
                }
            }
            Item::Function(function) => {
                let types = (std::iter::once(&function.ret))
                    .chain(function.params.iter().map(|param| &param.ty));
                for ty in types {
                    if let Some(why) = undeclared(ty, &scope.structs) {
                        return Err(wrong(format!("{}: {why}", function.name)));
                    }
                }
                let had = scope.functions.get(&function.name);
                // A definition takes the place of a prototype, keeping the
                // prototype's defaults where it has none of its own.
                let mut function = function.clone();
                if let Some(had) = had {
                    // A prototype and its definition agree in what they
                    // take, and a function has one body. The later of two
                    // used to take the earlier one's place without a word.
                    //
                    // What they return is not compared, and the body's is
                    // taken. BioWare's compiler takes it so too: KOTOR 1's
                    // `k_inc_ebonhawk` declares `void EBO_PlayTakeOff` and
                    // defines it as returning a string, and four scripts
                    // that include it compile to the game's bytes.
                    let same = had.params.len() == function.params.len()
                        && (had.params.iter().zip(&function.params)).all(|(a, b)| a.ty == b.ty);
                    if !same {
                        return Err(wrong(format!(
                            "{} is declared twice, with other parameters the second time",
                            function.name
                        )));
                    }
                    if function.body.is_none() {
                        continue;
                    }
                    if had.body.is_some() {
                        return Err(wrong(format!("{} is defined twice", function.name)));
                    }
                    for (param, old) in function.params.iter_mut().zip(&had.params) {
                        if param.default.is_none() {
                            param.default = old.default.clone();
                        }
                    }
                }
                if function.body.is_some() {
                    files.insert(function.name.clone(), file.clone());
                }
                scope.functions.insert(function.name.clone(), function);
            }
        }
    }
    let entry = ["main", "StartingConditional"]
        .into_iter()
        .find(|entry| {
            scope
                .functions
                .get(*entry)
                .is_some_and(|function| function.body.is_some())
        })
        .ok_or_else(|| Error {
            file: name.to_string(),
            line: 0,
            what: "the script has no main or StartingConditional".into(),
        })?;
    for (global, ty, _) in &globals {
        let start = scope.globals_size;
        scope.globals_size += size(ty, &scope.structs);
        scope.globals.insert(
            global.clone(),
            Var {
                ty: ty.clone(),
                home: Home::Global(start),
            },
        );
    }

    let mut asm = Asm::default();
    let conditional = entry == "StartingConditional";
    // The functions that the globals' values call, and the labels their code
    // used up.
    let mut globals_calls: Vec<String> = Vec::new();
    let mut labels = 0usize;
    // The start: the globals set, then the entry called.
    if globals.is_empty() {
        if conditional {
            asm.op(Op::Rsadd(Cell::Int));
        }
        asm.to(Op::Jsr, &format!("function {entry}"));
        asm.op(Op::Retn);
    } else {
        if conditional {
            asm.op(Op::Rsadd(Cell::Int));
        }
        asm.to(Op::Jsr, "globals");
        asm.op(Op::Retn);
        asm.label("globals");
        let globals_first = asm.code.len();
        let mut writer = Writer {
            scope: &scope,
            asm: &mut asm,
            top: 0,
            locals: vec![Vec::new()],
            exits: Vec::new(),
            ret: Ty::Void,
            ret_at: 0,
            end: "end of globals".into(),
            line: 0,
            labels: 0,
            calls: Vec::new(),
            file: name.to_string(),
            function: "globals".into(),
            noted: vec![Vec::new()],
            setting_globals: true,
            expressions: 0,
        };
        for (global, ty, value) in &globals {
            writer.reserve(ty)?;
            if let Some(value) = value {
                let given = writer.expr(value)?;
                if given != *ty {
                    return writer.error(format!("{global} is a {ty:?}, not a {given:?}"));
                }
                let home = scope.globals[global].home;
                let Home::Global(start) = home else {
                    unreachable!("a global")
                };
                let size = writer.size(ty);
                writer.copy_down(Home::Local(start), 0, size);
                writer.movsp(-size);
            }
            // A later global's value reads this one from the stack, where
            // it lies as a local of this code. The globals are not yet
            // found through `SaveBp`, which comes after the last of them,
            // so a read as a global underflowed.
            let Home::Global(start) = scope.globals[global].home else {
                unreachable!("a global")
            };
            writer.locals[0].push((
                global.clone(),
                Var {
                    ty: ty.clone(),
                    home: Home::Local(start),
                },
            ));
        }
        writer.asm.op(Op::SaveBp);
        if conditional {
            writer.asm.op(Op::Rsadd(Cell::Int));
        }
        writer.asm.to(Op::Jsr, &format!("function {entry}"));
        if conditional {
            writer.asm.op(Op::CpDownSp {
                offset: -12 - scope.globals_size,
                size: 4,
            });
            writer.asm.op(Op::MovSp(-4));
        }
        writer.asm.op(Op::RestoreBp);
        writer.asm.op(Op::MovSp(-scope.globals_size));
        writer.asm.op(Op::Retn);
        // The function that sets the globals and calls the script's own,
        // named for a debugger's calls.
        let past = writer.asm.code.len();
        writer.asm.debug.functions.push((
            "(globals)".to_string(),
            name.to_string(),
            globals_first,
            past,
        ));
        globals_calls = std::mem::take(&mut writer.calls);
        labels = writer.labels;
    }

    // The entry, then each function as the code first calls it, each one's
    // own calls written before its caller's next. After them come the
    // functions that only the globals' values call. They used to be left
    // out, and the call jumped to the start of the file.
    let mut stack: Vec<String> = globals_calls.into_iter().rev().collect();
    stack.push(entry.to_string());
    let mut written: BTreeSet<String> = BTreeSet::new();
    // Depth first: a function's own calls are written before the next
    // function its caller calls, as BioWare's compiler orders them.
    while let Some(next) = stack.pop() {
        if !written.insert(next.clone()) {
            continue;
        }
        let function = scope.functions[&next].clone();
        let Some(body) = function.body.clone() else {
            return Err(Error {
                file: name.to_string(),
                line: 0,
                what: format!("{next} is called but never defined"),
            });
        };
        let params_size: i32 = function
            .params
            .iter()
            .map(|param| size(&param.ty, &scope.structs))
            .sum();
        let ret_size = size(&function.ret, &scope.structs);
        let mut writer = Writer {
            scope: &scope,
            asm: &mut asm,
            top: 0,
            locals: vec![Vec::new(), Vec::new()],
            exits: Vec::new(),
            ret: function.ret.clone(),
            ret_at: -params_size - ret_size,
            end: format!("end of {next}"),
            line: 0,
            labels,
            calls: Vec::new(),
            file: files
                .get(&next)
                .cloned()
                .unwrap_or_else(|| name.to_string()),
            function: next.clone(),
            noted: vec![Vec::new(), Vec::new()],
            setting_globals: false,
            expressions: 0,
        };
        // The parameters lie below the frame, the first nearest it.
        let mut at = 0;
        for param in &function.params {
            at -= size(&param.ty, &scope.structs);
            writer.locals[0].push((
                param.name.clone(),
                Var {
                    ty: param.ty.clone(),
                    home: Home::Local(at),
                },
            ));
        }
        writer.asm.label(&format!("function {next}"));
        let first = writer.asm.code.len();
        // The parameters, noted for the whole function.
        let mut at = 0;
        let mut params_noted = Vec::new();
        for param in &function.params {
            at -= size(&param.ty, &scope.structs);
            params_noted.push(writer.asm.debug.locals.len());
            writer.asm.debug.locals.push((
                next.clone(),
                param.name.clone(),
                ty_name(&param.ty),
                at,
                first,
                usize::MAX,
            ));
        }
        writer.statements(&body)?;
        writer.close();
        let end = writer.end.clone();
        writer.asm.label(&end);
        if params_size != 0 {
            writer.asm.op(Op::MovSp(-params_size));
        }
        writer.asm.op(Op::Retn);
        let past = writer.asm.code.len();
        for noted in params_noted {
            writer.asm.debug.locals[noted].5 = past;
        }
        let file = writer.file.clone();
        writer
            .asm
            .debug
            .functions
            .push((next.clone(), file, first, past));
        labels = writer.labels;
        let calls = std::mem::take(&mut writer.calls);
        stack.extend(
            calls
                .into_iter()
                .rev()
                .filter(|called| !written.contains(called)),
        );
    }
    let ncs = asm.build();
    let globals_noted = scope
        .globals
        .iter()
        .filter_map(|(name, var)| match var.home {
            Home::Global(at) => Some((name.clone(), ty_name(&var.ty), at)),
            Home::Local(_) => None,
        })
        .collect();
    let debug = crate::compile::Debug::of(&asm.debug, &ncs, globals_noted, scope.globals_size);
    Ok((ncs, debug))
}
