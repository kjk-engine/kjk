//! NCS V1.0: compiled NWScript, the bytecode the games' script VM runs.
//!
//! Layout, every number big-endian: the tag `NCS V1.0`, then `0x42` and
//! the file's length (u32), then instructions to the end. An instruction
//! is its opcode (u8), its qualifier (u8: the operand types) and the
//! opcode's arguments. Jumps are relative to the jumping instruction.
//! Strings are bytes in the game's code page, kept as bytes.
//!
//! `Ncs::parse` refuses an opcode, a qualifier or an argument it does not
//! know; `Ncs::write` writes the same bytes back.

use std::fmt;

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "NCS";
/// The tag, the program-size marker and the length.
const HEADER_LEN: usize = 13;
const SIZE_MARKER: u8 = 0x42;

/// A single operand type (a qualifier of `RSADD`, `CONST`, `NEG`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Float,
    String,
    Object,
    Effect,
    Event,
    Location,
    Talent,
}

impl Ty {
    const CODES: [(u8, Ty); 8] = [
        (0x03, Ty::Int),
        (0x04, Ty::Float),
        (0x05, Ty::String),
        (0x06, Ty::Object),
        (0x10, Ty::Effect),
        (0x11, Ty::Event),
        (0x12, Ty::Location),
        (0x13, Ty::Talent),
    ];

    fn of(code: u8) -> Option<Ty> {
        Ty::CODES
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, ty)| *ty)
    }

    fn code(self) -> u8 {
        Ty::CODES
            .iter()
            .find(|(_, ty)| *ty == self)
            .expect("every type has a code")
            .0
    }

    /// The engine types: opaque handles the VM keeps for the host.
    pub fn engine(self) -> bool {
        matches!(self, Ty::Effect | Ty::Event | Ty::Location | Ty::Talent)
    }

    fn letter(self) -> &'static str {
        match self {
            Ty::Int => "I",
            Ty::Float => "F",
            Ty::String => "S",
            Ty::Object => "O",
            Ty::Effect => "EFF",
            Ty::Event => "EVT",
            Ty::Location => "LOC",
            Ty::Talent => "TAL",
        }
    }
}

/// The operand types of a two-operand instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pair {
    IntInt,
    FloatFloat,
    ObjectObject,
    StringString,
    /// Two structures (vectors among them) of this many bytes each.
    Struct(u16),
    IntFloat,
    FloatInt,
    /// Two engine values of one type.
    Engine(Ty),
    VectorVector,
    VectorFloat,
    FloatVector,
}

impl Pair {
    fn of(code: u8) -> Option<Pair> {
        Some(match code {
            0x20 => Pair::IntInt,
            0x21 => Pair::FloatFloat,
            0x22 => Pair::ObjectObject,
            0x23 => Pair::StringString,
            0x25 => Pair::IntFloat,
            0x26 => Pair::FloatInt,
            0x30 => Pair::Engine(Ty::Effect),
            0x31 => Pair::Engine(Ty::Event),
            0x32 => Pair::Engine(Ty::Location),
            0x33 => Pair::Engine(Ty::Talent),
            0x3a => Pair::VectorVector,
            0x3b => Pair::VectorFloat,
            0x3c => Pair::FloatVector,
            _ => return None,
        })
    }

    fn code(self) -> u8 {
        match self {
            Pair::IntInt => 0x20,
            Pair::FloatFloat => 0x21,
            Pair::ObjectObject => 0x22,
            Pair::StringString => 0x23,
            Pair::Struct(_) => 0x24,
            Pair::IntFloat => 0x25,
            Pair::FloatInt => 0x26,
            Pair::Engine(ty) => 0x30 + ty.code() - 0x10,
            Pair::VectorVector => 0x3a,
            Pair::VectorFloat => 0x3b,
            Pair::FloatVector => 0x3c,
        }
    }

    fn letters(self) -> String {
        match self {
            Pair::IntInt => "II".into(),
            Pair::FloatFloat => "FF".into(),
            Pair::ObjectObject => "OO".into(),
            Pair::StringString => "SS".into(),
            Pair::Struct(size) => format!("TT {size}"),
            Pair::IntFloat => "IF".into(),
            Pair::FloatInt => "FI".into(),
            Pair::Engine(ty) => format!("{0}{0}", ty.letter()),
            Pair::VectorVector => "VV".into(),
            Pair::VectorFloat => "VF".into(),
            Pair::FloatVector => "FV".into(),
        }
    }
}

/// A constant pushed by `CONST`.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i32),
    Float(f32),
    String(Vec<u8>),
    Object(i32),
}

/// One instruction, by its meaning. Stack offsets and sizes are bytes
/// (four a cell); jumps are bytes from the jumping instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Copies the top `size` bytes down to `offset` (negative) from the top.
    CpDownSp {
        offset: i32,
        size: u16,
    },
    /// Reserves a cell of this type, empty.
    Rsadd(Ty),
    /// Copies `size` bytes from `offset` (negative) to the top.
    CpTopSp {
        offset: i32,
        size: u16,
    },
    Const(Const),
    /// Calls engine routine `routine` (its index in `nwscript.nss`) with
    /// `args` arguments from the top.
    Action {
        routine: u16,
        args: u8,
    },
    LogAnd,
    LogOr,
    IncOr,
    ExcOr,
    BoolAnd,
    Equal(Pair),
    NotEqual(Pair),
    Geq(Pair),
    Gt(Pair),
    Lt(Pair),
    Leq(Pair),
    ShiftLeft,
    ShiftRight,
    UShiftRight,
    Add(Pair),
    Sub(Pair),
    Mul(Pair),
    Div(Pair),
    Mod,
    Neg(Ty),
    Comp,
    MovSp(i32),
    Jmp(i32),
    Jsr(i32),
    Jz(i32),
    Retn,
    /// Removes `size` bytes from the top but `keep` bytes at `offset`
    /// within them.
    Destruct {
        size: u16,
        offset: i16,
        keep: u16,
    },
    Not,
    DecSp(i32),
    IncSp(i32),
    Jnz(i32),
    CpDownBp {
        offset: i32,
        size: u16,
    },
    CpTopBp {
        offset: i32,
        size: u16,
    },
    DecBp(i32),
    IncBp(i32),
    SaveBp,
    RestoreBp,
    /// Saves the stack (`bp` bytes of globals, `sp` of locals) for the
    /// action argument that follows (`DelayCommand`, `AssignCommand`).
    StoreState {
        bp: u32,
        sp: u32,
    },
    Nop,
}

/// An instruction and where it starts in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub offset: u32,
    pub op: Op,
}

/// A whole program.
#[derive(Debug, Clone, PartialEq)]
pub struct Ncs {
    pub code: Vec<Instruction>,
}

/// Reads big-endian values off a cursor.
struct Cursor<'a> {
    reader: Reader<'a>,
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8], FormatError> {
        let bytes = self.reader.slice(self.at, len)?;
        self.at += len;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(self.u32()? as i32)
    }
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: FORMAT,
        what,
    }
}

impl Ncs {
    pub fn parse(bytes: &[u8]) -> Result<Ncs, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"NCS V1.0")?;
        let mut cursor = Cursor { reader, at: 8 };
        let marker = cursor.u8()?;
        let length = cursor.u32()? as usize;
        if marker != SIZE_MARKER || length != bytes.len() {
            return Err(unsupported(format!(
                "header {marker:#04x} length {length} in a {}-byte file",
                bytes.len()
            )));
        }
        let mut code = Vec::new();
        while cursor.at < bytes.len() {
            let offset = cursor.at as u32;
            let op = Ncs::op(&mut cursor).map_err(|error| match error {
                FormatError::Unsupported { what, .. } => unsupported(format!("{what} at {offset}")),
                other => other,
            })?;
            code.push(Instruction { offset, op });
        }
        Ok(Ncs { code })
    }

    fn op(cursor: &mut Cursor) -> Result<Op, FormatError> {
        let (opcode, qualifier) = (cursor.u8()?, cursor.u8()?);
        let bad = || unsupported(format!("opcode {opcode:#04x} qualifier {qualifier:#04x}"));
        let ty = || Ty::of(qualifier).ok_or_else(bad);
        let int = || (qualifier == 0x03).then_some(()).ok_or_else(bad);
        let pair = |cursor: &mut Cursor| -> Result<Pair, FormatError> {
            if qualifier == 0x24 {
                return Ok(Pair::Struct(cursor.u16()?));
            }
            Pair::of(qualifier).ok_or_else(bad)
        };
        let ints = |op: Op| int().map(|()| op);
        let none = |op: Op| (qualifier == 0x00).then_some(op).ok_or_else(bad);
        let copy = |cursor: &mut Cursor| -> Result<(i32, u16), FormatError> {
            (qualifier == 0x01).then_some(()).ok_or_else(bad)?;
            Ok((cursor.i32()?, cursor.u16()?))
        };
        Ok(match opcode {
            0x01 => {
                let (offset, size) = copy(cursor)?;
                Op::CpDownSp { offset, size }
            }
            0x02 => Op::Rsadd(ty()?),
            0x03 => {
                let (offset, size) = copy(cursor)?;
                Op::CpTopSp { offset, size }
            }
            0x04 => Op::Const(match ty()? {
                Ty::Int => Const::Int(cursor.i32()?),
                Ty::Float => Const::Float(f32::from_bits(cursor.u32()?)),
                Ty::String => {
                    let len = usize::from(cursor.u16()?);
                    Const::String(cursor.take(len)?.to_vec())
                }
                Ty::Object => Const::Object(cursor.i32()?),
                _ => return Err(bad()),
            }),
            0x05 => none(Op::Action {
                routine: cursor.u16()?,
                args: cursor.u8()?,
            })?,
            0x06..=0x0a | 0x11..=0x13 | 0x18 => {
                (qualifier == 0x20).then_some(()).ok_or_else(bad)?;
                match opcode {
                    0x06 => Op::LogAnd,
                    0x07 => Op::LogOr,
                    0x08 => Op::IncOr,
                    0x09 => Op::ExcOr,
                    0x0a => Op::BoolAnd,
                    0x11 => Op::ShiftLeft,
                    0x12 => Op::ShiftRight,
                    0x13 => Op::UShiftRight,
                    _ => Op::Mod,
                }
            }
            0x0b => Op::Equal(pair(cursor)?),
            0x0c => Op::NotEqual(pair(cursor)?),
            0x0d => Op::Geq(pair(cursor)?),
            0x0e => Op::Gt(pair(cursor)?),
            0x0f => Op::Lt(pair(cursor)?),
            0x10 => Op::Leq(pair(cursor)?),
            0x14 => Op::Add(pair(cursor)?),
            0x15 => Op::Sub(pair(cursor)?),
            0x16 => Op::Mul(pair(cursor)?),
            0x17 => Op::Div(pair(cursor)?),
            0x19 => Op::Neg(ty()?),
            0x1a => ints(Op::Comp)?,
            0x1b => none(Op::MovSp(cursor.i32()?))?,
            0x1d => none(Op::Jmp(cursor.i32()?))?,
            0x1e => none(Op::Jsr(cursor.i32()?))?,
            0x1f => none(Op::Jz(cursor.i32()?))?,
            0x20 => none(Op::Retn)?,
            0x21 => {
                (qualifier == 0x01).then_some(()).ok_or_else(bad)?;
                Op::Destruct {
                    size: cursor.u16()?,
                    offset: cursor.u16()? as i16,
                    keep: cursor.u16()?,
                }
            }
            0x22 => ints(Op::Not)?,
            0x23 => ints(Op::DecSp(cursor.i32()?))?,
            0x24 => ints(Op::IncSp(cursor.i32()?))?,
            0x25 => none(Op::Jnz(cursor.i32()?))?,
            0x26 => {
                let (offset, size) = copy(cursor)?;
                Op::CpDownBp { offset, size }
            }
            0x27 => {
                let (offset, size) = copy(cursor)?;
                Op::CpTopBp { offset, size }
            }
            0x28 => ints(Op::DecBp(cursor.i32()?))?,
            0x29 => ints(Op::IncBp(cursor.i32()?))?,
            0x2a => none(Op::SaveBp)?,
            0x2b => none(Op::RestoreBp)?,
            0x2c => {
                (qualifier == 0x10).then_some(()).ok_or_else(bad)?;
                Op::StoreState {
                    bp: cursor.u32()?,
                    sp: cursor.u32()?,
                }
            }
            0x2d => none(Op::Nop)?,
            _ => return Err(bad()),
        })
    }

    /// The file: header and every instruction, as `parse` read them.
    pub fn write(&self) -> Vec<u8> {
        let mut out = b"NCS V1.0".to_vec();
        out.push(SIZE_MARKER);
        out.extend_from_slice(&[0; 4]);
        for instruction in &self.code {
            write_op(&mut out, &instruction.op);
        }
        let length = out.len() as u32;
        out[9..HEADER_LEN].copy_from_slice(&length.to_be_bytes());
        out
    }

    /// Each instruction's offset as `write` lays it out (what `parse` would
    /// read back), for a program built in memory.
    pub fn lay_out(&mut self) {
        let mut at = HEADER_LEN as u32;
        for instruction in &mut self.code {
            instruction.offset = at;
            let mut bytes = Vec::new();
            write_op(&mut bytes, &instruction.op);
            at += bytes.len() as u32;
        }
    }

    /// The instruction starting at `offset`, by index.
    pub fn at(&self, offset: u32) -> Option<usize> {
        self.code
            .binary_search_by_key(&offset, |instruction| instruction.offset)
            .ok()
    }
}

fn write_op(out: &mut Vec<u8>, op: &Op) {
    let mut head = |opcode: u8, qualifier: u8| out.extend_from_slice(&[opcode, qualifier]);
    match op {
        Op::CpDownSp { .. } => head(0x01, 0x01),
        Op::Rsadd(ty) => head(0x02, ty.code()),
        Op::CpTopSp { .. } => head(0x03, 0x01),
        Op::Const(constant) => head(
            0x04,
            match constant {
                Const::Int(_) => 0x03,
                Const::Float(_) => 0x04,
                Const::String(_) => 0x05,
                Const::Object(_) => 0x06,
            },
        ),
        Op::Action { .. } => head(0x05, 0x00),
        Op::LogAnd => head(0x06, 0x20),
        Op::LogOr => head(0x07, 0x20),
        Op::IncOr => head(0x08, 0x20),
        Op::ExcOr => head(0x09, 0x20),
        Op::BoolAnd => head(0x0a, 0x20),
        Op::Equal(pair) => head(0x0b, pair.code()),
        Op::NotEqual(pair) => head(0x0c, pair.code()),
        Op::Geq(pair) => head(0x0d, pair.code()),
        Op::Gt(pair) => head(0x0e, pair.code()),
        Op::Lt(pair) => head(0x0f, pair.code()),
        Op::Leq(pair) => head(0x10, pair.code()),
        Op::ShiftLeft => head(0x11, 0x20),
        Op::ShiftRight => head(0x12, 0x20),
        Op::UShiftRight => head(0x13, 0x20),
        Op::Add(pair) => head(0x14, pair.code()),
        Op::Sub(pair) => head(0x15, pair.code()),
        Op::Mul(pair) => head(0x16, pair.code()),
        Op::Div(pair) => head(0x17, pair.code()),
        Op::Mod => head(0x18, 0x20),
        Op::Neg(ty) => head(0x19, ty.code()),
        Op::Comp => head(0x1a, 0x03),
        Op::MovSp(_) => head(0x1b, 0x00),
        Op::Jmp(_) => head(0x1d, 0x00),
        Op::Jsr(_) => head(0x1e, 0x00),
        Op::Jz(_) => head(0x1f, 0x00),
        Op::Retn => head(0x20, 0x00),
        Op::Destruct { .. } => head(0x21, 0x01),
        Op::Not => head(0x22, 0x03),
        Op::DecSp(_) => head(0x23, 0x03),
        Op::IncSp(_) => head(0x24, 0x03),
        Op::Jnz(_) => head(0x25, 0x00),
        Op::CpDownBp { .. } => head(0x26, 0x01),
        Op::CpTopBp { .. } => head(0x27, 0x01),
        Op::DecBp(_) => head(0x28, 0x03),
        Op::IncBp(_) => head(0x29, 0x03),
        Op::SaveBp => head(0x2a, 0x00),
        Op::RestoreBp => head(0x2b, 0x00),
        Op::StoreState { .. } => head(0x2c, 0x10),
        Op::Nop => head(0x2d, 0x00),
    }
    let u16 = |out: &mut Vec<u8>, v: u16| out.extend_from_slice(&v.to_be_bytes());
    let u32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_be_bytes());
    match op {
        Op::CpDownSp { offset, size }
        | Op::CpTopSp { offset, size }
        | Op::CpDownBp { offset, size }
        | Op::CpTopBp { offset, size } => {
            u32(out, *offset as u32);
            u16(out, *size);
        }
        Op::Const(Const::Int(v) | Const::Object(v)) => u32(out, *v as u32),
        Op::Const(Const::Float(v)) => u32(out, v.to_bits()),
        Op::Const(Const::String(bytes)) => {
            u16(out, bytes.len() as u16);
            out.extend_from_slice(bytes);
        }
        Op::Action { routine, args } => {
            u16(out, *routine);
            out.push(*args);
        }
        Op::Equal(Pair::Struct(size))
        | Op::NotEqual(Pair::Struct(size))
        | Op::Geq(Pair::Struct(size))
        | Op::Gt(Pair::Struct(size))
        | Op::Lt(Pair::Struct(size))
        | Op::Leq(Pair::Struct(size))
        | Op::Add(Pair::Struct(size))
        | Op::Sub(Pair::Struct(size))
        | Op::Mul(Pair::Struct(size))
        | Op::Div(Pair::Struct(size)) => u16(out, *size),
        Op::MovSp(v)
        | Op::Jmp(v)
        | Op::Jsr(v)
        | Op::Jz(v)
        | Op::Jnz(v)
        | Op::DecSp(v)
        | Op::IncSp(v)
        | Op::DecBp(v)
        | Op::IncBp(v) => u32(out, *v as u32),
        Op::Destruct { size, offset, keep } => {
            u16(out, *size);
            u16(out, *offset as u16);
            u16(out, *keep);
        }
        Op::StoreState { bp, sp } => {
            u32(out, *bp);
            u32(out, *sp);
        }
        _ => {}
    }
}

impl fmt::Display for Op {
    /// The mnemonic and its arguments (a jump's as a relative offset).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::CpDownSp { offset, size } => write!(f, "CPDOWNSP {offset}, {size}"),
            Op::Rsadd(ty) => write!(f, "RSADD{}", ty.letter()),
            Op::CpTopSp { offset, size } => write!(f, "CPTOPSP {offset}, {size}"),
            Op::Const(Const::Int(v)) => write!(f, "CONSTI {v}"),
            Op::Const(Const::Float(v)) => write!(f, "CONSTF {v:?}"),
            Op::Const(Const::String(bytes)) => {
                write!(f, "CONSTS {:?}", String::from_utf8_lossy(bytes))
            }
            Op::Const(Const::Object(v)) => write!(f, "CONSTO {v}"),
            Op::Action { routine, args } => write!(f, "ACTION {routine}, {args}"),
            Op::LogAnd => write!(f, "LOGANDII"),
            Op::LogOr => write!(f, "LOGORII"),
            Op::IncOr => write!(f, "INCORII"),
            Op::ExcOr => write!(f, "EXCORII"),
            Op::BoolAnd => write!(f, "BOOLANDII"),
            Op::Equal(pair) => write!(f, "EQUAL{}", pair.letters()),
            Op::NotEqual(pair) => write!(f, "NEQUAL{}", pair.letters()),
            Op::Geq(pair) => write!(f, "GEQ{}", pair.letters()),
            Op::Gt(pair) => write!(f, "GT{}", pair.letters()),
            Op::Lt(pair) => write!(f, "LT{}", pair.letters()),
            Op::Leq(pair) => write!(f, "LEQ{}", pair.letters()),
            Op::ShiftLeft => write!(f, "SHLEFTII"),
            Op::ShiftRight => write!(f, "SHRIGHTII"),
            Op::UShiftRight => write!(f, "USHRIGHTII"),
            Op::Add(pair) => write!(f, "ADD{}", pair.letters()),
            Op::Sub(pair) => write!(f, "SUB{}", pair.letters()),
            Op::Mul(pair) => write!(f, "MUL{}", pair.letters()),
            Op::Div(pair) => write!(f, "DIV{}", pair.letters()),
            Op::Mod => write!(f, "MODII"),
            Op::Neg(ty) => write!(f, "NEG{}", ty.letter()),
            Op::Comp => write!(f, "COMPI"),
            Op::MovSp(v) => write!(f, "MOVSP {v}"),
            Op::Jmp(v) => write!(f, "JMP {v:+}"),
            Op::Jsr(v) => write!(f, "JSR {v:+}"),
            Op::Jz(v) => write!(f, "JZ {v:+}"),
            Op::Retn => write!(f, "RETN"),
            Op::Destruct { size, offset, keep } => {
                write!(f, "DESTRUCT {size}, {offset}, {keep}")
            }
            Op::Not => write!(f, "NOTI"),
            Op::DecSp(v) => write!(f, "DECISP {v}"),
            Op::IncSp(v) => write!(f, "INCISP {v}"),
            Op::Jnz(v) => write!(f, "JNZ {v:+}"),
            Op::CpDownBp { offset, size } => write!(f, "CPDOWNBP {offset}, {size}"),
            Op::CpTopBp { offset, size } => write!(f, "CPTOPBP {offset}, {size}"),
            Op::DecBp(v) => write!(f, "DECIBP {v}"),
            Op::IncBp(v) => write!(f, "INCIBP {v}"),
            Op::SaveBp => write!(f, "SAVEBP"),
            Op::RestoreBp => write!(f, "RESTOREBP"),
            Op::StoreState { bp, sp } => write!(f, "STORESTATE {bp}, {sp}"),
            Op::Nop => write!(f, "NOP"),
        }
    }
}

impl Op {
    /// Where a jump (`JMP`, `JSR`, `JZ`, `JNZ`) at `offset` goes.
    pub fn target(&self, offset: u32) -> Option<u32> {
        match self {
            Op::Jmp(v) | Op::Jsr(v) | Op::Jz(v) | Op::Jnz(v) => {
                Some(offset.wrapping_add(*v as u32))
            }
            _ => None,
        }
    }

    /// The mnemonic alone, for counting.
    pub fn name(&self) -> String {
        let text = self.to_string();
        text.split(' ').next().unwrap_or("").to_string()
    }
}
