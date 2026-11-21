//! One function's instructions read back into statements: the stack the
//! compiler kept (`compile`'s `Writer`) is kept again, and each pattern it
//! writes is taken for the statement or expression that wrote it.

use std::collections::BTreeMap;

use fmt_odyssey::ncs::{Const, Instruction, Op, Pair};

use super::{Error, Program, Sig, Structs, Var, aggregate, field_name, of_cell, size};
use crate::catalog::{self, Catalog};
use crate::compile::parse::{Expr, Function, Located, Param, Stmt, SwitchPart, Ty};

type Done<T> = Result<T, Error>;

/// What lies on the stack above the variables while a statement is read.
#[derive(Debug, Clone)]
enum Entry {
    /// A value an expression pushed.
    Val(Expr, Ty),
    /// A cell reserved empty (`RSADD`): a variable being declared, or the
    /// result of a call whose arguments follow.
    Reserved(Ty),
    /// An action argument's code, read already. It takes no room.
    Action(Expr),
}

/// One thing of a block being read: a statement, or a switch's label.
#[derive(Debug, Clone)]
enum Part {
    Stmt(Stmt),
    Case(Expr),
    Default,
}

/// What a `break` or a `continue` leaves.
#[derive(Debug)]
enum Exit {
    Loop {
        /// The instruction after the loop.
        end: usize,
        /// The jump back to the loop's start.
        back: usize,
        /// Where a `continue` of it jumps, once one is seen.
        next: Option<usize>,
        /// The depth its body is read at, and the body's length when the
        /// reader reached `next` (a `for`'s step starts there).
        body_depth: usize,
        mark: Option<usize>,
    },
    Switch {
        /// Each case's label and value, and whether it is written yet.
        labels: Vec<(usize, Expr, bool)>,
        /// The `default`'s label, or the switch's end when it has none.
        default: usize,
        default_done: bool,
    },
}

/// The kind of block a run reads.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    /// A switch's body: it ends where the switch's value is popped.
    Switch,
    /// A `do` loop's body and its test, whose value is left on the stack.
    Do,
}

fn located(item: Stmt) -> Located<Stmt> {
    Located { line: 0, item }
}

fn block(statements: Vec<Stmt>) -> Box<Located<Stmt>> {
    Box::new(located(Stmt::Block(
        statements.into_iter().map(located).collect(),
    )))
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

/// The first letter of a variable's name, by its type.
fn letter(ty: &Ty) -> &'static str {
    match ty {
        Ty::Int => "n",
        Ty::Float => "f",
        Ty::String => "s",
        Ty::Object => "o",
        Ty::Vector => "v",
        Ty::Effect => "e",
        Ty::Event => "ev",
        Ty::Location => "l",
        Ty::Talent => "t",
        _ => "x",
    }
}

/// The sizes of a binary operator's two operands, bytes.
fn pair_sizes(pair: Pair) -> (i32, i32) {
    match pair {
        Pair::VectorVector => (12, 12),
        Pair::VectorFloat => (12, 4),
        Pair::FloatVector => (4, 12),
        Pair::Struct(size) => (i32::from(size), i32::from(size)),
        _ => (4, 4),
    }
}

pub(super) struct Machine<'a> {
    code: &'a [Instruction],
    targets: &'a [Option<usize>],
    catalog: &'a Catalog,
    sigs: &'a mut Vec<Sig>,
    structs: &'a mut Structs,
    by_start: &'a BTreeMap<usize, usize>,
    globals: &'a [Var],
    globals_size: i32,
    /// Locals first declared cell by cell and then found to be one vector
    /// or structure: the cells' names, and the whole's name and type.
    merges: Vec<(Vec<String>, String, Ty)>,
    /// The routines called with fewer arguments than `nwscript.nss` gives
    /// them parameters, and with how many (`Decompiled::routines`).
    pub(super) fewer: BTreeMap<String, usize>,
    /// Reading the globals' function: its variables are the globals.
    global_mode: bool,
    pc: usize,
    stack: Vec<Entry>,
    /// Action arguments being read, one in another.
    actions: usize,
    vars: Vec<Var>,
    vars_top: i32,
    params: Vec<Var>,
    ret_at: i32,
    ret_size: i32,
    fn_end: usize,
    parts: Vec<Part>,
    exits: Vec<Exit>,
    /// The loops being read, by their jumps back.
    open_backs: Vec<usize>,
    /// Each loop's start and the jumps back to it.
    backs: BTreeMap<usize, Vec<usize>>,
    named: usize,
    depth: usize,
}

impl<'a> Machine<'a> {
    pub(super) fn new(
        program: &Program<'a>,
        sigs: &'a mut Vec<Sig>,
        structs: &'a mut Structs,
        globals: &'a [Var],
    ) -> Machine<'a> {
        Machine {
            code: program.code,
            targets: program.targets,
            catalog: program.catalog,
            sigs,
            structs,
            by_start: program.by_start,
            globals,
            globals_size: program.globals_size,
            merges: Vec::new(),
            fewer: BTreeMap::new(),
            global_mode: false,
            pc: 0,
            stack: Vec::new(),
            actions: 0,
            vars: Vec::new(),
            vars_top: 0,
            params: Vec::new(),
            ret_at: 0,
            ret_size: 0,
            fn_end: usize::MAX,
            parts: Vec::new(),
            exits: Vec::new(),
            open_backs: Vec::new(),
            backs: BTreeMap::new(),
            named: 0,
            depth: 0,
        }
    }

    fn fail<T>(&self, what: impl Into<String>) -> Done<T> {
        Err(Error {
            at: self.pc,
            what: what.into(),
        })
    }

    fn op(&self, at: usize) -> Option<&'a Op> {
        self.code.get(at).map(|instruction| &instruction.op)
    }

    fn target(&self, at: usize) -> Done<usize> {
        match self.targets.get(at) {
            Some(Some(to)) => Ok(*to),
            _ => self.fail("a jump lands on no instruction"),
        }
    }

    fn entry_size(entry: &Entry) -> i32 {
        match entry {
            Entry::Val(_, ty) | Entry::Reserved(ty) => size(ty),
            Entry::Action(_) => 0,
        }
    }

    fn top(&self) -> i32 {
        self.vars_top + self.stack.iter().map(Self::entry_size).sum::<i32>()
    }

    fn fresh(&mut self, ty: &Ty, kind: &str) -> String {
        self.named += 1;
        format!("{}{kind}{}", letter(ty), self.named)
    }

    fn emit(&mut self, statement: Stmt) {
        self.parts.push(Part::Stmt(statement));
    }

    /// How many cells at the stack's bottom are reserved and empty: the
    /// variables a statement's start declares.
    fn prefix(&self) -> usize {
        self.stack
            .iter()
            .take_while(|entry| matches!(entry, Entry::Reserved(_)))
            .count()
    }

    fn declare(&mut self, ty: Ty, value: Option<Expr>) {
        let kind = if self.global_mode { "Global" } else { "Local" };
        let name = self.fresh(&ty, kind);
        self.vars.push(Var {
            name: name.clone(),
            ty: ty.clone(),
            start: self.vars_top,
            switch: false,
            part: self.parts.len(),
        });
        self.vars_top += size(&ty);
        self.emit(Stmt::Declare(ty, vec![(name, value)]));
    }

    /// The first `count` reserved cells become variables declared with no
    /// value.
    fn flush(&mut self, count: usize) {
        let reserved: Vec<Entry> = self.stack.drain(..count).collect();
        for entry in reserved {
            if let Entry::Reserved(ty) = entry {
                self.declare(ty, None);
            }
        }
    }

    /// Pops a value `bytes` long: one pushed whole, or a vector's three
    /// numbers.
    fn pop_val(&mut self, bytes: i32) -> Done<(Expr, Ty)> {
        match self.stack.last() {
            Some(Entry::Val(_, ty)) if size(ty) == bytes => {
                let Some(Entry::Val(expr, ty)) = self.stack.pop() else {
                    unreachable!("a value")
                };
                Ok((expr, ty))
            }
            Some(Entry::Val(Expr::Float(_), _)) if bytes == 12 && self.stack.len() >= 3 => {
                let mut parts = [0.0f32; 3];
                for at in (0..3).rev() {
                    match self.stack.pop() {
                        Some(Entry::Val(Expr::Float(value), _)) => parts[at] = value,
                        _ => return self.fail("a vector is not three numbers"),
                    }
                }
                Ok((Expr::Vector(parts), Ty::Vector))
            }
            _ => self.fail(format!("no value of {bytes} bytes is on the stack")),
        }
    }

    fn pop_action(&mut self) -> Done<Expr> {
        match self.stack.pop() {
            Some(Entry::Action(expr)) => Ok(expr),
            _ => self.fail("a routine's action argument is missing"),
        }
    }

    /// The variable (or the part of one) `bytes` long at `at` among
    /// `vars`.
    fn in_vars(structs: &Structs, vars: &[Var], at: i32, bytes: i32) -> Option<(Expr, Ty)> {
        for var in vars {
            let own = size(&var.ty);
            if var.switch || at < var.start || at >= var.start + own {
                continue;
            }
            if at == var.start && bytes == own {
                return Some((Expr::Name(var.name.clone()), var.ty.clone()));
            }
            let index = ((at - var.start) / 4) as usize;
            let base = Box::new(Expr::Name(var.name.clone()));
            if var.ty == Ty::Vector && bytes == 4 {
                let field = ["x", "y", "z"][index];
                return Some((Expr::Field(base, field.into()), Ty::Float));
            }
            if let (Ty::Struct(name), 4) = (&var.ty, bytes) {
                let cells = structs
                    .iter()
                    .find(|(own, _)| own == name)
                    .map(|(_, cells)| cells)?;
                return Some((
                    Expr::Field(base, field_name(index)),
                    cells.get(index)?.clone(),
                ));
            }
        }
        None
    }

    /// The locals that fill `at..at + bytes` cell by cell become one vector
    /// or structure, as a copy of them all at once shows them to be.
    fn merge(&mut self, at: i32, bytes: i32) -> bool {
        let Some(first) = self.vars.iter().position(|var| var.start == at) else {
            return false;
        };
        let count = (bytes / 4) as usize;
        let Some(cells) = self.vars.get(first..first + count) else {
            return false;
        };
        if cells.iter().any(|var| var.switch || size(&var.ty) != 4) {
            return false;
        }
        let types: Vec<Ty> = cells.iter().map(|var| var.ty.clone()).collect();
        let names: Vec<String> = cells.iter().map(|var| var.name.clone()).collect();
        let part = cells[0].part;
        let ty = aggregate(self.structs, &types);
        let name = self.fresh(&ty, "Local");
        self.merges.push((names, name.clone(), ty.clone()));
        self.vars.splice(
            first..first + count,
            [Var {
                name,
                ty,
                start: at,
                switch: false,
                part,
            }],
        );
        true
    }

    /// The local or parameter at `at` (bytes from the frame's base). A
    /// reserved cell there is declared first.
    fn local(&mut self, at: i32, bytes: i32) -> Done<(Expr, Ty)> {
        if at >= self.vars_top {
            let mut seen = self.vars_top;
            let mut count = 0;
            for entry in &self.stack {
                if !matches!(entry, Entry::Reserved(_)) || seen > at {
                    break;
                }
                seen += Self::entry_size(entry);
                count += 1;
            }
            if seen <= at {
                return self.fail("a copy reaches into a value being worked out");
            }
            self.flush(count);
        }
        let mut found = if at < 0 {
            Self::in_vars(self.structs, &self.params, at, bytes)
        } else {
            Self::in_vars(self.structs, &self.vars, at, bytes)
        };
        if found.is_none() && at >= 0 && bytes > 4 && self.merge(at, bytes) {
            found = Self::in_vars(self.structs, &self.vars, at, bytes);
        }
        match found {
            Some(found) => Ok(found),
            None => self.fail(format!(
                "no variable of {bytes} bytes is at {at} (a vector or a structure not yet read)"
            )),
        }
    }

    fn global(&self, at: i32, bytes: i32) -> Done<(Expr, Ty)> {
        let vars = if self.global_mode {
            &self.vars[..]
        } else {
            self.globals
        };
        match Self::in_vars(self.structs, vars, at, bytes) {
            Some(found) => Ok(found),
            None => self.fail(format!("no global of {bytes} bytes is at {at}")),
        }
    }

    /// A void call is a statement by itself, unless it is an action
    /// argument's code.
    fn called(&mut self, call: Expr, ret: Ty) {
        if ret == Ty::Void && self.actions == 0 && self.stack.len() == self.prefix() {
            let count = self.stack.len();
            self.flush(count);
            self.emit(Stmt::Expr(call));
        } else {
            self.stack.push(Entry::Val(call, ret));
        }
    }

    fn binary(&mut self, mark: &'static str, pair: Pair) -> Done<()> {
        let (left_size, right_size) = pair_sizes(pair);
        let (right, right_ty) = self.pop_val(right_size)?;
        let (left, left_ty) = self.pop_val(left_size)?;
        let ints = matches!(pair, Pair::IntInt);
        let vector = |ty: &Ty| *ty == Ty::Vector;
        let ty = match mark {
            "==" | "!=" | "<" | ">" | "<=" | ">=" => Ty::Int,
            _ if ints => Ty::Int,
            "+" => match pair {
                Pair::StringString => Ty::String,
                Pair::VectorVector => Ty::Vector,
                _ => Ty::Float,
            },
            "*" if vector(&left_ty) || vector(&right_ty) => Ty::Vector,
            "-" | "/" if vector(&left_ty) => Ty::Vector,
            _ => Ty::Float,
        };
        self.stack.push(Entry::Val(
            Expr::Binary(mark, Box::new(left), Box::new(right)),
            ty,
        ));
        self.pc += 1;
        Ok(())
    }

    fn int_binary(&mut self, mark: &'static str) -> Done<()> {
        self.binary(mark, Pair::IntInt)
    }

    fn unary(&mut self, mark: &'static str) -> Done<()> {
        let (operand, ty) = self.pop_val(4)?;
        self.stack
            .push(Entry::Val(Expr::Unary(mark, Box::new(operand)), ty));
        self.pc += 1;
        Ok(())
    }

    /// `++` or `--` of the int at `place`: after its value was just copied
    /// up, or before the copy that follows.
    fn step(&mut self, mark: &'static str, place: Expr) -> Done<()> {
        let just_read = matches!(self.stack.last(), Some(Entry::Val(read, _)) if *read == place)
            && matches!(
                self.op(self.pc.wrapping_sub(1)),
                Some(Op::CpTopSp { .. } | Op::CpTopBp { .. })
            );
        if just_read {
            self.stack.pop();
            self.stack.push(Entry::Val(
                Expr::Step(mark, false, Box::new(place)),
                Ty::Int,
            ));
            self.pc += 1;
            return Ok(());
        }
        self.pc += 1;
        let before = self.stack.len();
        self.eval()?;
        match self.stack.last() {
            Some(Entry::Val(read, _)) if self.stack.len() == before + 1 && *read == place => {
                self.stack.pop();
                self.stack
                    .push(Entry::Val(Expr::Step(mark, true, Box::new(place)), Ty::Int));
                Ok(())
            }
            _ => self.fail("a step is not followed by a copy of what it stepped"),
        }
    }

    /// An action argument: `STORESTATE`, a jump over its code, the code and
    /// a `RETN`.
    fn action(&mut self) -> Done<()> {
        let over = match self.op(self.pc + 1) {
            Some(Op::Jmp(_)) => self.target(self.pc + 1)?,
            _ => return self.fail("STORESTATE is not followed by a jump"),
        };
        if over < self.pc + 3 || !matches!(self.op(over - 1), Some(Op::Retn)) {
            return self.fail("an action's code does not end with RETN");
        }
        let before = self.stack.len();
        self.actions += 1;
        self.pc += 2;
        while self.pc < over - 1 {
            // The code's value, when it has one, is popped before the RETN.
            if self.pc == over - 2
                && let Some(Op::MovSp(by)) = self.op(self.pc)
                && self.stack.len() == before + 1
                && self.stack.last().map_or(0, Self::entry_size) == -*by
            {
                self.pc += 1;
                continue;
            }
            self.eval()?;
        }
        self.actions -= 1;
        if self.stack.len() != before + 1 {
            return self.fail("an action's code is not one expression");
        }
        let Some(Entry::Val(expr, _)) = self.stack.pop() else {
            return self.fail("an action's code is not one expression");
        };
        self.stack.push(Entry::Action(expr));
        self.pc = over;
        Ok(())
    }

    fn routine(&mut self, routine: u16, args: u8) -> Done<()> {
        let Some(routine) = self.catalog.routine_by_id(routine) else {
            return self.fail(format!("routine {routine} is not in nwscript.nss"));
        };
        let mut given = Vec::new();
        for param in routine.params.iter().take(usize::from(args)) {
            let ty = of_catalog(param.ty);
            given.push(match ty {
                Ty::Action => self.pop_action()?,
                other => self.pop_val(size(&other))?.0,
            });
        }
        if given.len() != usize::from(args) {
            return self.fail(format!("{} takes fewer arguments", routine.name));
        }
        if given.len() < routine.params.len() {
            let had = self.fewer.insert(routine.name.clone(), given.len());
            if had.is_some_and(|had| had != given.len()) {
                return self.fail(format!(
                    "{} is called with two numbers of arguments",
                    routine.name
                ));
            }
        }
        self.pc += 1;
        self.called(
            Expr::Call(routine.name.clone(), given),
            of_catalog(routine.ret),
        );
        Ok(())
    }

    /// A call of one of the program's own functions. The first call of a
    /// function tells its parameters' types.
    fn call(&mut self) -> Done<()> {
        let to = self.target(self.pc)?;
        let Some(&index) = self.by_start.get(&to) else {
            return self.fail("a call lands in no function");
        };
        let sig = self.sigs[index].clone();
        let mut args: Vec<(Expr, Ty)> = Vec::new();
        match (&sig.params, sig.params_size) {
            (Some(params), _) => {
                for ty in params {
                    args.push(self.pop_val(size(ty))?);
                }
            }
            (None, Some(bytes)) => {
                let mut left = bytes;
                while left > 0 {
                    let top = self.stack.last().map_or(0, Self::entry_size);
                    if top == 0 || top > left {
                        return self.fail("a call's arguments do not fill its parameters");
                    }
                    args.push(self.pop_val(top)?);
                    left -= top;
                }
            }
            (None, None) => {
                // No `return` of a value in it: every value of the
                // statement so far is an argument.
                while matches!(self.stack.last(), Some(Entry::Val(..))) {
                    let top = self.stack.last().map_or(0, Self::entry_size);
                    args.push(self.pop_val(top)?);
                }
            }
        }
        let ret = if sig.ret_size == 0 {
            Ty::Void
        } else {
            let mut cells = Vec::new();
            let mut left = sig.ret_size;
            while left > 0 {
                match self.stack.pop() {
                    Some(Entry::Reserved(ty)) => {
                        left -= size(&ty);
                        cells.push(ty);
                    }
                    _ => return self.fail("a call's result is not reserved before its arguments"),
                }
            }
            cells.reverse();
            match cells.as_slice() {
                [one] => one.clone(),
                many => aggregate(self.structs, many),
            }
        };
        let sig = &mut self.sigs[index];
        if sig.params.is_none() {
            sig.params = Some(args.iter().map(|(_, ty)| ty.clone()).collect());
        }
        if sig.ret.is_none() {
            sig.ret = Some(ret.clone());
        }
        let name = sig.name.clone();
        self.pc += 1;
        let args = args.into_iter().map(|(expr, _)| expr).collect();
        self.called(Expr::Call(name, args), ret);
        Ok(())
    }

    /// The top value put in the variable `bytes` long at `place`.
    fn assign(&mut self, place: (Expr, Ty), bytes: i32) -> Done<()> {
        let (value, ty) = self.pop_val(bytes)?;
        let assign = Expr::Assign("=", Box::new(place.0), Box::new(value));
        self.stack.push(Entry::Val(assign, ty));
        self.pc += 1;
        Ok(())
    }

    /// One instruction of an expression.
    fn eval(&mut self) -> Done<()> {
        let Some(op) = self.op(self.pc) else {
            return self.fail("the code ends inside a statement");
        };
        match op {
            Op::Rsadd(cell) => {
                self.stack.push(Entry::Reserved(of_cell(*cell)));
                self.pc += 1;
            }
            Op::Const(constant) => {
                let (expr, ty) = match constant {
                    Const::Int(value) => (Expr::Int(*value), Ty::Int),
                    Const::Float(value) => (Expr::Float(*value), Ty::Float),
                    Const::String(text) => (Expr::Str(text.clone()), Ty::String),
                    Const::Object(0) => (Expr::Name("OBJECT_SELF".into()), Ty::Object),
                    Const::Object(1) => (Expr::Name("OBJECT_INVALID".into()), Ty::Object),
                    Const::Object(other) => {
                        return self.fail(format!("the object constant {other} has no name"));
                    }
                };
                self.stack.push(Entry::Val(expr, ty));
                self.pc += 1;
            }
            Op::CpTopSp {
                offset,
                size: bytes,
            } => {
                let bytes = i32::from(*bytes);
                let at = self.top() + offset;
                let copies_top = *offset == -bytes
                    && matches!(self.stack.last(), Some(Entry::Val(_, ty)) if size(ty) == bytes);
                if copies_top {
                    // `&&` and `||` test a copy of their left side.
                    if !matches!(self.op(self.pc + 1), Some(Op::Jz(_))) {
                        return self.fail("a value is copied outside a logic operator or a switch");
                    }
                    let or = matches!(
                        self.op(self.pc + 2),
                        Some(Op::CpTopSp {
                            offset: -4,
                            size: 4
                        })
                    ) && matches!(self.op(self.pc + 3), Some(Op::Jz(_)))
                        && self.targets.get(self.pc + 1) == Some(&Some(self.pc + 4));
                    self.pc += if or { 4 } else { 2 };
                    return Ok(());
                }
                let (expr, ty) = self.local(at, bytes)?;
                self.stack.push(Entry::Val(expr, ty));
                self.pc += 1;
            }
            Op::CpTopBp {
                offset,
                size: bytes,
            } => {
                let (expr, ty) = self.global(self.globals_size + offset, i32::from(*bytes))?;
                self.stack.push(Entry::Val(expr, ty));
                self.pc += 1;
            }
            Op::CpDownSp {
                offset,
                size: bytes,
            } => {
                let at = self.top() + offset;
                let place = self.local(at, i32::from(*bytes))?;
                self.assign(place, i32::from(*bytes))?;
            }
            Op::CpDownBp {
                offset,
                size: bytes,
            } => {
                let place = self.global(self.globals_size + offset, i32::from(*bytes))?;
                self.assign(place, i32::from(*bytes))?;
            }
            Op::Action { routine, args } => self.routine(*routine, *args)?,
            Op::Jsr(_) => self.call()?,
            Op::StoreState { .. } => self.action()?,
            Op::LogAnd => self.int_binary("&&")?,
            Op::LogOr => self.int_binary("||")?,
            Op::IncOr => self.int_binary("|")?,
            Op::ExcOr => self.int_binary("^")?,
            Op::BoolAnd => self.int_binary("&")?,
            Op::ShiftLeft => self.int_binary("<<")?,
            Op::ShiftRight => self.int_binary(">>")?,
            Op::UShiftRight => self.int_binary(">>>")?,
            Op::Mod => self.int_binary("%")?,
            Op::Equal(pair) => self.binary("==", *pair)?,
            Op::NotEqual(pair) => self.binary("!=", *pair)?,
            Op::Geq(pair) => self.binary(">=", *pair)?,
            Op::Gt(pair) => self.binary(">", *pair)?,
            Op::Lt(pair) => self.binary("<", *pair)?,
            Op::Leq(pair) => self.binary("<=", *pair)?,
            Op::Add(pair) => self.binary("+", *pair)?,
            Op::Sub(pair) => self.binary("-", *pair)?,
            Op::Mul(pair) => self.binary("*", *pair)?,
            Op::Div(pair) => self.binary("/", *pair)?,
            Op::Neg(_) => self.unary("-")?,
            Op::Not => self.unary("!")?,
            Op::Comp => self.unary("~")?,
            Op::Destruct {
                size: whole,
                offset,
                keep,
            } => {
                let (base, ty) = self.pop_val(i32::from(*whole))?;
                let index = usize::from(offset.unsigned_abs() / 4);
                let (field, field_ty) = match &ty {
                    Ty::Vector if *keep == 4 && index < 3 => {
                        (["x", "y", "z"][index].to_string(), Ty::Float)
                    }
                    Ty::Struct(name) if *keep == 4 => {
                        let cell = self
                            .structs
                            .iter()
                            .find(|(own, _)| own == name)
                            .and_then(|(_, cells)| cells.get(index));
                        match cell {
                            Some(cell) => (field_name(index), cell.clone()),
                            None => return self.fail("a structure has no such field"),
                        }
                    }
                    _ => return self.fail("a field of more than one cell is read"),
                };
                self.stack
                    .push(Entry::Val(Expr::Field(Box::new(base), field), field_ty));
                self.pc += 1;
            }
            Op::IncSp(offset) | Op::DecSp(offset) => {
                let mark = if matches!(op, Op::IncSp(_)) {
                    "++"
                } else {
                    "--"
                };
                let (place, _) = self.local(self.top() + offset, 4)?;
                self.step(mark, place)?;
            }
            Op::IncBp(offset) | Op::DecBp(offset) => {
                let mark = if matches!(op, Op::IncBp(_)) {
                    "++"
                } else {
                    "--"
                };
                let (place, _) = self.global(self.globals_size + offset, 4)?;
                self.step(mark, place)?;
            }
            other => return self.fail(format!("{other:?} is not part of an expression")),
        }
        Ok(())
    }

    /// The statements from here to `end`, a block of their own.
    fn sub(&mut self, end: usize) -> Done<Vec<Stmt>> {
        let vars = self.vars_top;
        let parts = self.run(end, Kind::Plain)?;
        if self.pc != end || self.vars_top != vars || !self.stack.is_empty() {
            return self.fail("a block does not end where its jump says");
        }
        self.statements(parts)
    }

    fn statements(&self, parts: Vec<Part>) -> Done<Vec<Stmt>> {
        parts
            .into_iter()
            .map(|part| match part {
                Part::Stmt(statement) => Ok(statement),
                _ => self.fail("a switch's label is outside its switch"),
            })
            .collect()
    }

    fn run(&mut self, end: usize, kind: Kind) -> Done<Vec<Part>> {
        let saved = std::mem::take(&mut self.parts);
        // Where the block's own variables start. Bytes, not a count: cells
        // of an outer block may become one vector while this one is read.
        let base = self.vars_top;
        self.depth += 1;
        let result = self.run_inner(end, kind, base);
        self.depth -= 1;
        let parts = std::mem::replace(&mut self.parts, saved);
        result.map(|()| parts)
    }

    fn run_inner(&mut self, end: usize, kind: Kind, base: i32) -> Done<()> {
        while self.pc < end {
            // A loop's start is a statement's start: cells reserved before
            // it are variables declared.
            let count = self.prefix();
            if count > 0 && count == self.stack.len() && self.loop_at(end).is_some() {
                self.flush(count);
            }
            if self.stack.is_empty() {
                if kind == Kind::Switch {
                    self.labels(base);
                }
                let (depth, at, length) = (self.depth, self.pc, self.parts.len());
                if let Some(Exit::Loop {
                    next: Some(next),
                    body_depth,
                    mark,
                    ..
                }) = self.exits.last_mut()
                    && *body_depth == depth
                    && *next == at
                    && mark.is_none()
                {
                    *mark = Some(length);
                }
                if let Some(back) = self.loop_at(end) {
                    self.loop_stmt(back)?;
                    continue;
                }
            }
            let Some(op) = self.op(self.pc) else { break };
            match op {
                Op::Jz(_) => {
                    let count = self.prefix();
                    self.flush(count);
                    self.if_stmt(end)?;
                }
                Op::Jmp(_) => self.jump_stmt()?,
                Op::MovSp(by) => {
                    if self.pop_stmt(-*by, end, kind, base)? {
                        break;
                    }
                }
                Op::CpDownSp {
                    offset,
                    size: bytes,
                } => {
                    if !self.down_stmt(*offset, i32::from(*bytes))? {
                        self.eval()?;
                    }
                }
                Op::CpTopSp {
                    offset: -4,
                    size: 4,
                } if self.switch_head() => self.switch_stmt(end)?,
                _ => self.eval()?,
            }
        }
        if kind != Kind::Do {
            let count = self.prefix();
            if count == self.stack.len() {
                self.flush(count);
            }
        }
        Ok(())
    }

    /// The jump back of the loop that starts here, when one does: the
    /// outermost not yet being read.
    fn loop_at(&self, end: usize) -> Option<usize> {
        self.backs
            .get(&self.pc)?
            .iter()
            .copied()
            .filter(|back| *back < end && !self.open_backs.contains(back))
            .max()
    }

    fn loop_stmt(&mut self, back: usize) -> Done<()> {
        let head = self.pc;
        self.open_backs.push(back);
        // The test of a `while` or a `for` jumps past the jump back; a `do`
        // loop's test is the instruction before it.
        let test = (head..back).find(|at| {
            matches!(self.code[*at].op, Op::Jz(_)) && self.targets[*at] == Some(back + 1)
        });
        let exit = Exit::Loop {
            end: back + 1,
            back,
            next: None,
            body_depth: self.depth + 1,
            mark: None,
        };
        let statement = match test {
            Some(test) if test + 1 == back => {
                self.exits.push(exit);
                let vars = self.vars_top;
                let parts = self.run(back - 1, Kind::Do)?;
                self.exits.pop();
                if self.pc != back - 1 || self.stack.len() != 1 || self.vars_top != vars {
                    return self.fail("a do loop's test is not one value");
                }
                let (condition, _) = self.pop_val(4)?;
                Stmt::DoWhile(block(self.statements(parts)?), condition)
            }
            _ => {
                let condition = match test {
                    Some(test) => {
                        while self.pc < test {
                            self.eval()?;
                        }
                        if self.stack.len() != 1 {
                            return self.fail("a loop's test is not one value");
                        }
                        self.pc = test + 1;
                        Some(self.pop_val(4)?.0)
                    }
                    None => None,
                };
                self.exits.push(exit);
                let mut body = self.sub(back)?;
                let Some(Exit::Loop { next, mark, .. }) = self.exits.pop() else {
                    unreachable!("the loop pushed above")
                };
                // A `continue` that lands before the jump back lands on a
                // `for`'s step.
                let step = match (next, mark) {
                    (Some(next), Some(mark)) if next != back => {
                        let mut step = body.split_off(mark);
                        match (step.pop(), step.is_empty()) {
                            (Some(Stmt::Expr(step)), true) => Some(step),
                            _ => return self.fail("a for's step is not one expression"),
                        }
                    }
                    (Some(next), None) if next != back => {
                        return self.fail("a continue lands inside a statement");
                    }
                    _ => None,
                };
                match (condition, step) {
                    (Some(condition), None) => Stmt::While(condition, block(body)),
                    (condition, step) => Stmt::For(None, condition, step, block(body)),
                }
            }
        };
        self.open_backs.pop();
        self.pc = back + 1;
        self.emit(statement);
        Ok(())
    }

    /// An `if`: its test, a jump over its body when false, and its body
    /// ending with a jump past the `else`.
    fn if_stmt(&mut self, end: usize) -> Done<()> {
        if self.stack.len() != 1 {
            return self.fail("a test is not one value");
        }
        let other = self.target(self.pc)?;
        let past = match self.op(other.wrapping_sub(1)) {
            Some(Op::Jmp(_)) if other > self.pc + 1 => self.target(other - 1)?,
            _ => return self.fail("an if's body does not end with a jump"),
        };
        if past < other || past > end {
            return self.fail("an if's end is outside its block");
        }
        let (condition, _) = self.pop_val(4)?;
        self.pc += 1;
        let then = self.sub(other - 1)?;
        self.pc = other;
        let otherwise = if past > other {
            Some(block(self.sub(past)?))
        } else {
            None
        };
        self.pc = past;
        self.emit(Stmt::If(condition, block(then), otherwise));
        Ok(())
    }

    /// A jump that is a statement: `return`, `break` or `continue`.
    fn jump_stmt(&mut self) -> Done<()> {
        let count = self.prefix();
        if count != self.stack.len() {
            return self.fail("a jump inside an expression");
        }
        self.flush(count);
        let to = self.target(self.pc)?;
        let here = self.pc;
        let statement = if to == self.fn_end && self.vars_top == 0 {
            Stmt::Return(None)
        } else {
            let pops_switch = matches!(self.op(to), Some(Op::MovSp(-4)));
            match self.exits.last() {
                Some(Exit::Loop { end, .. }) if *end == to => Stmt::Break,
                Some(Exit::Switch { .. }) if pops_switch => Stmt::Break,
                Some(_) => {
                    let Some(Exit::Loop { back, next, .. }) = self
                        .exits
                        .iter_mut()
                        .rev()
                        .find(|exit| matches!(exit, Exit::Loop { .. }))
                    else {
                        return self.fail("a jump leaves no loop");
                    };
                    if to <= here || to > *back || next.is_some_and(|next| next != to) {
                        return self.fail("a jump is no break and no continue");
                    }
                    *next = Some(to);
                    Stmt::Continue
                }
                None => return self.fail("a jump leaves no loop"),
            }
        };
        self.pc += 1;
        self.emit(statement);
        Ok(())
    }

    /// A `MOVSP` that ends a statement or a block. True when it is the end
    /// of the switch being read.
    fn pop_stmt(&mut self, bytes: i32, end: usize, kind: Kind, base: i32) -> Done<bool> {
        let count = self.prefix();
        if self.stack.len() == count + 1
            && let Some(Entry::Val(_, ty)) = self.stack.last()
            && size(ty) == bytes
            && bytes > 0
        {
            self.flush(count);
            let (expr, _) = self.pop_val(bytes)?;
            self.emit(Stmt::Expr(expr));
            self.pc += 1;
            return Ok(false);
        }
        if self.stack.len() != count {
            return self.fail("a pop takes part of an expression");
        }
        self.flush(count);
        if self.pc + 1 < end && matches!(self.op(self.pc + 1), Some(Op::Jmp(_))) {
            let to = self.target(self.pc + 1)?;
            // `return;` pops the whole frame, a `break` the loop's locals:
            // the code after either goes on as if nothing was popped.
            if to == self.fn_end && bytes == self.vars_top {
                self.emit(Stmt::Return(None));
                self.pc += 2;
                return Ok(false);
            }
            if matches!(self.exits.last(), Some(Exit::Loop { end, .. }) if *end == to) {
                self.emit(Stmt::Break);
                self.pc += 2;
                return Ok(false);
            }
        }
        let closed = self.vars_top == base;
        if kind == Kind::Switch
            && closed
            && bytes == 4
            && self.vars.last().is_some_and(|var| var.switch)
        {
            return Ok(true);
        }
        // A block's end: its variables popped.
        let mut left = bytes;
        let mut first = usize::MAX;
        while left > 0 {
            match self.vars.last() {
                Some(var) if var.start >= base && !var.switch && size(&var.ty) <= left => {
                    left -= size(&var.ty);
                    first = first.min(var.part);
                    self.vars_top -= size(&var.ty);
                    self.vars.pop();
                }
                _ => return self.fail("a pop takes more than the block's variables"),
            }
        }
        let last = self.pc + 1 == end
            || (kind == Kind::Switch && matches!(self.op(self.pc + 1), Some(Op::MovSp(-4))));
        if !(last && self.vars_top == base) {
            // The variables were a block's inside this one.
            if first > self.parts.len() {
                return self.fail("a pop of no variable");
            }
            let inner = self.parts.split_off(first);
            let inner = self.statements(inner)?;
            self.emit(Stmt::Block(inner.into_iter().map(located).collect()));
        }
        self.pc += 1;
        Ok(false)
    }

    /// A `CPDOWNSP` that is a statement: a `return` of a value, or a
    /// variable declared with its value.
    fn down_stmt(&mut self, offset: i32, bytes: i32) -> Done<bool> {
        let top = self.top();
        let count = self.prefix();
        if !self.global_mode && self.ret_size == bytes && top + offset == self.ret_at {
            let follows = matches!(self.op(self.pc + 1), Some(Op::MovSp(by)) if -*by == top)
                && matches!(self.op(self.pc + 2), Some(Op::Jmp(_)))
                && self.targets.get(self.pc + 2) == Some(&Some(self.fn_end))
                && matches!(self.op(self.pc + 3), Some(Op::MovSp(by)) if -*by == bytes);
            if !follows {
                return self.fail("a result is set outside a return");
            }
            self.flush(count);
            let (value, _) = self.pop_val(bytes)?;
            if !self.stack.is_empty() {
                return self.fail("a return inside an expression");
            }
            self.emit(Stmt::Return(Some(value)));
            self.pc += 4;
            return Ok(true);
        }
        // `int a = value;`: the value copied onto the cell reserved just
        // below it, and popped.
        let cells = (bytes / 4) as usize;
        let values = match self.stack.last() {
            Some(Entry::Val(_, ty)) if size(ty) == bytes => 1,
            Some(Entry::Val(Expr::Float(_), _)) if bytes == 12 => 3,
            _ => return Ok(false),
        };
        let declares = self.stack.len() == count + values
            && count >= cells
            && offset == -2 * bytes
            && matches!(self.op(self.pc + 1), Some(Op::MovSp(by)) if -*by == bytes);
        if !declares {
            return Ok(false);
        }
        let reserved: Vec<Ty> = self.stack[count - cells..count]
            .iter()
            .filter_map(|entry| match entry {
                Entry::Reserved(ty) => Some(ty.clone()),
                _ => None,
            })
            .collect();
        let ty = match reserved.as_slice() {
            [one] => one.clone(),
            many => aggregate(self.structs, many),
        };
        let (value, _) = self.pop_val(bytes)?;
        self.stack.truncate(count - cells);
        self.flush(count - cells);
        self.declare(ty, Some(value));
        self.pc += 2;
        Ok(true)
    }

    /// Whether the copy here starts a switch's first case: the statement's
    /// one value copied, and not tested by a jump (a logic operator's is).
    fn switch_head(&self) -> bool {
        let count = self.prefix();
        self.actions == 0
            && self.stack.len() == count + 1
            && matches!(self.stack.last(), Some(Entry::Val(_, Ty::Int | Ty::String)))
            && !matches!(self.op(self.pc + 1), Some(Op::Jz(_)))
    }

    /// A switch: its value kept on the stack, a copy of it tested against
    /// each case, a jump to the default or the end, and its body.
    fn switch_stmt(&mut self, end: usize) -> Done<()> {
        let count = self.prefix();
        self.flush(count);
        let (value, ty) = self.pop_val(4)?;
        self.vars.push(Var {
            name: String::new(),
            ty: ty.clone(),
            start: self.vars_top,
            switch: true,
            part: self.parts.len(),
        });
        self.vars_top += 4;
        let mut labels = Vec::new();
        while matches!(
            self.op(self.pc),
            Some(Op::CpTopSp {
                offset: -4,
                size: 4
            })
        ) {
            self.pc += 1;
            self.stack
                .push(Entry::Val(Expr::Name(String::new()), ty.clone()));
            while !matches!(self.op(self.pc), Some(Op::Equal(_))) {
                if self.pc >= end {
                    return self.fail("a case is not compared");
                }
                self.eval()?;
            }
            self.eval()?;
            let case = match self.stack.pop() {
                Some(Entry::Val(Expr::Binary("==", _, case), _)) if self.stack.is_empty() => *case,
                _ => return self.fail("a case is not one value"),
            };
            if !matches!(self.op(self.pc), Some(Op::Jnz(_))) {
                return self.fail("a case does not jump");
            }
            labels.push((self.target(self.pc)?, case, false));
            self.pc += 1;
        }
        if !matches!(self.op(self.pc), Some(Op::Jmp(_))) {
            return self.fail("a switch's cases do not end with a jump");
        }
        let default = self.target(self.pc)?;
        self.pc += 1;
        self.exits.push(Exit::Switch {
            labels,
            default,
            default_done: false,
        });
        let parts = self.run(end, Kind::Switch)?;
        self.exits.pop();
        let ends = matches!(self.op(self.pc), Some(Op::MovSp(-4)))
            && self.vars.last().is_some_and(|var| var.switch)
            && self.stack.is_empty();
        if !ends {
            return self.fail("a switch does not end with its value popped");
        }
        self.vars.pop();
        self.vars_top -= 4;
        self.pc += 1;
        let parts = parts
            .into_iter()
            .map(|part| Located {
                line: 0,
                item: match part {
                    Part::Stmt(statement) => SwitchPart::Stmt(statement),
                    Part::Case(case) => SwitchPart::Case(case),
                    Part::Default => SwitchPart::Default,
                },
            })
            .collect();
        self.emit(Stmt::Switch(value, parts));
        Ok(())
    }

    /// The labels of the switch being read that stand here.
    fn labels(&mut self, base: i32) {
        let at = self.pc;
        let at_end = matches!(self.op(at), Some(Op::MovSp(-4))) && self.vars_top == base;
        let Some(Exit::Switch {
            labels,
            default,
            default_done,
        }) = self.exits.last_mut()
        else {
            return;
        };
        let mut here = Vec::new();
        // A default written among the cases keeps its place: the cases
        // are in the order tested, and the default is where its jump lands.
        for (label, case, done) in labels.iter_mut() {
            if *label == at && !*done {
                *done = true;
                here.push(Part::Case(case.clone()));
            }
        }
        if *default == at && !*default_done && !at_end {
            *default_done = true;
            here.push(Part::Default);
        }
        self.parts.extend(here);
    }

    /// The globals' function up to its `SAVEBP`: each global, declared in
    /// order.
    pub(super) fn globals(
        &mut self,
        start: usize,
        save: usize,
    ) -> Done<(Vec<Var>, Vec<Option<Expr>>)> {
        self.global_mode = true;
        self.pc = start;
        let parts = self.run(save, Kind::Plain)?;
        if self.pc != save || !self.stack.is_empty() {
            return self.fail("the globals do not end at SAVEBP");
        }
        let mut values = Vec::new();
        for statement in self.statements(parts)? {
            match statement {
                Stmt::Declare(_, names) => values.extend(names.into_iter().map(|(_, value)| value)),
                _ => return self.fail("the globals' function does more than declare them"),
            }
        }
        if values.len() != self.vars.len() {
            return self.fail("the globals are not declared one by one");
        }
        Ok((std::mem::take(&mut self.vars), values))
    }

    /// The function `index` of the program.
    pub(super) fn function(&mut self, index: usize) -> Done<Function> {
        let sig = self.sigs[index].clone();
        self.pc = sig.start;
        let Some(param_types) = sig.params.clone() else {
            return self.fail(format!("{} is written before any call of it", sig.name));
        };
        let params_size: i32 = param_types.iter().map(size).sum();
        if sig.end < sig.start + 1 || !matches!(self.op(sig.end - 1), Some(Op::Retn)) {
            return self.fail("a function does not end with RETN");
        }
        let body_end = if params_size > 0 {
            match self.op(sig.end.wrapping_sub(2)) {
                Some(Op::MovSp(by)) if -*by == params_size && sig.end >= sig.start + 2 => {
                    sig.end - 2
                }
                _ => return self.fail("a function does not pop its parameters"),
            }
        } else {
            sig.end - 1
        };
        // The parameters lie below the frame, the first nearest it.
        let mut at = 0;
        let mut params = Vec::new();
        for ty in &param_types {
            at -= size(ty);
            let name = self.fresh(ty, "Param");
            self.params.push(Var {
                name: name.clone(),
                ty: ty.clone(),
                start: at,
                switch: false,
                part: 0,
            });
            params.push(Param {
                ty: ty.clone(),
                name,
                default: None,
            });
        }
        self.ret_size = sig.ret_size;
        self.ret_at = -params_size - sig.ret_size;
        self.fn_end = body_end;
        for at in sig.start..body_end {
            if let (Op::Jmp(by), Some(Some(to))) = (&self.code[at].op, self.targets.get(at))
                && *by < 0
            {
                self.backs.entry(*to).or_default().push(at);
            }
        }
        let parts = self.run(body_end, Kind::Plain)?;
        if self.pc != body_end || !self.stack.is_empty() || !self.vars.is_empty() {
            return self.fail("a function's body does not end at its end");
        }
        let mut body = self.statements(parts)?;
        let merges = std::mem::take(&mut self.merges);
        for merge in &merges {
            body = merged_statements(body, merge);
        }
        let ret = match sig.ret {
            Some(ret) => ret,
            None if sig.ret_size == 0 => Ty::Void,
            None => return self.fail("a function's result has no known type"),
        };
        Ok(Function {
            ret,
            name: sig.name,
            params,
            body: Some(body.into_iter().map(located).collect()),
        })
    }
}

/// The cells of a vector or a structure, by their place in it.
fn cell_field(ty: &Ty, index: usize) -> String {
    match ty {
        Ty::Vector => ["x", "y", "z"][index.min(2)].to_string(),
        _ => field_name(index),
    }
}

type Merge = (Vec<String>, String, Ty);

/// `expr` with each cell of `merge` named as a field of the whole.
fn merged_expr(expr: Expr, merge: &Merge) -> Expr {
    let again = |expr: Box<Expr>| Box::new(merged_expr(*expr, merge));
    match expr {
        Expr::Name(name) => match merge.0.iter().position(|cell| *cell == name) {
            Some(index) => Expr::Field(
                Box::new(Expr::Name(merge.1.clone())),
                cell_field(&merge.2, index),
            ),
            None => Expr::Name(name),
        },
        Expr::Call(name, args) => Expr::Call(
            name,
            args.into_iter()
                .map(|arg| merged_expr(arg, merge))
                .collect(),
        ),
        Expr::Field(base, field) => Expr::Field(again(base), field),
        Expr::Unary(mark, operand) => Expr::Unary(mark, again(operand)),
        Expr::Binary(mark, left, right) => Expr::Binary(mark, again(left), again(right)),
        Expr::Assign(mark, target, value) => Expr::Assign(mark, again(target), again(value)),
        Expr::Step(mark, before, target) => Expr::Step(mark, before, again(target)),
        Expr::Choose(condition, then, otherwise) => {
            Expr::Choose(again(condition), again(then), again(otherwise))
        }
        other => other,
    }
}

fn merged_located(statement: Located<Stmt>, merge: &Merge) -> Located<Stmt> {
    let mut out = merged_statements(vec![statement.item], merge);
    let item = if out.len() == 1 {
        out.remove(0)
    } else {
        Stmt::Block(out.into_iter().map(located).collect())
    };
    located(item)
}

/// `statements` with the cells of `merge` declared as the one whole they
/// are: the first cell's declaration becomes the whole's, the others' go,
/// and a cell's value at its declaration is put in its field.
fn merged_statements(statements: Vec<Stmt>, merge: &Merge) -> Vec<Stmt> {
    let expr = |expr: Expr| merged_expr(expr, merge);
    let boxed = |statement: Box<Located<Stmt>>| Box::new(merged_located(*statement, merge));
    let list = |statements: Vec<Located<Stmt>>| -> Vec<Located<Stmt>> {
        merged_statements(statements.into_iter().map(|s| s.item).collect(), merge)
            .into_iter()
            .map(located)
            .collect()
    };
    let mut out = Vec::new();
    for statement in statements {
        match statement {
            Stmt::Declare(ty, names) => {
                for (name, value) in names {
                    let Some(index) = merge.0.iter().position(|cell| *cell == name) else {
                        out.push(Stmt::Declare(ty.clone(), vec![(name, value.map(expr))]));
                        continue;
                    };
                    if index == 0 {
                        out.push(Stmt::Declare(
                            merge.2.clone(),
                            vec![(merge.1.clone(), None)],
                        ));
                    }
                    if let Some(value) = value {
                        let field = Expr::Field(
                            Box::new(Expr::Name(merge.1.clone())),
                            cell_field(&merge.2, index),
                        );
                        out.push(Stmt::Expr(Expr::Assign(
                            "=",
                            Box::new(field),
                            Box::new(expr(value)),
                        )));
                    }
                }
            }
            Stmt::Block(inner) => out.push(Stmt::Block(list(inner))),
            Stmt::Expr(value) => out.push(Stmt::Expr(expr(value))),
            Stmt::If(condition, then, otherwise) => {
                out.push(Stmt::If(expr(condition), boxed(then), otherwise.map(boxed)));
            }
            Stmt::While(condition, body) => out.push(Stmt::While(expr(condition), boxed(body))),
            Stmt::DoWhile(body, condition) => out.push(Stmt::DoWhile(boxed(body), expr(condition))),
            Stmt::For(init, condition, step, body) => out.push(Stmt::For(
                init.map(expr),
                condition.map(expr),
                step.map(expr),
                boxed(body),
            )),
            Stmt::Switch(value, parts) => {
                let mut parts_out = Vec::new();
                for part in parts {
                    match part.item {
                        SwitchPart::Case(case) => parts_out.push(SwitchPart::Case(expr(case))),
                        SwitchPart::Default => parts_out.push(SwitchPart::Default),
                        SwitchPart::Stmt(inner) => parts_out.extend(
                            merged_statements(vec![inner], merge)
                                .into_iter()
                                .map(SwitchPart::Stmt),
                        ),
                    }
                }
                out.push(Stmt::Switch(
                    expr(value),
                    parts_out
                        .into_iter()
                        .map(|item| Located { line: 0, item })
                        .collect(),
                ));
            }
            Stmt::Return(value) => out.push(Stmt::Return(value.map(expr))),
            other => out.push(other),
        }
    }
    out
}
