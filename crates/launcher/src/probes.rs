//! The view layers' checks the approved tests run, without a window
//! (`docs/TESTS.md`): `grass_in_scenes`, `weather_in_scenes`,
//! `particle_limits` and `script_room_clips`; the playthrough's chase
//! (`harness_chase`); watching a pazaak match (`pazaak_watch_view`); and a
//! held node under a straightened spine (`held_node_straightened`); and a
//! companion drawn from what the party table keeps of it
//! (`kept_companion_drawn`); and the rooms a fight's light reaches
//! (`fight_light_rooms`); and what the capsule shadows send the shader
//! (`capsule_shadows`); and a character's look by its alignment
//! (`alignment_looks`).
//! Built with the `probes` feature, as the checks are.

pub use crate::alignment_look::probes::probes as alignment_looks;
pub use crate::cast::straighten_probes as straightened;
pub use crate::dangly::probes as dangly;
pub use crate::fight_rooms::probes::probes as fight_rooms;
pub use crate::grass::probes as grass;
pub use crate::model_fx::limit_probes as particle_limits;
pub use crate::own_fx::probes as script_room_clips;
pub use crate::pazaak_view::probes as pazaak_watch;
pub use crate::playthrough::probes as chase;
pub use crate::shadows::probes::probes as capsule_shadows;
pub use crate::viewer::kept_probes::probes as kept_companion;
pub use crate::weather::probes as weather;
