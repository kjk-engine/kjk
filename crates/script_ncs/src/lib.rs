//! NWScript for the sim (I16): the engine routines a game's scripts call
//! (`catalog`, read from its `nwscript.nss`), and later the VM that runs
//! compiled scripts (`fmt_odyssey::ncs`) on the authority.
//!
//! Pure: no file I/O, no game data in the repository. The launcher reads
//! the install and hands the bytes over.

pub mod catalog;
pub mod disasm;
#[cfg(feature = "probes")]
pub mod probes;
pub mod vm;

pub use catalog::{Catalog, CatalogError, Routine, Type};
