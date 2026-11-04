//! Programs built from instructions here rather than compiled from a
//! script: the VM's probes, and the routine calls kjk itself runs
//! (`sim::script::calls`).

use std::collections::BTreeMap;

use fmt_odyssey::ncs::{Instruction, Ncs, Op};

/// Builds a program from instructions and labelled jumps.
#[derive(Default)]
pub struct Asm {
    pub(crate) code: Vec<(Op, Option<&'static str>)>,
    pub(crate) labels: BTreeMap<&'static str, usize>,
}

impl Asm {
    pub fn op(&mut self, op: Op) -> &mut Asm {
        self.code.push((op, None));
        self
    }

    /// A jump (`Op::Jmp(0)` and the like) to `label`.
    pub fn to(&mut self, jump: fn(i32) -> Op, label: &'static str) -> &mut Asm {
        self.code.push((jump(0), Some(label)));
        self
    }

    pub fn label(&mut self, name: &'static str) -> &mut Asm {
        self.labels.insert(name, self.code.len());
        self
    }

    /// The program, jumps resolved.
    pub fn build(&self) -> Ncs {
        let mut ncs = Ncs {
            code: self
                .code
                .iter()
                .map(|(op, _)| Instruction {
                    offset: 0,
                    op: op.clone(),
                })
                .collect(),
        };
        ncs.lay_out();
        let offsets: Vec<u32> = ncs.code.iter().map(|i| i.offset).collect();
        for (at, (_, label)) in self.code.iter().enumerate() {
            let Some(label) = label else { continue };
            let target = self.labels.get(label).map_or(0, |&n| offsets[n]);
            let rel = target as i32 - offsets[at] as i32;
            ncs.code[at].op = match ncs.code[at].op {
                Op::Jmp(_) => Op::Jmp(rel),
                Op::Jsr(_) => Op::Jsr(rel),
                Op::Jz(_) => Op::Jz(rel),
                _ => Op::Jnz(rel),
            };
        }
        ncs
    }
}
