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

/// Compiles the script `name` (`source`, its `.nss`). `read` gives an
/// include's source by name. The program is the script's `main` or
/// `StartingConditional` and what they call.
pub fn compile(
    name: &str,
    source: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
) -> Result<Ncs, Error> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    seen.insert(name.to_ascii_lowercase());
    load(name, source, read, &mut seen, &mut items)?;
    code::program(name, &items, catalog)
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
