//! The view layers' checks the approved tests run, without a window
//! (`docs/TESTS.md`): `grass_in_scenes`, `weather_in_scenes`,
//! `particle_limits` and `script_room_clips`; the playthrough's chase
//! (`harness_chase`); and watching a pazaak match (`pazaak_watch_view`).
//! Built with the `probes` feature, as the checks are.

pub use crate::grass::probes as grass;
pub use crate::model_fx::limit_probes as particle_limits;
pub use crate::own_fx::probes as script_room_clips;
pub use crate::pazaak_view::probes as pazaak_watch;
pub use crate::playthrough::probes as chase;
pub use crate::weather::probes as weather;
