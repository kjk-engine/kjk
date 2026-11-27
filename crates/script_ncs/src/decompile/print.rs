//! A program's items written as NWScript source the compiler reads back to
//! the same tree.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::compile::parse::{Expr, Function, Item, Located, Stmt, SwitchPart, Ty};

/// A type's name in source.
pub(super) fn ty_name(ty: &Ty) -> String {
    match ty {
        Ty::Void => "void".into(),
        Ty::Int => "int".into(),
        Ty::Float => "float".into(),
        Ty::String => "string".into(),
        Ty::Object => "object".into(),
        Ty::Vector => "vector".into(),
        Ty::Effect => "effect".into(),
        Ty::Event => "event".into(),
        Ty::Location => "location".into(),
        Ty::Talent => "talent".into(),
        Ty::Action => "action".into(),
        Ty::Struct(name) => format!("struct {name}"),
    }
}

/// The shortest decimal the lexer reads back as `value` (no sign, no
/// exponent: the lexer takes neither).
fn float_digits(value: f32) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let value = value.abs();
    (1..=60).find_map(|digits| {
        let text = format!("{value:.digits$}");
        (text.parse::<f32>().ok()?.to_bits() == value.to_bits()).then_some(text)
    })
}

/// How tightly a binary operator binds (the parser's levels), loosest 0.
fn level(mark: &str) -> usize {
    match mark {
        "||" => 0,
        "&&" => 1,
        "|" => 2,
        "^" => 3,
        "&" => 4,
        "==" | "!=" => 5,
        "<" | ">" | "<=" | ">=" => 6,
        "<<" | ">>" | ">>>" => 7,
        "+" | "-" => 8,
        _ => 9,
    }
}

/// An operand of a unary operator or a call's place.
const TIGHT: usize = 10;

/// The source being written. Negative numbers have no literal in the
/// language (`-6` is 6 negated), so a negative constant is a `#define`,
/// which the compiler takes as the number itself.
#[derive(Default)]
pub(super) struct Printer {
    defines: BTreeMap<String, String>,
    body: String,
    pub(super) error: Option<String>,
}

impl Printer {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(what);
        }
    }

    fn int(&mut self, value: i32) -> String {
        if value >= 0 {
            return value.to_string();
        }
        if value == i32::MIN {
            return "0x80000000".into();
        }
        let name = format!("INT_MINUS_{}", -i64::from(value));
        self.defines.insert(name.clone(), value.to_string());
        name
    }

    fn float(&mut self, value: f32) -> String {
        let Some(digits) = float_digits(value) else {
            self.fail(format!("the float {value} has no literal"));
            return "0.0".into();
        };
        if !value.is_sign_negative() {
            return digits;
        }
        let name = format!("FLOAT_MINUS_{}", digits.replace('.', "_"));
        self.defines.insert(name.clone(), format!("-{digits}"));
        name
    }

    fn string(text: &[u8]) -> String {
        let mut out = String::from("\"");
        for &byte in text {
            match byte {
                b'"' => out.push_str("\\\""),
                b'\\' => out.push_str("\\\\"),
                b'\n' => out.push_str("\\n"),
                b'\t' => out.push_str("\\t"),
                b'\r' => out.push_str("\\r"),
                // Any other byte that prints nothing, as `\x` and its two
                // hex digits. Written raw, a carriage return lasted only
                // until an editor set the file's line ends, and the string
                // was then another. A byte above 127 is a letter of the
                // games' code page and stays as it is.
                other if other < 0x20 || other == 0x7f => {
                    out.push_str(&format!("\\x{other:02x}"));
                }
                other => out.push(char::from(other)),
            }
        }
        out.push('"');
        out
    }

    /// `expr`, in brackets where an operator binding at `min` or tighter
    /// would otherwise take it apart.
    pub(super) fn expr(&mut self, expr: &Expr, min: usize) -> String {
        let (text, own) = match expr {
            Expr::Int(value) => (self.int(*value), TIGHT),
            Expr::Float(value) => (self.float(*value), TIGHT),
            Expr::Str(text) => (Self::string(text), TIGHT),
            Expr::Vector(parts) => {
                // A negative part is written with its sign inside the
                // brackets, where the compiler reads it into the number
                // (`Parser::primary`). Anywhere else a negative number has
                // to be a `#define` (`float`), which a vector cannot hold.
                let parts: Vec<String> = (parts.iter())
                    .map(|part| match float_digits(*part) {
                        Some(digits) if part.is_sign_negative() => format!("-{digits}"),
                        Some(digits) => digits,
                        None => {
                            self.fail(format!("the float {part} has no literal"));
                            "0.0".into()
                        }
                    })
                    .collect();
                (format!("[{}]", parts.join(", ")), TIGHT)
            }
            Expr::Name(name) => (name.clone(), TIGHT),
            Expr::Call(name, args) => {
                let args: Vec<String> = args.iter().map(|arg| self.expr(arg, 0)).collect();
                (format!("{name}({})", args.join(", ")), TIGHT)
            }
            Expr::Field(base, field) => (format!("{}.{field}", self.expr(base, TIGHT)), TIGHT),
            Expr::Unary(mark, operand) => {
                // `- -x` must not read as `--x`.
                let inner = match **operand {
                    Expr::Unary(..) | Expr::Step(..) => format!("({})", self.expr(operand, 0)),
                    _ => self.expr(operand, TIGHT),
                };
                (format!("{mark}{inner}"), TIGHT)
            }
            Expr::Binary(mark, left, right) => {
                let own = level(mark);
                let left = self.expr(left, own);
                let right = self.expr(right, own + 1);
                (format!("{left} {mark} {right}"), own)
            }
            Expr::Assign(mark, target, value) => {
                let target = self.expr(target, TIGHT);
                let value = self.expr(value, 0);
                // Looser than every operator: always in brackets inside one.
                let text = format!("{target} {mark} {value}");
                return if min > 0 { format!("({text})") } else { text };
            }
            Expr::Step(mark, before, target) => {
                let target = self.expr(target, TIGHT);
                let text = if *before {
                    format!("{mark}{target}")
                } else {
                    format!("{target}{mark}")
                };
                (format!("({text})"), TIGHT)
            }
            Expr::Choose(condition, then, otherwise) => {
                let text = format!(
                    "{} ? {} : {}",
                    self.expr(condition, 1),
                    self.expr(then, 1),
                    self.expr(otherwise, 1)
                );
                return format!("({text})");
            }
        };
        if own < min { format!("({text})") } else { text }
    }

    /// A statement's expression: a lone step needs no brackets.
    fn top_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Step(mark, before, target) => {
                let target = self.expr(target, TIGHT);
                if *before {
                    format!("{mark}{target}")
                } else {
                    format!("{target}{mark}")
                }
            }
            other => self.expr(other, 0),
        }
    }

    fn line(&mut self, depth: usize, text: &str) {
        for _ in 0..depth {
            self.body.push_str("    ");
        }
        self.body.push_str(text);
        self.body.push('\n');
    }

    /// A statement as a body in braces: a block's own statements, or the
    /// statement alone.
    fn braced(&mut self, statement: &Stmt, depth: usize) {
        match statement {
            Stmt::Block(statements) => self.statements(statements, depth + 1),
            other => self.stmt(other, depth + 1),
        }
    }

    fn statements(&mut self, statements: &[Located<Stmt>], depth: usize) {
        for statement in statements {
            self.stmt(&statement.item, depth);
        }
    }

    fn stmt(&mut self, statement: &Stmt, depth: usize) {
        match statement {
            Stmt::Empty => self.line(depth, ";"),
            Stmt::Block(statements) => {
                self.line(depth, "{");
                self.statements(statements, depth + 1);
                self.line(depth, "}");
            }
            Stmt::Declare(ty, names) => {
                for (name, value) in names {
                    let text = match value {
                        Some(value) => {
                            format!("{} {name} = {};", ty_name(ty), self.expr(value, 0))
                        }
                        None => format!("{} {name};", ty_name(ty)),
                    };
                    self.line(depth, &text);
                }
            }
            Stmt::Expr(expr) => {
                let text = format!("{};", self.top_expr(expr));
                self.line(depth, &text);
            }
            Stmt::If(condition, then, otherwise) => {
                let mut head = format!("if ({}) {{", self.expr(condition, 0));
                let mut then = then;
                let mut otherwise = otherwise;
                loop {
                    self.line(depth, &head);
                    self.braced(&then.item, depth);
                    let Some(other) = otherwise else { break };
                    // An `else` holding one `if` and nothing else chains.
                    let chained = match &other.item {
                        Stmt::Block(inner) if inner.len() == 1 => match &inner[0].item {
                            Stmt::If(condition, then, otherwise) => {
                                Some((condition, then, otherwise))
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    match chained {
                        Some((condition, next_then, next_otherwise)) => {
                            head = format!("}} else if ({}) {{", self.expr(condition, 0));
                            then = next_then;
                            otherwise = next_otherwise;
                        }
                        None => {
                            self.line(depth, "} else {");
                            self.braced(&other.item, depth);
                            break;
                        }
                    }
                }
                self.line(depth, "}");
            }
            Stmt::While(condition, body) => {
                let head = format!("while ({}) {{", self.expr(condition, 0));
                self.line(depth, &head);
                self.braced(&body.item, depth);
                self.line(depth, "}");
            }
            Stmt::DoWhile(body, condition) => {
                self.line(depth, "do {");
                self.braced(&body.item, depth);
                let tail = format!("}} while ({});", self.expr(condition, 0));
                self.line(depth, &tail);
            }
            Stmt::For(init, condition, step, body) => {
                let mut part = |expr: &Option<Expr>| match expr {
                    Some(expr) => self.top_expr(expr),
                    None => String::new(),
                };
                let head = format!(
                    "for ({}; {}; {}) {{",
                    part(init),
                    part(condition),
                    part(step)
                );
                self.line(depth, &head);
                self.braced(&body.item, depth);
                self.line(depth, "}");
            }
            Stmt::Switch(value, parts) => {
                let head = format!("switch ({}) {{", self.expr(value, 0));
                self.line(depth, &head);
                for part in parts {
                    match &part.item {
                        SwitchPart::Case(case) => {
                            let text = format!("case {}:", self.expr(case, 0));
                            self.line(depth + 1, &text);
                        }
                        SwitchPart::Default => self.line(depth + 1, "default:"),
                        SwitchPart::Stmt(statement) => self.stmt(statement, depth + 2),
                    }
                }
                self.line(depth, "}");
            }
            Stmt::Break => self.line(depth, "break;"),
            Stmt::Continue => self.line(depth, "continue;"),
            Stmt::Return(value) => {
                let text = match value {
                    Some(value) => format!("return {};", self.expr(value, 0)),
                    None => "return;".into(),
                };
                self.line(depth, &text);
            }
        }
    }

    fn head(function: &Function) -> String {
        let params: Vec<String> = function
            .params
            .iter()
            .map(|param| format!("{} {}", ty_name(&param.ty), param.name))
            .collect();
        format!(
            "{} {}({})",
            ty_name(&function.ret),
            function.name,
            params.join(", ")
        )
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Include(_) | Item::Define(..) => {}
            Item::Struct(name, fields) => {
                let _ = writeln!(self.body, "struct {name} {{");
                for (ty, field) in fields {
                    let _ = writeln!(self.body, "    {} {field};", ty_name(ty));
                }
                self.body.push_str("};\n\n");
            }
            Item::Global { ty, names, .. } => {
                for (name, value) in names {
                    let text = match value {
                        Some(value) => {
                            format!("{} {name} = {};", ty_name(ty), self.expr(value, 0))
                        }
                        None => format!("{} {name};", ty_name(ty)),
                    };
                    self.line(0, &text);
                }
            }
            Item::Function(function) => match &function.body {
                None => {
                    let text = format!("{};", Self::head(function));
                    self.line(0, &text);
                }
                Some(body) => {
                    let text = format!("\n{} {{", Self::head(function));
                    self.line(0, &text);
                    self.statements(body, 1);
                    self.line(0, "}");
                }
            },
        }
    }

    /// The whole source: the `#define`s the numbers needed, then the items.
    pub(super) fn source(mut self, items: &[Item]) -> Result<String, String> {
        for item in items {
            self.item(item);
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut out = String::new();
        for (name, value) in &self.defines {
            let _ = writeln!(out, "#define {name} {value}");
        }
        if !self.defines.is_empty() {
            out.push('\n');
        }
        out.push_str(&self.body);
        Ok(out)
    }
}
