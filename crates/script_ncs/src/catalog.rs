//! The engine routines and constants a game's scripts are compiled against,
//! read from its `nwscript.nss` (in each install; never copied here).
//!
//! The file is C-like: comments, `#define` lines, constants
//! (`int NAME = 1;`, `float`, `string`, or another constant's name) and
//! routine prototypes (`int Random(int nMaxInteger);`, defaults after
//! `=`, a vector's as `[x, y, z]`). A routine's number, what `ACTION`
//! names, is its place among the prototypes, from 0.

use std::collections::BTreeMap;
use std::fmt;

/// A value type in NWScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
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
    /// A routine's argument that is a deferred call (`DelayCommand`'s): no
    /// stack value, the state `STORE_STATE` saved.
    Action,
}

impl Type {
    /// A type's name. Without case, and `OBJECT_ID` as `object`: K1's own
    /// file declares `SetAvailableNPCId(INT nNPC, OBJECT_ID oidNPC)`.
    fn of(word: &str) -> Option<Type> {
        Some(match word.to_ascii_lowercase().as_str() {
            "object_id" => Type::Object,
            "void" => Type::Void,
            "int" => Type::Int,
            "float" => Type::Float,
            "string" => Type::String,
            "object" => Type::Object,
            "vector" => Type::Vector,
            "effect" => Type::Effect,
            "event" => Type::Event,
            "location" => Type::Location,
            "talent" => Type::Talent,
            "action" => Type::Action,
            _ => return None,
        })
    }

    /// Stack cells a value takes (a vector three, an action none).
    pub fn cells(self) -> usize {
        match self {
            Type::Void | Type::Action => 0,
            Type::Vector => 3,
            _ => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Type::Void => "void",
            Type::Int => "int",
            Type::Float => "float",
            Type::String => "string",
            Type::Object => "object",
            Type::Vector => "vector",
            Type::Effect => "effect",
            Type::Event => "event",
            Type::Location => "location",
            Type::Talent => "talent",
            Type::Action => "action",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
    /// The default as written (`TRUE`, `0.0f`, `[0.0,0.0,0.0]`, ...).
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Routine {
    pub id: u16,
    pub name: String,
    pub ret: Type,
    pub params: Vec<Param>,
}

impl fmt::Display for Routine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}(", self.ret.name(), self.name)?;
        for (n, param) in self.params.iter().enumerate() {
            let comma = if n > 0 { ", " } else { "" };
            write!(f, "{comma}{} {}", param.ty.name(), param.name)?;
            if let Some(default) = &param.default {
                write!(f, " = {default}")?;
            }
        }
        write!(f, ")")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub name: String,
    pub value: Value,
}

/// Everything a `nwscript.nss` declares.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    /// By number.
    pub routines: Vec<Routine>,
    pub constants: Vec<Constant>,
    /// The engine structures (`ENGINE_STRUCTURE_n`), in order.
    pub structures: Vec<String>,
}

/// Why a `nwscript.nss` could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError(pub String);

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nwscript.nss: {}", self.0)
    }
}

impl std::error::Error for CatalogError {}

/// The text without comments, strings kept whole.
fn uncommented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('"', _) => {
                out.push(c);
                for c in chars.by_ref() {
                    out.push(c);
                    if c == '"' {
                        break;
                    }
                }
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut star = false;
                for c in chars.by_ref() {
                    if star && c == '/' {
                        break;
                    }
                    star = c == '*';
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// `text` split on `separator` outside strings, brackets and parentheses.
fn split_top(text: &str, separator: char) -> Vec<&str> {
    let (mut parts, mut depth, mut quoted, mut start) = (Vec::new(), 0i32, false, 0);
    for (at, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' if !quoted => depth += 1,
            ')' | ']' if !quoted => depth -= 1,
            _ if c == separator && depth == 0 && !quoted => {
                parts.push(&text[start..at]);
                start = at + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, CatalogError> {
        let mut catalog = Catalog::default();
        let text = uncommented(text);
        let mut lines = Vec::new();
        for line in text.lines() {
            match line.trim().strip_prefix("#define") {
                Some(define) => {
                    let mut words = define.split_whitespace();
                    if let (Some(name), Some(value)) = (words.next(), words.next())
                        && name.starts_with("ENGINE_STRUCTURE_")
                    {
                        catalog.structures.push(value.to_string());
                    }
                }
                None => lines.push(line),
            }
        }
        let code = lines.join("\n");
        for statement in split_top(&code, ';') {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            match statement.split_once('(') {
                Some((head, rest)) if !head.contains('=') => catalog.routine(head, rest)?,
                _ => catalog.constant(statement)?,
            }
        }
        Ok(catalog)
    }

    fn routine(&mut self, head: &str, rest: &str) -> Result<(), CatalogError> {
        let error = || CatalogError(format!("routine {:?}", head.trim()));
        let mut words = head.split_whitespace();
        let ret = words.next().and_then(Type::of).ok_or_else(error)?;
        let name = words.next().ok_or_else(error)?.to_string();
        let list = rest.trim().strip_suffix(')').ok_or_else(error)?;
        let mut params = Vec::new();
        for param in split_top(list, ',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }
            let (declared, default) = match param.split_once('=') {
                Some((declared, default)) => (declared, Some(default.trim().to_string())),
                None => (param, None),
            };
            let mut words = declared.split_whitespace();
            let ty = words.next().and_then(Type::of).ok_or_else(error)?;
            let name = words.next().ok_or_else(error)?.to_string();
            params.push(Param { ty, name, default });
        }
        let id = u16::try_from(self.routines.len()).map_err(|_| error())?;
        self.routines.push(Routine {
            id,
            name,
            ret,
            params,
        });
        Ok(())
    }

    fn constant(&mut self, statement: &str) -> Result<(), CatalogError> {
        let error = || CatalogError(format!("constant {statement:?}"));
        let (declared, value) = statement.split_once('=').ok_or_else(error)?;
        let mut words = declared.split_whitespace();
        let ty = words.next().and_then(Type::of).ok_or_else(error)?;
        let name = words.next().ok_or_else(error)?.to_string();
        let value = value.trim();
        let value = match (ty, self.value(value)) {
            (_, Some(known)) => known,
            (Type::Int, None) => Value::Int(value.parse().map_err(|_| error())?),
            (Type::Float, None) => {
                Value::Float(value.trim_end_matches('f').parse().map_err(|_| error())?)
            }
            (Type::String, None) => Value::String(
                value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .ok_or_else(error)?
                    .to_string(),
            ),
            _ => return Err(error()),
        };
        self.constants.push(Constant { name, value });
        Ok(())
    }

    /// An earlier constant's value, by name.
    fn value(&self, name: &str) -> Option<Value> {
        self.constant_named(name).map(|c| c.value.clone())
    }

    pub fn constant_named(&self, name: &str) -> Option<&Constant> {
        self.constants.iter().find(|c| c.name == name)
    }

    pub fn routine_named(&self, name: &str) -> Option<&Routine> {
        self.routines.iter().find(|r| r.name == name)
    }

    pub fn routine_by_id(&self, id: u16) -> Option<&Routine> {
        self.routines.get(usize::from(id))
    }

    /// How many routines and constants of each type: for reports.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for routine in &self.routines {
            *counts.entry(routine.ret.name()).or_insert(0) += 1;
        }
        counts
    }
}
