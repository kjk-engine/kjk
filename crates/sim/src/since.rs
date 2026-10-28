//! The version each change to a format saves hold arrived in, by the name
//! of its bump file (`bumps/README.md`): a decoder reads the layout before
//! it with `if input.since(crate::since::<format>::<NAME>)`: the version of
//! that format the bytes are in, wherever in a save they lie. A pending
//! bump's comes from the build script (`crate::pending::since`); the merge's
//! pin (`cargo xtask pin`) writes its number here. A format with no bump
//! pending re-exports nothing, hence the `allow`s.

pub mod save {
    #[allow(unused_imports)]
    pub use crate::pending::since::save::*;
}

pub mod snapshot {
    #[allow(unused_imports)]
    pub use crate::pending::since::snapshot::*;
}

pub mod pack {
    #[allow(unused_imports)]
    pub use crate::pending::since::pack::*;
}
