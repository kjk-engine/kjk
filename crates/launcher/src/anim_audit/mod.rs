//! The saber animation audit: reads the frames `kjk play --audit-log PATH`
//! writes (`kjk_game::anim_audit_log`) and measures them (`checks`).

use std::collections::BTreeMap;

use serde::Deserialize;

pub mod checks;

/// One drawn frame of the player's figure, as `AuditFrame` writes it. Every
/// place is in game space, in metres.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Frame {
    /// The drawn moment (sim ms).
    pub ms: f64,
    /// The sim's legs and torso sequence names.
    pub sim: [String; 2],
    /// The legs and torso sequence names the drawn body plays.
    pub shown: [String; 2],
    /// The drawn body plays another sequence than the sim's on a part.
    pub swapped: bool,
    /// The saber move cuts.
    pub cuts: bool,
    /// The move cuts, or the saber is blocked, in a lock or blocking a bolt.
    pub contact: bool,
    /// The saber rules in play (`ja` or `mda`).
    pub rules: String,
    /// The drawn body's nodes, by KOTOR name. A frame may lack any of them.
    #[serde(default)]
    pub nodes: BTreeMap<String, [f32; 3]>,
    /// The same body parts on JA's skeleton, by JA's bone name. A frame may
    /// lack any of them.
    #[serde(default)]
    pub ref_nodes: BTreeMap<String, [f32; 3]>,
    #[serde(default)]
    pub blades: Vec<Blade>,
}

/// One lit blade in hand, as `AuditBlade` writes it. Each blade is its hilt
/// and its tip.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Blade {
    /// Its place among the character's blades.
    pub slot: u8,
    /// The drawn blade is still growing or shrinking.
    pub growing: bool,
    pub drawn: [[f32; 3]; 2],
    /// JA's blade as the view poses it.
    pub reference: [[f32; 3]; 2],
    pub sim: [[f32; 3]; 2],
    /// The drawn hand that grips the blade.
    pub hand: [f32; 3],
}

/// The frames of an audit log's text, one JSON object per line. Blank lines
/// are skipped.
pub fn parse(text: &str) -> serde_json::Result<Vec<Frame>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect()
}
