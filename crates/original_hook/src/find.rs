//! The game's functions and fields, found in its own code each run. No
//! address or offset of a game is written in kjk. The library decodes the
//! game's code and follows the strings and the instruction shapes the warp
//! list's code is known by.
//!
//! - `OnModulePicked` is the one function that uses both the "Module Load"
//!   message's format (`%c%s.%s %s`) and the default player's name
//!   (`test`). Its first read of its control gives the field it checks.
//!   Its 16-bit read of the menu gives the picked index, and the next field
//!   it reads is the array of module names. The global it loads first and
//!   reads through gives the app manager and the client app's place in it.
//! - `PopulateModules` takes `OnModulePicked`'s address, to give each entry
//!   of the list its click.
//! - `OnLoadGamePicked` is the one caller of `PopulateModules`. The second
//!   field of the menu it reads is the flag the menu's first frame sets.
//! - The warp button's handler is `OnLoadGamePicked`, or a few instructions
//!   that jump to it. The menu's constructor takes the handler's address,
//!   to give the button its click. The constructor's caller stores the new
//!   menu in the client app's internal half, which gives the menu's place.

use std::collections::BTreeSet;

use iced_x86::{
    Decoder, DecoderOptions, FlowControl, Instruction, MemorySize, Mnemonic, OpKind, Register,
};

/// The game's executable as it is in memory.
pub struct Image<'a> {
    /// The code section, and its address.
    pub code: &'a [u8],
    pub code_at: u64,
    /// The sections that hold the strings, with their addresses.
    pub data: Vec<(&'a [u8], u64)>,
    /// 64 for the Mac ports, 32 for the Windows builds.
    pub bitness: u32,
}

/// What the library calls and touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    /// The global `g_pAppManager`, a pointer.
    pub app_manager: u64,
    /// The offsets that lead from the app manager to the main menu: to the
    /// client app, to its internal half, and to its main menu pointer.
    pub menu_path: [u64; 3],
    pub populate_modules: u64,
    pub on_module_picked: u64,
    /// The menu's flag that its first frame sets.
    pub drawn: u64,
    /// The menu's index of the picked module, 16 bits.
    pub picked: u64,
    /// The menu's array of module names, a pointer followed by a count.
    pub modules: u64,
    /// The field of the picked control that `OnModulePicked` checks.
    pub control_check: u64,
}

/// The code, decoded once: where functions start, and what each
/// instruction refers to.
struct Code<'a> {
    image: &'a Image<'a>,
    /// Call targets and code addresses taken as values.
    starts: BTreeSet<u64>,
    /// (instruction, the code address it takes as a value).
    taken: Vec<(u64, u64)>,
    /// (instruction, the function it calls).
    calls: Vec<(u64, u64)>,
}

impl<'a> Code<'a> {
    fn decode(image: &'a Image<'a>) -> Code<'a> {
        let mut code = Code {
            image,
            starts: BTreeSet::new(),
            taken: Vec::new(),
            calls: Vec::new(),
        };
        let mut decoder = Decoder::with_ip(
            image.bitness,
            image.code,
            image.code_at,
            DecoderOptions::NONE,
        );
        let mut instruction = Instruction::default();
        let mut pushed_frame = None;
        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);
            let at = instruction.ip();
            // A frame's prologue starts a function, even one that nothing
            // calls: Clang reaches some functions only by a tail jump.
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
                && instruction.op0_kind() == OpKind::NearBranch64
                || instruction.flow_control() == FlowControl::Call
                    && instruction.op0_kind() == OpKind::NearBranch32
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
                }
            }
        }
        code
    }

    fn in_code(&self, address: u64) -> bool {
        (self.image.code_at..self.image.code_at + self.image.code.len() as u64).contains(&address)
    }

    /// The function `address` lies in: its start and its end.
    fn function_at(&self, address: u64) -> Option<(u64, u64)> {
        let start = *self.starts.range(..=address).next_back()?;
        let end = self
            .starts
            .range(start + 1..)
            .next()
            .copied()
            .unwrap_or(self.image.code_at + self.image.code.len() as u64);
        Some((start, end))
    }

    /// The instructions of the function that starts at `start`.
    fn instructions(&self, start: u64) -> Vec<Instruction> {
        let Some((start, end)) = self.function_at(start) else {
            return Vec::new();
        };
        let from = (start - self.image.code_at) as usize;
        let to = (end - self.image.code_at) as usize;
        Decoder::with_ip(
            self.image.bitness,
            &self.image.code[from..to],
            start,
            DecoderOptions::NONE,
        )
        .into_iter()
        .collect()
    }

    /// The functions of ten instructions at most that go on to `target`, by
    /// a call or a jump: a handler that calls a member function, or a C1
    /// constructor that jumps to the C2 one. A function's body ends at its
    /// first return or jump, so the padding after it does not count.
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
            if body.len() <= 10
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

    /// The functions with an instruction that refers to `target` in
    /// `refs`.
    fn functions_with(&self, refs: &[(u64, u64)], target: u64) -> BTreeSet<u64> {
        refs.iter()
            .filter(|(_, value)| *value == target)
            .filter_map(|(at, _)| self.function_at(*at).map(|(start, _)| start))
            .collect()
    }
}

/// The addresses an instruction refers to: an immediate, an absolute memory
/// operand, or a RIP-relative one.
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

/// Whether `register` holds the stack: a frame's locals, not an object.
fn is_stack(register: Register) -> bool {
    matches!(
        register.full_register(),
        Register::RSP | Register::RBP | Register::ESP | Register::EBP
    )
}

/// The displacement of an instruction's memory operand on an object's
/// register: not the stack, not RIP, not an absolute address.
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

/// The address of the NUL-ended string `text` in the image's data. It has
/// to start a string, so a NUL comes before it.
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

/// The one element of `set`, or why there is not one.
fn one(set: BTreeSet<u64>, what: &str) -> Result<u64, String> {
    match set.len() {
        1 => Ok(*set.iter().next().unwrap_or(&0)),
        0 => Err(format!("no {what}")),
        n => Err(format!("{n} candidates for {what}: {set:x?}")),
    }
}

/// The game's functions and fields, or which of them was not found.
pub fn find(image: &Image) -> Result<Found, String> {
    let format = string(image, b"%c%s.%s %s").ok_or("no \"Module Load\" format string")?;
    let player = string(image, b"test").ok_or("no default player name")?;
    let code = Code::decode(image);
    // A string's address is taken by a `lea`, `push` or `mov`, which
    // `values` reads, but it lies outside the code, so `Code::decode` kept
    // none of them. They are found again here.
    let refs = |target: u64| -> BTreeSet<u64> {
        let mut decoder = Decoder::with_ip(
            image.bitness,
            image.code,
            image.code_at,
            DecoderOptions::NONE,
        );
        let mut found = BTreeSet::new();
        let mut instruction = Instruction::default();
        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);
            if values(&instruction).contains(&target)
                && let Some((start, _)) = code.function_at(instruction.ip())
            {
                found.insert(start);
            }
        }
        found
    };
    let with_player = refs(player);
    let picked_fn = one(
        refs(format).intersection(&with_player).copied().collect(),
        "OnModulePicked",
    )?;
    let populate = one(
        code.functions_with(&code.taken, picked_fn),
        "PopulateModules",
    )?;
    let load_game = one(
        code.functions_with(&code.calls, populate),
        "OnLoadGamePicked",
    )?;

    let picked_code = code.instructions(picked_fn);
    let control_check =
        control_field(&picked_code, image.bitness).ok_or("OnModulePicked's control field")?;
    let (picked, modules) = list_fields(&picked_code).ok_or("OnModulePicked's list fields")?;
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
    let drawn = *read.get(1).ok_or("OnLoadGamePicked's drawn flag")?;

    // The warp button's handler: `OnLoadGamePicked` itself, or a function
    // of a few instructions that goes to it.
    let mut handlers = code.going_to(load_game);
    handlers.insert(load_game);
    let constructors: BTreeSet<u64> = handlers
        .iter()
        .flat_map(|handler| code.functions_with(&code.taken, *handler))
        .collect();
    let constructor = one(
        constructors,
        &format!("the main menu's constructor (the warp button's handlers {handlers:x?})"),
    )?;
    let menu = menu_field(&code, constructor).ok_or(format!(
        "where the main menu is kept (its constructor at {constructor:#x})"
    ))?;
    let pointer = u64::from(image.bitness / 8);
    Ok(Found {
        app_manager,
        // The client app keeps its internal half right after its table of
        // virtual functions.
        menu_path: [client, pointer, menu],
        populate_modules: populate,
        on_module_picked: picked_fn,
        drawn,
        picked,
        modules,
        control_check,
    })
}

/// The field of its control that `OnModulePicked` reads first. The control
/// is its second argument: `rsi` on the Mac, the stack's `[ebp+8]` with
/// `thiscall`.
fn control_field(code: &[Instruction], bitness: u32) -> Option<u64> {
    if bitness == 64 {
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

/// The picked index, the menu's one 16-bit field `OnModulePicked` reads,
/// and the array of names, the next field it reads.
fn list_fields(code: &[Instruction]) -> Option<(u64, u64)> {
    let (index, picked) = code.iter().enumerate().find(|(_, instruction)| {
        matches!(instruction.mnemonic(), Mnemonic::Movsx | Mnemonic::Movsxd)
            && instruction.memory_size() == MemorySize::Int16
            && field(instruction).is_some()
    })?;
    let picked = picked.memory_displacement64();
    let modules = code[index + 1..]
        .iter()
        .take(6)
        .filter_map(field)
        .find(|offset| *offset != picked)?;
    Some((picked, modules))
}

/// The global `OnModulePicked` reads a pointer from and then reads through
/// at a small offset: the app manager, and its client app's place.
fn app_manager(code: &[Instruction], bitness: u32) -> Option<(u64, u64)> {
    for (index, instruction) in code.iter().enumerate() {
        let next = &code[index + 1..code.len().min(index + 4)];
        if bitness == 64 {
            // `lea r, [rip+G]`, `mov r2, [r]`, `mov r3, [r2+client]`.
            if instruction.mnemonic() != Mnemonic::Lea || instruction.memory_base() != Register::RIP
            {
                continue;
            }
            let global = instruction.memory_displacement64();
            let holder = instruction.op0_register();
            let Some(load) = next.iter().find(|next| {
                next.mnemonic() == Mnemonic::Mov
                    && next.memory_base() == holder
                    && next.memory_displacement64() == 0
                    && next.op0_kind() == OpKind::Register
            }) else {
                continue;
            };
            let manager = load.op0_register();
            if let Some(client) = next.iter().find(|next| {
                next.memory_base() == manager
                    && field(next).is_some_and(|offset| offset > 0 && offset < 0x40)
            }) {
                return Some((global, client.memory_displacement64()));
            }
        } else {
            // `mov r, [G]`, `mov r2, [r+client]`.
            let absolute = instruction.mnemonic() == Mnemonic::Mov
                && instruction.op0_kind() == OpKind::Register
                && instruction.op1_kind() == OpKind::Memory
                && instruction.memory_base() == Register::None
                && instruction.memory_index() == Register::None;
            if !absolute {
                continue;
            }
            let manager = instruction.op0_register();
            if let Some(client) = next.iter().find(|next| {
                next.memory_base() == manager
                    && field(next).is_some_and(|offset| offset > 0 && offset < 0x40)
            }) {
                return Some((
                    instruction.memory_displacement64(),
                    client.memory_displacement64(),
                ));
            }
        }
    }
    None
}

/// Where the client app's internal half keeps its main menu: the field the
/// constructor's caller stores the new menu in, just after the call. A
/// constructor reached through a function of a few instructions (a C1
/// constructor that jumps to the C2 one) counts as called.
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
        let stored = body[after..].iter().take(10).find(|instruction| {
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
