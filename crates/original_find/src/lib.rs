//! Finds the functions and fields of the original games' warp list in the
//! game's own decoded code. No address or offset of a game is written in
//! kjk, so they are found again on each run. Each one is known by the
//! strings its code uses or by the shape of the instructions that read it.
//!
//! - The warp list's click handler is the one function that uses both the
//!   format of the "Module Load" message (`%c%s.%s %s`) and the default
//!   player's name (`test`). Its first read of the clicked control gives the
//!   control's field that it checks. Its one 16-bit read of the menu gives
//!   the index of the picked module. The next field of the menu that it
//!   reads is the array of module names. The global it loads first and reads
//!   through is the pointer to the game's app manager. The offset it reads
//!   at is the client app's place in the app manager.
//! - The function that fills the warp list takes the click handler's
//!   address, to give each entry of the list its click.
//! - The function that opens the warp list is the one caller of the function
//!   that fills it. The second field of the menu that it reads is the flag
//!   that the menu's first frame sets.
//! - The hidden button's handler is the function that opens the warp list,
//!   or a short function that goes to it. The main menu's constructor takes
//!   the handler's address, to give the button its click. The constructor's
//!   caller stores the new menu in the client app's internal half, which
//!   gives the menu's place there.

use std::collections::BTreeSet;

use iced_x86::{
    Decoder, DecoderOptions, FlowControl, Instruction, MemorySize, Mnemonic, OpKind, Register,
};

/// The most instructions a function that only goes on to another one has.
/// Such a function is a button's handler that calls a member function, or a
/// constructor that jumps to the constructor that does the work.
const SHORT_FUNCTION: usize = 10;
/// The client app's offset in the app manager is below this.
const CLIENT_OFFSET_LIMIT: u64 = 0x40;
/// The instructions after the app manager's global is loaded in which its
/// reads are looked for.
const MANAGER_READS: usize = 3;
/// The instructions after the read of the picked index in which the read of
/// the array of names is looked for.
const NAMES_READ: usize = 6;
/// The instructions after the call to the menu's constructor in which the
/// store of the new menu is looked for.
const MENU_STORE: usize = 10;

/// Whether the game is a 32-bit or a 64-bit program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bitness {
    /// The Windows builds.
    Bits32,
    /// The Mac ports.
    Bits64,
}

impl Bitness {
    fn bits(self) -> u32 {
        match self {
            Bitness::Bits32 => 32,
            Bitness::Bits64 => 64,
        }
    }

    /// The size of a pointer, in bytes.
    pub fn pointer_size(self) -> u64 {
        u64::from(self.bits() / 8)
    }
}

/// The game's executable as it is in memory.
pub struct Image<'a> {
    pub code: &'a [u8],
    /// The address of the code's first byte.
    pub code_at: u64,
    /// The sections that hold the strings, each with its address.
    pub data: Vec<(&'a [u8], u64)>,
    pub bitness: Bitness,
}

/// The functions the library calls and the fields it reads and writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    /// The global that points to the game's app manager.
    pub app_manager: u64,
    /// The offsets that lead from the app manager to the main menu. The
    /// first leads to the client app, the second to its internal half, and
    /// the third to the main menu.
    pub menu_path: [u64; 3],
    /// The function that fills the warp list.
    pub populate_modules: u64,
    /// The warp list's click handler, which starts the picked module.
    pub on_module_picked: u64,
    /// The menu's flag that its first frame sets.
    pub drawn: u64,
    /// The menu's 16-bit index of the picked module.
    pub picked: u64,
    /// The menu's array of module names, a pointer followed by a count.
    pub modules: u64,
    /// The field of the clicked control that the click handler checks.
    pub control_check: u64,
}

/// The code, decoded once.
struct Code<'a> {
    image: &'a Image<'a>,
    /// The addresses where functions start. They are the targets of calls,
    /// the code addresses taken as values, and the frame prologues.
    starts: BTreeSet<u64>,
    /// Each instruction that takes a code address as a value, with that
    /// address.
    taken: Vec<(u64, u64)>,
    /// Each call, with the function it calls.
    calls: Vec<(u64, u64)>,
    /// Each instruction that takes the address of a wanted string, with that
    /// address.
    strings: Vec<(u64, u64)>,
}

impl<'a> Code<'a> {
    /// Decodes the image's code, and records the instructions that take the
    /// address of one of the `wanted` strings.
    fn decode(image: &'a Image<'a>, wanted: &[u64]) -> Code<'a> {
        let mut code = Code {
            image,
            starts: BTreeSet::new(),
            taken: Vec::new(),
            calls: Vec::new(),
            strings: Vec::new(),
        };
        let mut decoder = Decoder::with_ip(
            image.bitness.bits(),
            image.code,
            image.code_at,
            DecoderOptions::NONE,
        );
        let mut instruction = Instruction::default();
        let mut pushed_frame = None;
        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);
            let at = instruction.ip();
            // Clang reaches some functions only by a tail jump, so a frame's
            // prologue counts as a function's start even when nothing calls
            // it.
            if let Some(push) = pushed_frame.take()
                && is_frame_setup(&instruction)
            {
                code.starts.insert(push);
            }
            if instruction.mnemonic() == Mnemonic::Push
                && instruction.op0_kind() == OpKind::Register
                && matches!(instruction.op0_register(), Register::RBP | Register::EBP)
            {
                pushed_frame = Some(at);
            }
            if instruction.flow_control() == FlowControl::Call
                && matches!(
                    instruction.op0_kind(),
                    OpKind::NearBranch32 | OpKind::NearBranch64
                )
            {
                let target = instruction.near_branch_target();
                if code.in_code(target) {
                    code.starts.insert(target);
                    code.calls.push((at, target));
                }
                continue;
            }
            for value in values(&instruction) {
                if code.in_code(value) {
                    code.starts.insert(value);
                    code.taken.push((at, value));
                } else if wanted.contains(&value) {
                    code.strings.push((at, value));
                }
            }
        }
        code
    }

    fn in_code(&self, address: u64) -> bool {
        (self.image.code_at..self.code_end()).contains(&address)
    }

    fn code_end(&self) -> u64 {
        self.image.code_at + self.image.code.len() as u64
    }

    /// The start and the end of the function that `address` lies in.
    fn function_at(&self, address: u64) -> Option<(u64, u64)> {
        let start = *self.starts.range(..=address).next_back()?;
        let end = self
            .starts
            .range(start + 1..)
            .next()
            .copied()
            .unwrap_or(self.code_end());
        Some((start, end))
    }

    fn instructions(&self, start: u64) -> Vec<Instruction> {
        let Some((start, end)) = self.function_at(start) else {
            return Vec::new();
        };
        let from = (start - self.image.code_at) as usize;
        let to = (end - self.image.code_at) as usize;
        Decoder::with_ip(
            self.image.bitness.bits(),
            &self.image.code[from..to],
            start,
            DecoderOptions::NONE,
        )
        .into_iter()
        .collect()
    }

    /// The short functions that call or jump to `target`. A function's body
    /// ends at its first return or jump, so the padding after it does not
    /// count towards `SHORT_FUNCTION`.
    fn going_to(&self, target: u64) -> BTreeSet<u64> {
        let mut out = BTreeSet::new();
        for start in self.starts.iter().copied() {
            let instructions = self.instructions(start);
            let end = instructions
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction.flow_control(),
                        FlowControl::Return | FlowControl::UnconditionalBranch
                    )
                })
                .map(|index| index + 1)
                .unwrap_or(instructions.len());
            let body = &instructions[..end];
            if body.len() <= SHORT_FUNCTION
                && body.iter().any(|instruction| {
                    matches!(instruction.mnemonic(), Mnemonic::Jmp | Mnemonic::Call)
                        && instruction.near_branch_target() == target
                })
            {
                out.insert(start);
            }
        }
        out
    }

    /// The functions that hold an instruction of `refs` that refers to
    /// `target`.
    fn functions_with(&self, refs: &[(u64, u64)], target: u64) -> BTreeSet<u64> {
        refs.iter()
            .filter(|(_, value)| *value == target)
            .filter_map(|(at, _)| self.function_at(*at).map(|(start, _)| start))
            .collect()
    }
}

/// The addresses an instruction refers to. They are its immediates and its
/// absolute or RIP-relative memory operands.
fn values(instruction: &Instruction) -> Vec<u64> {
    let mut out = Vec::new();
    for operand in 0..instruction.op_count() {
        match instruction.op_kind(operand) {
            OpKind::Immediate32 | OpKind::Immediate32to64 | OpKind::Immediate64 => {
                out.push(instruction.immediate(operand))
            }
            OpKind::Memory => {
                // iced gives a RIP-relative operand's displacement as the
                // address it reaches.
                let base = instruction.memory_base();
                let relative = base == Register::RIP || base == Register::EIP;
                let absolute =
                    base == Register::None && instruction.memory_index() == Register::None;
                if relative || absolute {
                    out.push(instruction.memory_displacement64());
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether `instruction` is `mov rbp, rsp` or `mov ebp, esp`, the second
/// half of a frame's prologue.
fn is_frame_setup(instruction: &Instruction) -> bool {
    instruction.mnemonic() == Mnemonic::Mov
        && instruction.op0_kind() == OpKind::Register
        && instruction.op1_kind() == OpKind::Register
        && matches!(
            (instruction.op0_register(), instruction.op1_register()),
            (Register::RBP, Register::RSP) | (Register::EBP, Register::ESP)
        )
}

/// Whether `register` holds the stack. A read through it is of the frame's
/// locals, not of an object's field.
fn is_stack(register: Register) -> bool {
    matches!(
        register.full_register(),
        Register::RSP | Register::RBP | Register::ESP | Register::EBP
    )
}

/// The offset of the object's field that an instruction reads or writes.
/// It is none when the instruction's memory operand is on the stack, is
/// RIP-relative or is an absolute address.
fn field(instruction: &Instruction) -> Option<u64> {
    let memory =
        (0..instruction.op_count()).any(|operand| instruction.op_kind(operand) == OpKind::Memory);
    let base = instruction.memory_base();
    (memory
        && base != Register::None
        && base != Register::RIP
        && base != Register::EIP
        && !is_stack(base))
    .then(|| instruction.memory_displacement64())
}

/// The address of the zero-ended string `text` in the image's data. The
/// match must start a string, so a zero byte comes before it.
fn string(image: &Image, text: &[u8]) -> Option<u64> {
    let mut wanted = text.to_vec();
    wanted.push(0);
    image.data.iter().find_map(|(bytes, at)| {
        bytes
            .windows(wanted.len())
            .enumerate()
            .find(|(index, window)| {
                *window == wanted.as_slice() && (*index == 0 || bytes[index - 1] == 0)
            })
            .map(|(index, _)| at + index as u64)
    })
}

/// The one element of `set`, or why there is not exactly one.
fn one(set: BTreeSet<u64>, what: &str) -> Result<u64, String> {
    match set.len() {
        1 => Ok(*set.iter().next().unwrap_or(&0)),
        0 => Err(format!("no {what}")),
        n => Err(format!("{n} candidates for {what}: {set:x?}")),
    }
}

/// The game's functions and fields, or a text that says which of them was
/// not found.
pub fn find(image: &Image) -> Result<Found, String> {
    let format = string(image, b"%c%s.%s %s").ok_or("no \"Module Load\" format string")?;
    let player = string(image, b"test").ok_or("no default player name")?;
    let code = Code::decode(image, &[format, player]);
    let with_player = code.functions_with(&code.strings, player);
    let picked_fn = one(
        code.functions_with(&code.strings, format)
            .intersection(&with_player)
            .copied()
            .collect(),
        "warp list's click handler",
    )?;
    let populate = one(
        code.functions_with(&code.taken, picked_fn),
        "function that fills the warp list",
    )?;
    let load_game = one(
        code.functions_with(&code.calls, populate),
        "function that opens the warp list",
    )?;

    let picked_code = code.instructions(picked_fn);
    let control_check = control_field(&picked_code, image.bitness)
        .ok_or("the click handler's read of its control")?;
    let (picked, modules) =
        list_fields(&picked_code).ok_or("the click handler's reads of the module list")?;
    let (app_manager, client) =
        app_manager(&picked_code, image.bitness).ok_or("the app manager")?;

    let load_code = code.instructions(load_game);
    let read: Vec<u64> = load_code
        .iter()
        .take_while(|instruction| {
            !(instruction.mnemonic() == Mnemonic::Call
                && instruction.near_branch_target() == populate)
        })
        .filter_map(field)
        .collect();
    let drawn = *read.get(1).ok_or("the menu's flag for its first frame")?;

    let mut handlers = code.going_to(load_game);
    handlers.insert(load_game);
    let constructors: BTreeSet<u64> = handlers
        .iter()
        .flat_map(|handler| code.functions_with(&code.taken, *handler))
        .collect();
    let constructor = one(
        constructors,
        &format!("main menu's constructor (the hidden button's handlers {handlers:x?})"),
    )?;
    let menu = menu_field(&code, constructor).ok_or(format!(
        "where the main menu is kept (its constructor at {constructor:#x})"
    ))?;
    Ok(Found {
        app_manager,
        // The client app keeps its internal half right after its table of
        // virtual functions.
        menu_path: [client, image.bitness.pointer_size(), menu],
        populate_modules: populate,
        on_module_picked: picked_fn,
        drawn,
        picked,
        modules,
        control_check,
    })
}

/// The field of its control that the click handler reads first. The control
/// is the handler's second argument. On the Mac it is in `rsi`. On Windows
/// the handler is a member function that takes its arguments on the stack,
/// so the control is read from `[ebp+8]`.
fn control_field(code: &[Instruction], bitness: Bitness) -> Option<u64> {
    if bitness == Bitness::Bits64 {
        return code
            .iter()
            .find(|instruction| {
                instruction.memory_base() == Register::RSI && field(instruction).is_some()
            })
            .map(Instruction::memory_displacement64);
    }
    let (index, loaded) = code.iter().enumerate().find(|(_, instruction)| {
        instruction.mnemonic() == Mnemonic::Mov
            && instruction.memory_base() == Register::EBP
            && instruction.memory_displacement64() == 8
            && instruction.op0_kind() == OpKind::Register
    })?;
    let register = loaded.op0_register();
    code[index + 1..]
        .iter()
        .find(|instruction| instruction.memory_base() == register && field(instruction).is_some())
        .map(Instruction::memory_displacement64)
}

/// The offsets of the picked index and of the array of names. The picked
/// index is the only 16-bit field of the menu that the click handler reads.
/// The array of names is the next field it reads.
fn list_fields(code: &[Instruction]) -> Option<(u64, u64)> {
    let (index, picked) = code.iter().enumerate().find(|(_, instruction)| {
        matches!(instruction.mnemonic(), Mnemonic::Movsx | Mnemonic::Movsxd)
            && instruction.memory_size() == MemorySize::Int16
            && field(instruction).is_some()
    })?;
    let picked = picked.memory_displacement64();
    let modules = code[index + 1..]
        .iter()
        .take(NAMES_READ)
        .filter_map(field)
        .find(|offset| *offset != picked)?;
    Some((picked, modules))
}

/// The global that points to the app manager, and the client app's offset
/// in the app manager. The click handler loads the global's pointer and
/// then reads through it at a small offset.
fn app_manager(code: &[Instruction], bitness: Bitness) -> Option<(u64, u64)> {
    for (index, instruction) in code.iter().enumerate() {
        let next = &code[index + 1..code.len().min(index + 1 + MANAGER_READS)];
        // The 64-bit code takes the global's address with `lea r, [rip+G]`
        // and then loads it with `mov r2, [r]`. The 32-bit code loads it at
        // once with `mov r2, [G]`.
        let (global, manager) = if bitness == Bitness::Bits64 {
            if instruction.mnemonic() != Mnemonic::Lea || instruction.memory_base() != Register::RIP
            {
                continue;
            }
            let holder = instruction.op0_register();
            let Some(load) = next.iter().find(|next| {
                next.mnemonic() == Mnemonic::Mov
                    && next.memory_base() == holder
                    && next.memory_displacement64() == 0
                    && next.op0_kind() == OpKind::Register
            }) else {
                continue;
            };
            (instruction.memory_displacement64(), load.op0_register())
        } else {
            let absolute = instruction.mnemonic() == Mnemonic::Mov
                && instruction.op0_kind() == OpKind::Register
                && instruction.op1_kind() == OpKind::Memory
                && instruction.memory_base() == Register::None
                && instruction.memory_index() == Register::None;
            if !absolute {
                continue;
            }
            (
                instruction.memory_displacement64(),
                instruction.op0_register(),
            )
        };
        if let Some(client) = next.iter().find(|next| {
            next.memory_base() == manager
                && field(next).is_some_and(|offset| offset > 0 && offset < CLIENT_OFFSET_LIMIT)
        }) {
            return Some((global, client.memory_displacement64()));
        }
    }
    None
}

/// The offset where the client app's internal half keeps its main menu. It
/// is the field that the constructor's caller stores the new menu in, just
/// after the call. A call to a short function that goes on to the
/// constructor counts as a call to the constructor.
fn menu_field(code: &Code, constructor: u64) -> Option<u64> {
    let mut targets = code.going_to(constructor);
    targets.insert(constructor);
    for (at, target) in code.calls.iter().copied() {
        if !targets.contains(&target) {
            continue;
        }
        let (start, _) = code.function_at(at)?;
        let body = code.instructions(start);
        let after = body.iter().position(|instruction| instruction.ip() == at)? + 1;
        let stored = body[after..].iter().take(MENU_STORE).find(|instruction| {
            instruction.mnemonic() == Mnemonic::Mov
                && instruction.op0_kind() == OpKind::Memory
                && instruction.op1_kind() == OpKind::Register
                && field(instruction).is_some()
        });
        if let Some(stored) = stored {
            return Some(stored.memory_displacement64());
        }
    }
    None
}
