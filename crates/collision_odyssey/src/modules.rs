//! A whole module's collision, as the sim and `kjk` want it: the entry
//! area built with the module's clip brushes, and its doors set.

use asset_odyssey::module::{AreaIr, Kind, ModuleIr, Source};
use collision::clip_file::ClipFile;

use crate::{AreaCollision, BuildOptions, Loader, Surfaces, build_area, overrides};

/// Which doors are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doors {
    /// None.
    Closed,
    /// Every door that can open: not `Static`, and not a transition to
    /// another module (going through one loads that module instead).
    Openable,
    /// Every door.
    All,
}

impl Doors {
    pub fn name(self) -> &'static str {
        match self {
            Doors::Closed => "closed",
            Doors::Openable => "open where they can open",
            Doors::All => "all open",
        }
    }
}

/// Opens (state 1) the doors `doors` names and closes (state 0) the rest
/// of `area`'s (in `module`); returns how many are open.
pub fn set_doors(
    collision: &mut AreaCollision,
    area: &AreaIr,
    module: &str,
    doors: Doors,
) -> usize {
    let mut open = 0;
    for entity in &collision.entities {
        if entity.kind != Kind::Door {
            continue;
        }
        let Some(instance) = area.instances.get(entity.entity as usize) else {
            continue;
        };
        let transition = instance
            .linked_to
            .as_ref()
            .is_some_and(|(to, _)| !to.is_empty() && !to.eq_ignore_ascii_case(module));
        let opens = match doors {
            Doors::Closed => false,
            Doors::Openable => !instance.is_static && !transition,
            Doors::All => true,
        };
        open += usize::from(opens);
        collision.level.set_state(entity.entity, u8::from(opens));
    }
    open
}

/// The clip file built into kjk for `module` of `game` (`k1`, `tsl`),
/// parsed; `None` when there is none.
pub fn built_in_clip(game: &str, module: &str) -> Result<Option<ClipFile>, String> {
    overrides::text(game, module)
        .map(|text| {
            ClipFile::parse(text)
                .map_err(|error| format!("data/clip/{game}/{module}.clip: {error}"))
        })
        .transpose()
}

/// A module's entry area and its collision.
#[derive(Debug, Clone)]
pub struct ModuleCollision {
    pub module: ModuleIr,
    /// The entry area's index in `module.areas`.
    pub area: usize,
    pub surfaces: Surfaces,
    pub collision: AreaCollision,
}

impl ModuleCollision {
    pub fn area(&self) -> &AreaIr {
        &self.module.areas[self.area]
    }

    /// See [`set_doors`].
    pub fn set_doors(&mut self, doors: Doors) -> usize {
        let area = &self.module.areas[self.area];
        set_doors(&mut self.collision, area, &self.module.name, doors)
    }
}

/// Builds `module`'s entry area (its first when the IFO names none that
/// exists) with `clip`'s brushes; the doors start closed.
pub fn build_module(
    source: &dyn Source,
    module: &str,
    clip: Option<&ClipFile>,
    options: &BuildOptions,
) -> Result<ModuleCollision, String> {
    let module = module.to_ascii_lowercase();
    let ir =
        ModuleIr::build(&module, source).map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = ir
        .areas
        .iter()
        .position(|area| area.resref == ir.entry_area)
        .or((!ir.areas.is_empty()).then_some(0))
        .ok_or(format!("{module}: no area"))?;
    let surfaces = Surfaces::read(source).ok_or("surfacemat.2da does not resolve")?;
    let loader = Loader { source };
    let collision = build_area(&loader, &ir.areas[area], &surfaces, clip, options);
    Ok(ModuleCollision {
        module: ir,
        area,
        surfaces,
        collision,
    })
}
