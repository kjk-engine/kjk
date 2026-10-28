//! A readable listing of a compiled script: one instruction a line at its
//! file offset, jump targets labelled (`sub_` for what `JSR` calls, `L_`
//! for the rest), and each `ACTION` named with its prototype when the
//! game's catalog is given.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use fmt_odyssey::ncs::{Ncs, Op};

use crate::Catalog;

/// The listing of `ncs`, named from `catalog` when there is one.
pub fn listing(ncs: &Ncs, catalog: Option<&Catalog>) -> String {
    let mut subs = BTreeSet::new();
    let mut labels = BTreeSet::new();
    for instruction in &ncs.code {
        if let Some(target) = instruction.op.target(instruction.offset) {
            if matches!(instruction.op, Op::Jsr(_)) {
                subs.insert(target);
            } else {
                labels.insert(target);
            }
        }
    }
    let label = |offset: u32| {
        if subs.contains(&offset) {
            format!("sub_{offset:08x}")
        } else {
            format!("L_{offset:08x}")
        }
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "; {} instructions, {} subroutines",
        ncs.code.len(),
        subs.len()
    );
    for instruction in &ncs.code {
        let offset = instruction.offset;
        if subs.contains(&offset) || labels.contains(&offset) {
            let _ = writeln!(out, "{}:", label(offset));
        }
        let op = &instruction.op;
        let text = match op {
            Op::Action { routine, args } => match catalog.and_then(|c| c.routine_by_id(*routine)) {
                Some(found) => format!("ACTION {}, {args}    ; {found}", found.name),
                None => op.to_string(),
            },
            _ => match op.target(offset) {
                Some(target) => format!("{:<20} -> {}", op.to_string(), label(target)),
                None => op.to_string(),
            },
        };
        let _ = writeln!(out, "  {offset:08x}  {text}");
    }
    out
}
