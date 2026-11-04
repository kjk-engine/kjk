//! Party changes, queued for the save's scripts to make as it loads
//! (`sim::script::calls`): a companion made available by its template,
//! joined, sent off, or forgotten by the routines the games' own scripts
//! call, so its creature is made as kjk makes every companion's
//! (`make_npc`: its sheet, items and career as the party table keeps
//! them). Creature creation is the engine's, which a save alone cannot do.

use std::fmt;

use sim::Save;
use sim::script::calls::{self, Call, Queued, int, text};
use sim::script::{MODULE, Pending};

use crate::Editor;

/// A party change a save holds queued, as `pending_party` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyChange {
    /// `AddAvailableNPCByTemplate`.
    MadeAvailable { npc: u8, template: String },
    /// `SpawnAvailableNPC` beside the first player, then `AddPartyMember`.
    Joins { npc: u8 },
    /// `RemovePartyMember`.
    Leaves { npc: u8 },
    /// `RemoveAvailableNPC`.
    Forgotten { npc: u8 },
}

impl fmt::Display for PartyChange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PartyChange::MadeAvailable { npc, template } => {
                write!(f, "companion {npc} made available as {template}")
            }
            PartyChange::Joins { npc } => write!(f, "companion {npc} joins the party"),
            PartyChange::Leaves { npc } => write!(f, "companion {npc} leaves the party"),
            PartyChange::Forgotten { npc } => write!(f, "companion {npc} is no longer available"),
        }
    }
}

impl Editor<'_> {
    /// Companion `npc` made available as `template`'s creature, as
    /// `AddAvailableNPCByTemplate` makes it when the save loads.
    pub fn make_available(&mut self, npc: u8, template: &str) -> Result<(), String> {
        if template.trim().is_empty() {
            return Err("a companion needs its template".into());
        }
        let npc = i32::from(npc);
        self.queue(vec![
            text(template),
            int(npc),
            Call::Routine("AddAvailableNPCByTemplate", 2),
        ])
    }

    /// Companion `npc` (available by then) made beside the first player
    /// and joined to the party when the save loads.
    pub fn join(&mut self, npc: u8) -> Result<(), String> {
        let npc = i32::from(npc);
        self.queue(vec![
            Call::Routine("GetFirstPC", 0),
            Call::Routine("GetLocation", 1),
            int(npc),
            Call::Routine("SpawnAvailableNPC", 2),
            int(npc),
            Call::Routine("AddPartyMember", 2),
        ])
    }

    /// Companion `npc` out of the party when the save loads.
    pub fn leave(&mut self, npc: u8) -> Result<(), String> {
        self.queue(vec![
            int(i32::from(npc)),
            Call::Routine("RemovePartyMember", 1),
        ])
    }

    /// Companion `npc` no longer available when the save loads.
    pub fn remove_available(&mut self, npc: u8) -> Result<(), String> {
        self.queue(vec![
            int(i32::from(npc)),
            Call::Routine("RemoveAvailableNPC", 1),
        ])
    }

    /// `calls` run by the module when the save loads, after what waits.
    fn queue(&mut self, calls: Vec<Call>) -> Result<(), String> {
        let name = calls::queued(&[calls])?;
        self.save.script.pending.push(Pending::new(&name, MODULE));
        Ok(())
    }
}

/// The party changes `save` holds queued, in the order they run.
pub fn pending_party(save: &Save) -> Vec<PartyChange> {
    save.script
        .pending
        .iter()
        .filter_map(|pending| calls::steps(&pending.script)?.ok())
        .filter_map(|steps| change(&steps))
        .collect()
}

/// The change a queued run's steps make, if it is one of the party's.
fn change(steps: &[Queued]) -> Option<PartyChange> {
    let npc = steps.iter().find_map(|step| match step {
        Queued::Int(npc) => u8::try_from(*npc).ok(),
        _ => None,
    })?;
    let Some(Queued::Routine(last, _)) = steps.last() else {
        return None;
    };
    Some(match last.as_str() {
        "AddAvailableNPCByTemplate" => {
            let template = steps.iter().find_map(|step| match step {
                Queued::Text(template) => Some(template.clone()),
                _ => None,
            })?;
            PartyChange::MadeAvailable { npc, template }
        }
        "AddPartyMember" => PartyChange::Joins { npc },
        "RemovePartyMember" => PartyChange::Leaves { npc },
        "RemoveAvailableNPC" => PartyChange::Forgotten { npc },
        _ => return None,
    })
}
