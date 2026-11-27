//! An NWScript decompiler: a compiled script (`.ncs`) back to source
//! (`.nss`) that `compile` turns into the same bytes again.
//!
//! It reads the layout `compile` writes (described there), one pattern at a
//! time, into the compiler's own tree (`compile::parse`), and prints that
//! tree. Names are lost in a compiled script, so variables and functions get
//! new ones (`nLocal1`, `sub1`); so are `const`s, includes and comments.
//!
//! A decompiled source is only handed out by `decompile_checked` when it
//! compiles back to the bytes it came from. What the reader does not yet
//! know (structures, the `?:` choice) is refused with the instruction it
//! stopped at, never guessed.

mod body;
mod print;

use std::collections::BTreeMap;

use fmt_odyssey::ncs::{Instruction, Ncs, Op, Ty as Cell};

use crate::catalog::Catalog;
use crate::compile::parse::{Function, Item, Param, Ty};

/// Why a script was not decompiled: the instruction the reader stopped at,
/// and what it met there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub at: usize,
    pub what: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instruction {}: {}", self.at, self.what)
    }
}

/// The start of a structure's name here. A compiled script keeps no
/// structure's name, so each gets one that says how many cells it has
/// (`Cells10_1`), which is all `size` needs to know of it.
const STRUCT: &str = "Cells";

/// A value's size on the stack, bytes.
fn size(ty: &Ty) -> i32 {
    match ty {
        Ty::Void | Ty::Action => 0,
        Ty::Vector => 12,
        Ty::Struct(name) => name
            .strip_prefix(STRUCT)
            .and_then(|rest| rest.split('_').next())
            .and_then(|cells| cells.parse::<i32>().ok())
            .map_or(0, |cells| cells * 4),
        _ => 4,
    }
}

/// The structures of a program: each one's name and its cells' types.
type Structs = Vec<(String, Vec<Ty>)>;

/// The type of a value of these cells: a vector for three floats, else the
/// structure with those cells, a new one if there is none yet.
fn aggregate(structs: &mut Structs, cells: &[Ty]) -> Ty {
    if matches!(cells, [Ty::Float, Ty::Float, Ty::Float]) {
        return Ty::Vector;
    }
    if let Some((name, _)) = structs.iter().find(|(_, own)| own == cells) {
        return Ty::Struct(name.clone());
    }
    let name = format!("{STRUCT}{}_{}", cells.len(), structs.len() + 1);
    structs.push((name.clone(), cells.to_vec()));
    Ty::Struct(name)
}

/// The name of a structure's cell `index`.
fn field_name(index: usize) -> String {
    format!("f{}", index + 1)
}

fn of_cell(cell: Cell) -> Ty {
    match cell {
        Cell::Int => Ty::Int,
        Cell::Float => Ty::Float,
        Cell::String => Ty::String,
        Cell::Object => Ty::Object,
        Cell::Effect => Ty::Effect,
        Cell::Event => Ty::Event,
        Cell::Location => Ty::Location,
        Cell::Talent => Ty::Talent,
    }
}

/// The program being read, as every function's reader sees it.
struct Program<'a> {
    code: &'a [Instruction],
    /// Where each jump lands, by instruction.
    targets: &'a [Option<usize>],
    catalog: &'a Catalog,
    /// The program's functions (`Sig`s) by their first instruction.
    by_start: &'a BTreeMap<usize, usize>,
    globals_size: i32,
}

/// What is known of one function of the program before it is read.
#[derive(Debug, Clone)]
struct Sig {
    name: String,
    /// Its instructions: `start..end`, the last a `RETN`.
    start: usize,
    end: usize,
    /// The size of its result, from a `return` in it (0: none found).
    ret_size: i32,
    ret: Option<Ty>,
    /// Its parameters' size, where a `return` shows it.
    params_size: Option<i32>,
    /// Its parameters' types, from the first call of it.
    params: Option<Vec<Ty>>,
}

/// A variable and where it lives: bytes from the frame's base for a local
/// (a parameter below 0), from the globals' start for a global.
#[derive(Debug, Clone)]
struct Var {
    name: String,
    ty: Ty,
    start: i32,
    /// A switch's value, kept on the stack while its body runs.
    switch: bool,
    /// The statement of its level that declares it.
    part: usize,
}

/// Where each jump of the program lands, by instruction.
fn targets(code: &[Instruction]) -> Vec<Option<usize>> {
    let by_offset: BTreeMap<u32, usize> = code
        .iter()
        .enumerate()
        .map(|(index, instruction)| (instruction.offset, index))
        .collect();
    code.iter()
        .map(|instruction| match instruction.op {
            Op::Jmp(by) | Op::Jsr(by) | Op::Jz(by) | Op::Jnz(by) => {
                let to = i64::from(instruction.offset) + i64::from(by);
                u32::try_from(to)
                    .ok()
                    .and_then(|to| by_offset.get(&to).copied())
            }
            _ => None,
        })
        .collect()
}

/// A function's result and parameter sizes, from a `return` of a value in
/// it: the value copied down below the frame, the whole frame popped, and a
/// jump to the function's end.
fn frame(
    code: &[Instruction],
    targets: &[Option<usize>],
    start: usize,
    end: usize,
) -> (i32, Option<i32>) {
    for jump in start + 2..end {
        let (Op::Jmp(_), Some(to)) = (&code[jump].op, targets[jump]) else {
            continue;
        };
        let params = if to + 1 == end {
            0
        } else if to + 2 == end {
            match code[to].op {
                Op::MovSp(by) if by < 0 => match by.checked_neg() {
                    Some(bytes) => bytes,
                    None => continue,
                },
                _ => continue,
            }
        } else {
            continue;
        };
        let (Op::MovSp(all), Op::CpDownSp { offset, size }) =
            (&code[jump - 1].op, &code[jump - 2].op)
        else {
            continue;
        };
        let size = i32::from(*size);
        if offset - all == -(params + size) {
            return (size, Some(params));
        }
    }
    (0, None)
}

/// A script's source, read back from its compiled form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decompiled {
    /// The source, in the games' code page (a byte a character).
    pub source: Vec<u8>,
    /// The routines the script calls with fewer arguments than
    /// `nwscript.nss` gives them parameters, and with how many. Such a
    /// script was compiled against an older `nwscript.nss`, in which the
    /// routine had no more parameters than that. It compiles to the same
    /// bytes only against a catalog cut the same way (`Catalog::cut`).
    pub routines: Vec<(String, usize)>,
}

/// The source of `ncs`.
pub fn decompile(ncs: &Ncs, catalog: &Catalog) -> Result<Decompiled, Error> {
    let code = &ncs.code[..];
    let fail = |at: usize, what: &str| Error {
        at,
        what: what.to_string(),
    };
    let targets = targets(code);

    // The start: the entry's int reserved (a `StartingConditional`), a call
    // and a `RETN`.
    let conditional = matches!(code.first().map(|i| &i.op), Some(Op::Rsadd(Cell::Int)));
    let call = usize::from(conditional);
    let first = match (code.get(call).map(|i| &i.op), targets.get(call)) {
        (Some(Op::Jsr(_)), Some(Some(first))) => *first,
        _ => return Err(fail(call, "the program does not start with a call")),
    };
    if !matches!(code.get(call + 1).map(|i| &i.op), Some(Op::Retn)) {
        return Err(fail(
            call + 1,
            "the program's first call is not followed by RETN",
        ));
    }

    let mut starts: Vec<usize> = code
        .iter()
        .zip(&targets)
        .filter_map(|(instruction, target)| match instruction.op {
            Op::Jsr(_) => *target,
            _ => None,
        })
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let end_of = |start: usize| -> usize {
        starts
            .iter()
            .copied()
            .find(|other| *other > start)
            .unwrap_or(code.len())
    };

    // The globals' function, when the first call's function saves BP.
    let first_end = end_of(first);
    let save = (first..first_end).find(|at| matches!(code[*at].op, Op::SaveBp));
    let (entry, globals_size) = match save {
        None => (first, 0),
        Some(save) => {
            let call = save + 1 + usize::from(conditional);
            // A call, and no other jump: a damaged script could hold a
            // plain jump there, which was taken for the entry's call.
            let entry = match (code.get(call).map(|at| &at.op), targets.get(call)) {
                (Some(Op::Jsr(_)), Some(Some(entry))) => entry,
                _ => {
                    return Err(fail(
                        call,
                        "the globals are not followed by the entry's call",
                    ));
                }
            };
            let restore = (call..first_end).find(|at| matches!(code[*at].op, Op::RestoreBp));
            let globals_size = match restore.and_then(|at| code.get(at + 1)).map(|i| &i.op) {
                Some(Op::MovSp(by)) => by.checked_neg().unwrap_or(0),
                _ => 0,
            };
            (*entry, globals_size)
        }
    };

    let mut sigs: Vec<Sig> = Vec::new();
    let mut by_start: BTreeMap<usize, usize> = BTreeMap::new();
    for start in starts.iter().copied() {
        if save.is_some() && start == first {
            continue;
        }
        let end = end_of(start);
        let (ret_size, params_size) = frame(code, &targets, start, end);
        let mut sig = Sig {
            name: format!("sub{}", sigs.len()),
            start,
            end,
            ret_size,
            ret: None,
            params_size,
            params: None,
        };
        if start == entry {
            sig.name = if conditional {
                "StartingConditional"
            } else {
                "main"
            }
            .to_string();
            sig.ret_size = if conditional { 4 } else { 0 };
            sig.ret = Some(if conditional { Ty::Int } else { Ty::Void });
            sig.params = Some(Vec::new());
        }
        by_start.insert(start, sigs.len());
        sigs.push(sig);
    }

    let program = Program {
        code,
        targets: &targets,
        catalog,
        by_start: &by_start,
        globals_size,
    };
    let mut fewer: BTreeMap<String, usize> = BTreeMap::new();
    let mut structs: Structs = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut globals: Vec<Var> = Vec::new();
    if let Some(save) = save {
        let mut machine = body::Machine::new(&program, &mut sigs, &mut structs, &[]);
        let (vars, values) = machine.globals(first, save)?;
        fewer.append(&mut machine.fewer);
        // A global copied whole as more than one cell is a vector or a
        // structure: its cells were declared one by one.
        let mut wholes: Vec<(i32, i32)> = code
            .iter()
            .filter_map(|instruction| match instruction.op {
                Op::CpTopBp { offset, size } | Op::CpDownBp { offset, size } if size > 4 => {
                    Some((globals_size + offset, i32::from(size)))
                }
                _ => None,
            })
            .collect();
        wholes.sort_unstable();
        wholes.dedup();
        let mut at = 0;
        while at < vars.len() {
            let var = &vars[at];
            let whole = wholes
                .iter()
                .find(|(start, bytes)| *start == var.start && *bytes > size(&var.ty));
            let mut count = 1;
            let mut ty = var.ty.clone();
            if let Some((start, bytes)) = whole {
                let cells: Vec<Ty> = vars[at..]
                    .iter()
                    .take_while(|other| other.start < start + bytes)
                    .map(|other| other.ty.clone())
                    .collect();
                let plain = values[at..at + cells.len()].iter().all(Option::is_none)
                    && cells.iter().all(|cell| size(cell) == 4)
                    && cells.len() as i32 * 4 == *bytes;
                if !plain {
                    return Err(fail(save, "a global structure's cells are not plain"));
                }
                count = cells.len();
                ty = aggregate(&mut structs, &cells);
            }
            globals.push(Var {
                name: var.name.clone(),
                ty: ty.clone(),
                start: var.start,
                switch: false,
                part: 0,
            });
            items.push(Item::Global {
                constant: false,
                ty,
                names: vec![(var.name.clone(), values[at].clone())],
            });
            at += count;
        }
    }

    let mut functions: Vec<Function> = Vec::new();
    for index in 0..sigs.len() {
        let mut machine = body::Machine::new(&program, &mut sigs, &mut structs, &globals);
        functions.push(machine.function(index)?);
        for (name, count) in std::mem::take(&mut machine.fewer) {
            if fewer
                .insert(name.clone(), count)
                .is_some_and(|had| had != count)
            {
                return Err(Error {
                    at: 0,
                    what: format!("{name} is called with two numbers of arguments"),
                });
            }
        }
    }
    // Every function but the entry is declared before the first body, so
    // the source reads in any compiler whatever order the bodies are in.
    for function in &functions {
        if function.name != "main" && function.name != "StartingConditional" {
            items.push(Item::Function(Function {
                ret: function.ret.clone(),
                name: function.name.clone(),
                params: function
                    .params
                    .iter()
                    .map(|param| Param {
                        ty: param.ty.clone(),
                        name: param.name.clone(),
                        default: None,
                    })
                    .collect(),
                body: None,
            }));
        }
    }
    items.extend(functions.into_iter().map(Item::Function));
    let declared = structs.iter().map(|(name, cells)| {
        let fields = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| (cell.clone(), field_name(index)))
            .collect();
        Item::Struct(name.clone(), fields)
    });
    items.splice(0..0, declared);

    let mut source = String::new();
    for (name, count) in &fewer {
        source.push_str(&format!(
            "// Compiled against an older nwscript.nss: {name} took {count} arguments.
"
        ));
    }
    source.push_str(
        &print::Printer::default()
            .source(&items)
            .map_err(|what| Error { at: 0, what })?,
    );
    Ok(Decompiled {
        source: source.chars().map(|c| c as u8).collect(),
        routines: fewer.into_iter().collect(),
    })
}

/// `decompile`, and the proof: the source compiled again must be `ncs` to
/// the byte, or it is refused.
pub fn decompile_checked(name: &str, ncs: &Ncs, catalog: &Catalog) -> Result<Decompiled, Error> {
    let decompiled = decompile(ncs, catalog)?;
    let cut;
    let catalog = if decompiled.routines.is_empty() {
        catalog
    } else {
        cut = catalog.cut(&decompiled.routines);
        &cut
    };
    let source = &decompiled.source;
    let again =
        crate::compile::compile_as_bioware(name, source, &|_| None, catalog).map_err(|error| {
            Error {
                at: 0,
                what: format!("the decompiled source does not compile: {error}"),
            }
        })?;
    if again.write() != ncs.write() {
        let at = again
            .code
            .iter()
            .zip(&ncs.code)
            .position(|(ours, theirs)| ours != theirs)
            .unwrap_or(again.code.len().min(ncs.code.len()));
        return Err(Error {
            at,
            what: "the decompiled source compiles to other bytes".into(),
        });
    }
    Ok(decompiled)
}
