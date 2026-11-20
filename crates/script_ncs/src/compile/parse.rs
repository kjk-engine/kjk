//! NWScript read into a tree: a file's structures, globals and functions,
//! their statements and expressions, by recursive descent, with C's order
//! of operations.

use super::Error;
use super::lexer::{Tok, Token};

/// A value's type, as a script names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Void,
    Int,
    Float,
    String,
    Object,
    Vector,
    Effect,
    Event,
    Location,
    Talent,
    Action,
    Struct(String),
}

impl Ty {
    fn named(word: &str) -> Option<Ty> {
        Some(match word {
            "void" => Ty::Void,
            "int" => Ty::Int,
            "float" => Ty::Float,
            "string" => Ty::String,
            "object" => Ty::Object,
            "vector" => Ty::Vector,
            "effect" => Ty::Effect,
            "event" => Ty::Event,
            "location" => Ty::Location,
            "talent" => Ty::Talent,
            "action" => Ty::Action,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i32),
    Float(f32),
    Str(Vec<u8>),
    /// `[x, y, z]`, a vector's literal.
    Vector([f32; 3]),
    Name(String),
    Call(String, Vec<Expr>),
    /// A field of a structure (`.x` of a vector).
    Field(Box<Expr>, String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// `=` or a compound assignment (`+=`), to a place.
    Assign(&'static str, Box<Expr>, Box<Expr>),
    /// `++` or `--`, before (`true`) or after its place.
    Step(&'static str, bool, Box<Expr>),
    /// `c ? a : b`.
    Choose(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Located<T> {
    pub line: u32,
    pub item: T,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Block(Vec<Located<Stmt>>),
    /// Variables of one type, each with its value if it has one.
    Declare(Ty, Vec<(String, Option<Expr>)>),
    Expr(Expr),
    If(Expr, Box<Located<Stmt>>, Option<Box<Located<Stmt>>>),
    While(Expr, Box<Located<Stmt>>),
    DoWhile(Box<Located<Stmt>>, Expr),
    For(Option<Expr>, Option<Expr>, Option<Expr>, Box<Located<Stmt>>),
    /// The switch's value, and its body: cases, defaults and statements in
    /// order.
    Switch(Expr, Vec<Located<SwitchPart>>),
    Break,
    Continue,
    Return(Option<Expr>),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SwitchPart {
    Case(Expr),
    Default,
    Stmt(Stmt),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: Ty,
    pub name: String,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub ret: Ty,
    pub name: String,
    pub params: Vec<Param>,
    /// `None` for a prototype.
    pub body: Option<Vec<Located<Stmt>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Include(String),
    Define(String, String),
    Struct(String, Vec<(Ty, String)>),
    /// Globals of one type; `const` ones are values, not cells.
    Global {
        constant: bool,
        ty: Ty,
        names: Vec<(String, Option<Expr>)>,
    },
    Function(Function),
}

struct Parser<'t> {
    tokens: &'t [Token],
    at: usize,
    file: &'t str,
}

type Parsed<T> = Result<T, Error>;

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.tokens[self.at.min(self.tokens.len() - 1)].tok
    }

    fn peek_at(&self, ahead: usize) -> &Tok {
        &self.tokens[(self.at + ahead).min(self.tokens.len() - 1)].tok
    }

    fn line(&self) -> u32 {
        self.tokens[self.at.min(self.tokens.len() - 1)].line
    }

    fn error<T>(&self, what: impl Into<String>) -> Parsed<T> {
        Err(Error {
            file: self.file.to_string(),
            line: self.line(),
            what: what.into(),
        })
    }

    fn next(&mut self) -> Tok {
        let tok = self.peek().clone();
        self.at += 1;
        tok
    }

    fn is(&self, mark: &str) -> bool {
        matches!(self.peek(), Tok::Mark(m) if *m == mark)
    }

    fn eat(&mut self, mark: &str) -> bool {
        let is = self.is(mark);
        if is {
            self.at += 1;
        }
        is
    }

    fn expect(&mut self, mark: &str) -> Parsed<()> {
        if self.eat(mark) {
            Ok(())
        } else {
            self.error(format!("`{mark}` is missing before {}", self.peek()))
        }
    }

    fn name(&mut self) -> Parsed<String> {
        match self.next() {
            Tok::Name(name) => Ok(name),
            other => {
                self.at -= 1;
                self.error(format!("a name is missing before {other}"))
            }
        }
    }

    fn word_is(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Name(name) if name == word)
    }

    /// A type at the reader, if there is one: a basic type, or `struct NAME`.
    fn ty(&mut self) -> Option<Ty> {
        let Tok::Name(word) = self.peek().clone() else {
            return None;
        };
        if word == "struct" {
            if let Tok::Name(name) = self.peek_at(1).clone() {
                self.at += 2;
                return Some(Ty::Struct(name));
            }
            return None;
        }
        let ty = Ty::named(&word)?;
        self.at += 1;
        Some(ty)
    }

    fn items(&mut self) -> Parsed<Vec<Item>> {
        let mut items = Vec::new();
        loop {
            match self.peek().clone() {
                Tok::End => return Ok(items),
                Tok::Include(name) => {
                    self.at += 1;
                    items.push(Item::Include(name));
                }
                Tok::Define(name, value) => {
                    self.at += 1;
                    items.push(Item::Define(name, value));
                }
                _ => items.push(self.item()?),
            }
        }
    }

    fn item(&mut self) -> Parsed<Item> {
        // A structure's definition: `struct NAME { fields };`.
        if self.word_is("struct")
            && matches!(self.peek_at(1), Tok::Name(_))
            && matches!(self.peek_at(2), Tok::Mark("{"))
        {
            self.at += 1;
            let name = self.name()?;
            self.expect("{")?;
            let mut fields = Vec::new();
            while !self.eat("}") {
                let Some(ty) = self.ty() else {
                    return self.error("a structure's field has no type");
                };
                loop {
                    fields.push((ty.clone(), self.name()?));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(";")?;
            }
            self.expect(";")?;
            return Ok(Item::Struct(name, fields));
        }
        let constant = self.word_is("const");
        if constant {
            self.at += 1;
        }
        let Some(ty) = self.ty() else {
            return self.error(format!("{} does not start a declaration", self.peek()));
        };
        let name = self.name()?;
        if self.eat("(") {
            let mut params = Vec::new();
            if !self.eat(")") {
                loop {
                    if self.word_is("const") {
                        self.at += 1;
                    }
                    let Some(ty) = self.ty() else {
                        return self.error("a parameter has no type");
                    };
                    let name = self.name()?;
                    let default = if self.eat("=") {
                        Some(self.assignment()?)
                    } else {
                        None
                    };
                    params.push(Param { ty, name, default });
                    if self.eat(")") {
                        break;
                    }
                    self.expect(",")?;
                }
            }
            let body = if self.eat(";") {
                None
            } else {
                Some(self.block()?)
            };
            return Ok(Item::Function(Function {
                ret: ty,
                name,
                params,
                body,
            }));
        }
        let names = self.declarators(name)?;
        Ok(Item::Global {
            constant,
            ty,
            names,
        })
    }

    /// `name [= value], name [= value];` after the first name.
    fn declarators(&mut self, first: String) -> Parsed<Vec<(String, Option<Expr>)>> {
        let mut names = Vec::new();
        let mut name = first;
        loop {
            let value = if self.eat("=") {
                Some(self.assignment()?)
            } else {
                None
            };
            names.push((name, value));
            if self.eat(";") {
                return Ok(names);
            }
            // A declaration goes on after a comma, else it ends with `;`.
            if !self.eat(",") {
                self.expect(";")?;
            }
            name = self.name()?;
        }
    }

    fn block(&mut self) -> Parsed<Vec<Located<Stmt>>> {
        self.expect("{")?;
        let mut statements = Vec::new();
        while !self.eat("}") {
            if matches!(self.peek(), Tok::End) {
                return self.error("a block does not end");
            }
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn statement(&mut self) -> Parsed<Located<Stmt>> {
        let line = self.line();
        let item = self.statement_item()?;
        Ok(Located { line, item })
    }

    fn statement_item(&mut self) -> Parsed<Stmt> {
        if self.is("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        let word = match self.peek() {
            Tok::Name(word) => word.clone(),
            _ => String::new(),
        };
        match word.as_str() {
            "if" => {
                self.at += 1;
                self.expect("(")?;
                let condition = self.expression()?;
                self.expect(")")?;
                let then = Box::new(self.statement()?);
                let otherwise = if self.word_is("else") {
                    self.at += 1;
                    Some(Box::new(self.statement()?))
                } else {
                    None
                };
                return Ok(Stmt::If(condition, then, otherwise));
            }
            "while" => {
                self.at += 1;
                self.expect("(")?;
                let condition = self.expression()?;
                self.expect(")")?;
                return Ok(Stmt::While(condition, Box::new(self.statement()?)));
            }
            "do" => {
                self.at += 1;
                let body = Box::new(self.statement()?);
                if !self.word_is("while") {
                    return self.error("a do loop has no while");
                }
                self.at += 1;
                self.expect("(")?;
                let condition = self.expression()?;
                self.expect(")")?;
                self.expect(";")?;
                return Ok(Stmt::DoWhile(body, condition));
            }
            "for" => {
                self.at += 1;
                self.expect("(")?;
                let part = |parser: &mut Parser, end: &str| -> Parsed<Option<Expr>> {
                    if parser.eat(end) {
                        return Ok(None);
                    }
                    let expr = parser.expression()?;
                    parser.expect(end)?;
                    Ok(Some(expr))
                };
                let init = part(self, ";")?;
                let condition = part(self, ";")?;
                let step = part(self, ")")?;
                return Ok(Stmt::For(
                    init,
                    condition,
                    step,
                    Box::new(self.statement()?),
                ));
            }
            "switch" => {
                self.at += 1;
                self.expect("(")?;
                let value = self.expression()?;
                self.expect(")")?;
                self.expect("{")?;
                let mut parts = Vec::new();
                while !self.eat("}") {
                    let line = self.line();
                    let part = if self.word_is("case") {
                        self.at += 1;
                        let value = self.expression()?;
                        self.expect(":")?;
                        SwitchPart::Case(value)
                    } else if self.word_is("default") {
                        self.at += 1;
                        self.expect(":")?;
                        SwitchPart::Default
                    } else {
                        SwitchPart::Stmt(self.statement()?.item)
                    };
                    parts.push(Located { line, item: part });
                }
                return Ok(Stmt::Switch(value, parts));
            }
            "break" => {
                self.at += 1;
                self.expect(";")?;
                return Ok(Stmt::Break);
            }
            "continue" => {
                self.at += 1;
                self.expect(";")?;
                return Ok(Stmt::Continue);
            }
            "return" => {
                self.at += 1;
                if self.eat(";") {
                    return Ok(Stmt::Return(None));
                }
                let value = self.expression()?;
                self.expect(";")?;
                return Ok(Stmt::Return(Some(value)));
            }
            _ => {}
        }
        if word == "const" {
            self.at += 1;
        }
        let start = self.at;
        if let Some(ty) = self.ty() {
            if let Tok::Name(_) = self.peek() {
                let name = self.name()?;
                return Ok(Stmt::Declare(ty, self.declarators(name)?));
            }
            self.at = start;
        }
        let expr = self.expression()?;
        self.expect(";")?;
        Ok(Stmt::Expr(expr))
    }

    fn expression(&mut self) -> Parsed<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> Parsed<Expr> {
        let target = self.choose()?;
        for mark in [
            "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ">>>=",
        ] {
            if self.eat(mark) {
                let value = self.assignment()?;
                return Ok(Expr::Assign(mark, Box::new(target), Box::new(value)));
            }
        }
        Ok(target)
    }

    fn choose(&mut self) -> Parsed<Expr> {
        let condition = self.binary(0)?;
        if self.eat("?") {
            let then = self.assignment()?;
            self.expect(":")?;
            let otherwise = self.assignment()?;
            return Ok(Expr::Choose(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        Ok(condition)
    }

    /// The binary operators by how tightly they bind, loosest first.
    const LEVELS: [&'static [&'static str]; 10] = [
        &["||"],
        &["&&"],
        &["|"],
        &["^"],
        &["&"],
        &["==", "!="],
        &["<", ">", "<=", ">="],
        &["<<", ">>", ">>>"],
        &["+", "-"],
        &["*", "/", "%"],
    ];

    fn binary(&mut self, level: usize) -> Parsed<Expr> {
        if level == Self::LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'more: loop {
            for mark in Self::LEVELS[level] {
                if self.is(mark) {
                    self.at += 1;
                    let right = self.binary(level + 1)?;
                    left = Expr::Binary(mark, Box::new(left), Box::new(right));
                    continue 'more;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Parsed<Expr> {
        for mark in ["!", "~", "-", "+"] {
            if self.eat(mark) {
                let operand = self.unary()?;
                // BioWare's compiler folds no sign into a number: `-6` is
                // 6 pushed and negated.
                return Ok(match (mark, operand) {
                    ("+", operand) => operand,
                    (mark, operand) => Expr::Unary(mark, Box::new(operand)),
                });
            }
        }
        for mark in ["++", "--"] {
            if self.eat(mark) {
                let operand = self.unary()?;
                return Ok(Expr::Step(mark, true, Box::new(operand)));
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Parsed<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(".") {
                let field = self.name()?;
                expr = Expr::Field(Box::new(expr), field);
            } else if self.is("++") || self.is("--") {
                let mark = if self.eat("++") {
                    "++"
                } else {
                    self.at += 1;
                    "--"
                };
                expr = Expr::Step(mark, false, Box::new(expr));
            } else {
                return Ok(expr);
            }
        }
    }

    fn primary(&mut self) -> Parsed<Expr> {
        match self.next() {
            Tok::Int(value) => Ok(Expr::Int(value)),
            Tok::Float(value) => Ok(Expr::Float(value)),
            Tok::Str(value) => Ok(Expr::Str(value)),
            Tok::Mark("(") => {
                let expr = self.expression()?;
                self.expect(")")?;
                Ok(expr)
            }
            Tok::Mark("[") => {
                let mut values = [0.0f32; 3];
                for (at, value) in values.iter_mut().enumerate() {
                    if at > 0 && !self.eat(",") {
                        break;
                    }
                    let negative = self.eat("-");
                    *value = match self.next() {
                        Tok::Float(v) => v,
                        Tok::Int(v) => v as f32,
                        other => return self.error(format!("{other} cannot stand in a vector")),
                    };
                    if negative {
                        *value = -*value;
                    }
                }
                self.expect("]")?;
                Ok(Expr::Vector(values))
            }
            Tok::Name(name) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.expression()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Name(name))
            }
            other => {
                self.at -= 1;
                self.error(format!("{other} does not start a value"))
            }
        }
    }
}

/// The items of a file's words.
pub fn parse(tokens: &[Token], file: &str) -> Result<Vec<Item>, Error> {
    Parser {
        tokens,
        at: 0,
        file,
    }
    .items()
}
