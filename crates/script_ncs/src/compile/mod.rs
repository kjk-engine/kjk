//! An NWScript compiler: a script's source (`.nss`) to the games' compiled
//! form (`.ncs`, `fmt_odyssey::ncs`), against a game's `nwscript.nss`
//! (`Catalog`). It lays the program out as BioWare's compiler does, so the
//! games' own scripts compile to the bytes the games ship (`kjk script
//! compile` sweeps an install for how many do).
//!
//! What the layout is, from the games' compiled scripts:
//!
//! - The program starts with a call of the script's `main` (or
//!   `StartingConditional`, whose int is reserved first) and a `RETN`.
//!   Globals are set by a function of their own around that call.
//! - A local is reserved where it is declared (`RSADD`), its value copied
//!   down onto it (`CPDOWNSP`) and the copy popped. A block pops its locals
//!   at its end.
//! - A call's arguments are pushed last first, every default written out.
//!   An action argument (`DelayCommand`'s) is `STORESTATE`, a jump over its
//!   code, the code and a `RETN`.
//! - `return` copies its value down to the reserved result, pops the
//!   function's locals and jumps to its end; the code after it is still
//!   written, as if it went on.
//! - An `if` jumps over its body when false (`JZ`), and its body ends with a
//!   jump past the `else`, even when there is none.
//! - A `switch` keeps its value on the stack and tests a copy of it against
//!   each case in turn (`JNZ`).

mod code;
pub mod lexer;
pub mod parse;

use std::collections::BTreeSet;

use fmt_odyssey::ncs::Ncs;

use crate::catalog::Catalog;

/// Why a script does not compile: where, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub file: String,
    pub line: u32,
    pub what: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.nss line {}: {}", self.file, self.line, self.what)
    }
}

/// A file's items, each with the file it is from, the includes read in
/// where they stand, each file once.
fn load(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<(String, parse::Item)>,
) -> Result<(), Error> {
    let tokens = lexer::lex(source, name)?;
    for item in parse::parse(&tokens, name)? {
        match item {
            parse::Item::Include(include) => {
                if include == "nwscript" || !seen.insert(include.clone()) {
                    continue;
                }
                let bytes = read(&include).ok_or_else(|| Error {
                    file: name.to_string(),
                    line: 0,
                    what: format!("the include {include} is not found"),
                })?;
                load(&include, &bytes, read, seen, out)?;
            }
            item => out.push((name.to_string(), item)),
        }
    }
    Ok(())
}

/// How a `continue`, and a `break` out of a `switch`, leave the blocks
/// they are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    /// They pop the locals of the blocks they leave. A script written now
    /// is compiled so (`compile`).
    Clean,
    /// They pop nothing, as BioWare's compiler writes them, so the locals
    /// stay on the stack and every later place of the function is off. 12
    /// of the two games' compiled scripts hold this, The Sith Lords'
    /// floating mines among them. It is what proves a decompiled script
    /// (`decompile::decompile_checked`), and what `kjk script compile`
    /// measures the compiler against the games with
    /// (`compile_as_bioware`).
    AsBioWare,
}

/// Compiles the script `name` (`source`, its `.nss`). `read` gives an
/// include's source by name. The program is the script's `main` or
/// `StartingConditional` and what they call.
///
/// It is laid out as BioWare's compiler lays it out, but for one fault of
/// that compiler which this does not copy (`Leaving::Clean`).
pub fn compile(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
) -> Result<Ncs, Error> {
    compile_debug(name, source, read, catalog).map(|(ncs, _)| ncs)
}

/// `compile`, with what a debugger needs to read the program's runs
/// (`Debug`).
pub fn compile_debug(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
) -> Result<(Ncs, Debug), Error> {
    compiled(name, source, read, catalog, Leaving::Clean)
}

/// `compile`, to the bytes BioWare's compiler writes, its fault with
/// `continue` and `break` copied (`Leaving::AsBioWare`). It is for
/// comparing with a script the games ship, never for a script to run.
pub fn compile_as_bioware(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
) -> Result<Ncs, Error> {
    compiled(name, source, read, catalog, Leaving::AsBioWare).map(|(ncs, _)| ncs)
}

fn compiled(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
    leaving: Leaving,
) -> Result<(Ncs, Debug), Error> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    seen.insert(name.to_ascii_lowercase());
    load(name, source, read, &mut seen, &mut items)?;
    code::program(name, &items, catalog, leaving)
}

/// What the compiler notes as it writes, by instruction (its place in the
/// program, made offsets by `Debug::of`).
#[derive(Debug, Clone, Default)]
pub(crate) struct Rec {
    pub lines: Vec<(usize, String, u32)>,
    pub functions: Vec<(String, String, usize, usize)>,
    pub locals: Vec<(String, String, String, i32, usize, usize)>,
}

/// What a debugger needs of a compiled script: where each source line's
/// code starts, each function's code, each local's place while it lives,
/// and each global's. Offsets are the instructions' (`Instruction::offset`),
/// places are bytes: a local's from the stack's top as its function starts
/// (a parameter's below it), a global's from the stack's bottom.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Debug {
    /// (offset, file, line) where a statement's code starts.
    pub lines: Vec<(u32, String, u32)>,
    /// (name, file, first offset, past-last offset).
    pub functions: Vec<(String, String, u32, u32)>,
    pub locals: Vec<DebugLocal>,
    /// (name, type, place). The place is from the first global.
    pub globals: Vec<(String, String, i32)>,
    /// The bytes all the globals take together. 0 in notes written before
    /// this was kept, and for a script with no globals.
    pub globals_size: i32,
}

/// A local variable or a parameter, as `Debug` keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugLocal {
    pub function: String,
    pub name: String,
    pub ty: String,
    pub home: i32,
    /// The offsets it lives from and before.
    pub from: u32,
    pub to: u32,
}

impl Debug {
    /// The notes made `rec` into offsets of `ncs`.
    pub(crate) fn of(
        rec: &Rec,
        ncs: &Ncs,
        globals: Vec<(String, String, i32)>,
        globals_size: i32,
    ) -> Debug {
        // Past the last instruction: the program's length.
        let end = ncs.write().len() as u32;
        let offset = |at: usize| {
            ncs.code
                .get(at)
                .map_or(end, |instruction| instruction.offset)
        };
        Debug {
            lines: rec
                .lines
                .iter()
                .map(|(at, file, line)| (offset(*at), file.clone(), *line))
                .collect(),
            functions: rec
                .functions
                .iter()
                .map(|(name, file, from, to)| {
                    (name.clone(), file.clone(), offset(*from), offset(*to))
                })
                .collect(),
            locals: rec
                .locals
                .iter()
                .map(|(function, name, ty, home, from, to)| DebugLocal {
                    function: function.clone(),
                    name: name.clone(),
                    ty: ty.clone(),
                    home: *home,
                    from: offset(*from),
                    to: offset(*to),
                })
                .collect(),
            globals,
            globals_size,
        }
    }

    /// The text `NAME.ndb` holds: a line for each note.
    pub fn write(&self) -> String {
        let mut out = String::from("kjk-ndb 1\n");
        for (offset, file, line) in &self.lines {
            out.push_str(&format!("line {offset} {file} {line}\n"));
        }
        for (name, file, from, to) in &self.functions {
            out.push_str(&format!("fn {name} {file} {from} {to}\n"));
        }
        for local in &self.locals {
            out.push_str(&format!(
                "local {} {} {} {} {} {}\n",
                local.function, local.name, local.ty, local.home, local.from, local.to
            ));
        }
        for (name, ty, home) in &self.globals {
            out.push_str(&format!("global {name} {ty} {home}\n"));
        }
        if self.globals_size != 0 {
            out.push_str(&format!("globals {}\n", self.globals_size));
        }
        out
    }

    /// The notes `write` wrote.
    pub fn parse(text: &str) -> Result<Debug, String> {
        let mut lines = text.lines();
        if lines.next() != Some("kjk-ndb 1") {
            return Err("not a kjk debug file (kjk-ndb 1)".into());
        }
        let mut debug = Debug::default();
        let number = |word: &str| {
            word.parse::<i64>()
                .map_err(|_| format!("{word:?} is no number"))
        };
        for line in lines {
            let words: Vec<&str> = line.split(' ').collect();
            match words.as_slice() {
                ["line", offset, file, at] => {
                    debug.lines.push((
                        number(offset)? as u32,
                        file.to_string(),
                        number(at)? as u32,
                    ));
                }
                ["fn", name, file, from, to] => debug.functions.push((
                    name.to_string(),
                    file.to_string(),
                    number(from)? as u32,
                    number(to)? as u32,
                )),
                ["local", function, name, ty, home, from, to] => debug.locals.push(DebugLocal {
                    function: function.to_string(),
                    name: name.to_string(),
                    ty: ty.to_string(),
                    home: number(home)? as i32,
                    from: number(from)? as u32,
                    to: number(to)? as u32,
                }),
                ["global", name, ty, home] => {
                    debug
                        .globals
                        .push((name.to_string(), ty.to_string(), number(home)? as i32));
                }
                ["globals", size] => debug.globals_size = number(size)? as i32,
                [""] => {}
                _ => return Err(format!("{line:?} is not a debug note")),
            }
        }
        Ok(debug)
    }

    /// The stack cell that holds the first cell of the global at `home`
    /// (its place in `globals`), in a run whose globals end at the cell `bp`
    /// (`vm::Watch::bp`).
    ///
    /// The globals are not always at the stack's bottom. A script that
    /// starts at `StartingConditional` keeps its answer below them, so
    /// there the first global is the second cell. Counted from the bottom,
    /// every global of such a script was read one cell too low. Code that
    /// runs later (`DelayCommand`) has nothing below its globals.
    pub fn global_cell(&self, home: i32, bp: usize) -> i64 {
        let first = match self.globals_size {
            // Notes written before the size was kept: from the bottom.
            0 => 0,
            size => bp as i64 - i64::from(size) / 4,
        };
        first + i64::from(home) / 4
    }

    /// The function whose code holds `offset`.
    pub fn function_at(&self, offset: u32) -> Option<&(String, String, u32, u32)> {
        self.functions
            .iter()
            .find(|(_, _, from, to)| (*from..*to).contains(&offset))
    }

    /// The source line a statement's code starts at `offset`, if one does.
    pub fn line_at(&self, offset: u32) -> Option<(&str, u32)> {
        self.lines
            .iter()
            .rev()
            .find(|(at, _, _)| *at == offset)
            .map(|(_, file, line)| (file.as_str(), *line))
    }
}

/// Whether `source` is a script with a body to run (`main` or
/// `StartingConditional`), not an include of functions.
pub fn has_entry(source: &[u8]) -> bool {
    let Ok(tokens) = lexer::lex(source, "") else {
        return false;
    };
    tokens.windows(2).any(|pair| {
        matches!(&pair[0].tok, lexer::Tok::Name(name) if name == "main" || name == "StartingConditional")
            && matches!(pair[1].tok, lexer::Tok::Mark("("))
    })
}
