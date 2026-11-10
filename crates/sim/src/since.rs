//! The version each change to a format saves hold arrived in, by the name
//! of its bump file (`bumps/README.md`): a decoder reads the layout before
//! it with `if input.since(crate::since::<format>::<NAME>)`: the version of
//! that format the bytes are in, wherever in a save they lie. A pending
//! bump's comes from the build script (`crate::pending::since`); the merge's
//! pin (`cargo xtask pin`) writes its number here. A format with no bump
//! pending re-exports nothing, hence the `allow`s.

pub mod save {
    pub const CLAUDE_TSL_DANTOOINE: crate::Since = crate::Since(crate::Stored::Save, 60);
    pub const CLAUDE_TSL_NAR_SHADDAA: crate::Since = crate::Since(crate::Stored::Save, 61);
    pub const CLAUDE_TSL_ONDERON: crate::Since = crate::Since(crate::Stored::Save, 62);
    pub const CLAUDE_TSL_TELOS: crate::Since = crate::Since(crate::Stored::Save, 63);
    pub const SAVE_MIGRATIONS: crate::Since = crate::Since(crate::Stored::Save, 64);
    pub const TSL_BUGS: crate::Since = crate::Since(crate::Stored::Save, 65);
    pub const CLAUDE_TSL_FINALE_2: crate::Since = crate::Since(crate::Stored::Save, 66);
    pub const COOP_PLAYTEST_FIXES: crate::Since = crate::Since(crate::Stored::Save, 67);
    pub const FEAT_CYCLE: crate::Since = crate::Since(crate::Stored::Save, 68);
    pub const ITEM_CYCLE: crate::Since = crate::Since(crate::Stored::Save, 69);
    pub const COMPANION_KEPT_ITEMS: crate::Since = crate::Since(crate::Stored::Save, 70);
    pub const DRIVEN_AI_DEFERRED: crate::Since = crate::Since(crate::Stored::Save, 71);
    pub const CLAUDE_SAVE_RECONCILE: crate::Since = crate::Since(crate::Stored::Save, 72);
    pub const REEL_NOTES: crate::Since = crate::Since(crate::Stored::Save, 73);
    pub const REPUTATION_HELD: crate::Since = crate::Since(crate::Stored::Save, 74);
    pub const SITH_BASE_FIXES: crate::Since = crate::Since(crate::Stored::Save, 75);
    pub const MOD_ROUTINES: crate::Since = crate::Since(crate::Stored::Save, 76);
    pub const MOD_ROUTINES_2: crate::Since = crate::Since(crate::Stored::Save, 77);
    pub const CLAUDE_K1_ROUTINES: crate::Since = crate::Since(crate::Stored::Save, 78);
    pub const CLAUDE_KREIA_HAND: crate::Since = crate::Since(crate::Stored::Save, 79);
    #[allow(unused_imports)]
    pub use crate::pending::since::save::*;
}

pub mod snapshot {
    pub const REMOTE_DROID: crate::Since = crate::Since(crate::Stored::Snapshot, 73);
    pub const CLAUDE_ITEMS_LOAD: crate::Since = crate::Since(crate::Stored::Snapshot, 74);
    pub const CLAUDE_UI_ISSUES: crate::Since = crate::Since(crate::Stored::Snapshot, 75);
    pub const FEAT_CYCLE: crate::Since = crate::Since(crate::Stored::Snapshot, 76);
    pub const ITEM_CYCLE: crate::Since = crate::Since(crate::Stored::Snapshot, 77);
    pub const DROID_FEEL: crate::Since = crate::Since(crate::Stored::Snapshot, 78);
    pub const CLAUDE_SAVE_RECONCILE: crate::Since = crate::Since(crate::Stored::Snapshot, 79);
    pub const REEL_NOTES: crate::Since = crate::Since(crate::Stored::Snapshot, 80);
    pub const TUSKEN_STANDING: crate::Since = crate::Since(crate::Stored::Snapshot, 81);
    pub const SITH_BASE_FIXES: crate::Since = crate::Since(crate::Stored::Snapshot, 82);
    pub const MOD_ROUTINES: crate::Since = crate::Since(crate::Stored::Snapshot, 83);
    pub const MOD_ROUTINES_2: crate::Since = crate::Since(crate::Stored::Snapshot, 84);
    pub const CLAUDE_K1_ROUTINES: crate::Since = crate::Since(crate::Stored::Snapshot, 85);
    pub const BR_COMBAT: crate::Since = crate::Since(crate::Stored::Snapshot, 86);
    pub const CLAUDE_BR_TSL_STORY: crate::Since = crate::Since(crate::Stored::Snapshot, 87);
    pub const CLAUDE_BR8_ISSUE_209: crate::Since = crate::Since(crate::Stored::Snapshot, 88);
    pub const BR9_KNOWN: crate::Since = crate::Since(crate::Stored::Snapshot, 89);
    pub const BR9_PARTY: crate::Since = crate::Since(crate::Stored::Snapshot, 90);
    pub const BR9B_TSL: crate::Since = crate::Since(crate::Stored::Snapshot, 91);
    pub const FEAT_DYNAMIC_CROSSHAIR: crate::Since = crate::Since(crate::Stored::Snapshot, 92);
    #[allow(unused_imports)]
    pub use crate::pending::since::snapshot::*;
}

pub mod pack {
    pub const CLAUDE_TSL_FINALE: crate::Since = crate::Since(crate::Stored::Pack, 21);
    pub const CLAUDE_BODIES_COLLISION: crate::Since = crate::Since(crate::Stored::Pack, 22);
    pub const COMPANION_THROWS: crate::Since = crate::Since(crate::Stored::Pack, 23);
    #[allow(unused_imports)]
    pub use crate::pending::since::pack::*;
}
