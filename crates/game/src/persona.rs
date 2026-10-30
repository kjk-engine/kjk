//! Who each seat's player is at a session's start (`sim::persona`): the
//! character made in character creation for the first seat, else the
//! default the host gives each seat. A player portrait (`portraits.2da`,
//! `forpc`) names one body per build: a soldier's and a Jedi Guardian's the
//! large (`appearance_l`), a scoundrel's and a Jedi Consular's the small
//! (`appearance_s`), any other class's (a scout's, a Jedi Sentinel's) the
//! middle (`appearancenumber`), as the games' class selection and portrait
//! picker take them (TSL's picker by its class, reone's for K1's).

use fmt_odyssey::twoda::TwoDa;
use sim::creation::Creation;
use sim::levels::Choices;
use sim::persona::{FEMALE, MALE, Persona};
use sim::{Seat, SimWorld};
use ui::intent::CharacterDesc;
use vfs::resolver::Resolver;

/// `portraits.2da`'s bodies, by build: the large, the middle, the small.
const BUILDS: [&str; 3] = ["appearance_l", "appearancenumber", "appearance_s"];

/// The build (`BUILDS`) of a class's body, by `classes.2da` row: a soldier
/// and a Jedi Guardian the large, a scoundrel and a Jedi Consular the small,
/// any other the middle.
fn build(class: u8) -> usize {
    match class {
        0 | 3 => 0,
        2 | 4 => 2,
        _ => 1,
    }
}

/// The class a seat without a character made is drawn as: a soldier, as
/// the dev sheet it fights with.
const DEFAULT_CLASS: u8 = 0;

/// The player portraits, by row: their texture, whether female, and their
/// bodies by build.
pub struct Portraits(Vec<(u16, String, bool, [Option<u16>; 3])>);

impl Portraits {
    pub fn read(resolver: &Resolver) -> Option<Portraits> {
        Some(Portraits::of(&resolver.two_da("portraits")?))
    }

    pub fn of(table: &TwoDa) -> Portraits {
        let cell = |row: usize, name: &str| table.named(row, table.column(name)?);
        let number = |row: usize, name: &str| cell(row, name)?.parse::<u16>().ok();
        Portraits(
            (0..table.cells.len())
                .filter(|row| number(*row, "forpc") == Some(1))
                .filter_map(|row| {
                    Some((
                        u16::try_from(row).ok()?,
                        cell(row, "baseresref")?,
                        number(row, "sex") == Some(u16::from(FEMALE)),
                        BUILDS.map(|column| number(row, column)),
                    ))
                })
                .collect(),
        )
    }

    /// The player portraits' rows.
    #[cfg(feature = "probes")]
    pub fn rows(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().map(|(row, ..)| *row)
    }

    /// The persona of portrait `texture` for `class`, if it is a player's.
    pub fn persona(&self, seat: Seat, texture: &str, class: u8, name: &str) -> Option<Persona> {
        let (row, _, female, bodies) = self
            .0
            .iter()
            .find(|(_, of, ..)| of.eq_ignore_ascii_case(texture))?;
        Some(Persona {
            seat,
            appearance: bodies[build(class)]?,
            gender: if *female { FEMALE } else { MALE },
            portrait: *row,
            name: name.to_string(),
        })
    }

    /// A seat's default: male and female in turn, each sex's portraits in
    /// order, a soldier's body.
    pub fn default(&self, seat: Seat) -> Option<Persona> {
        let female = seat.0 % 2 == 1;
        let of_sex: Vec<&String> = self
            .0
            .iter()
            .filter(|(_, _, sex, _)| *sex == female)
            .map(|(_, texture, ..)| texture)
            .collect();
        let texture = of_sex.get(usize::from(seat.0 / 2) % of_sex.len().max(1))?;
        self.persona(seat, texture, DEFAULT_CLASS, "")
    }

    /// The body the first portrait of a sex gives `class`.
    pub fn first(&self, female: bool, class: u8) -> Option<u16> {
        let (.., bodies) = self.0.iter().find(|(_, _, sex, _)| *sex == female)?;
        bodies[build(class)]
    }
}

/// What `made` asks the sim to make of `seat`'s player.
pub fn creation(portraits: &Portraits, seat: Seat, made: &CharacterDesc) -> Option<Creation> {
    Some(Creation {
        persona: portraits.persona(seat, &made.portrait, made.class, &made.name)?,
        abilities: made.abilities,
        choices: Choices {
            class: made.class,
            ability: None,
            skills: made.skills,
            feats: made.feats.clone(),
            powers: Vec::new(),
        },
    })
}

/// The character the first seat plays as, when one was made.
#[derive(Debug, Clone, Copy)]
pub enum Made<'a> {
    /// Character creation's.
    Desc(&'a CharacterDesc),
    /// Quick creation's by hand (`kjk play --made`): a class
    /// (`classes.2da` row), a player portrait, a name; the class's
    /// recommended scores and skills.
    Quick {
        class: u8,
        portrait: &'a str,
        name: &'a str,
    },
}

/// `classes.2da`'s score columns, strength to charisma (`sheet::Ability`
/// order): a class's recommended scores.
pub(crate) const SCORES: [&str; 6] = ["str", "dex", "con", "int", "wis", "cha"];

/// Quick creation of `class`: its recommended scores, and the skills the
/// game picks for its first level (`Classes::auto`).
fn quick(
    world: &SimWorld,
    resolver: &Resolver,
    (class, portrait, name): (u8, &str, &str),
) -> Result<CharacterDesc, String> {
    let table = resolver
        .two_da("classes")
        .ok_or("classes.2da: not found or unreadable")?;
    let row = usize::from(class);
    let abilities = SCORES.map(|column| {
        table
            .column(column)
            .and_then(|at| table.named(row, at))
            .and_then(|score| score.parse().ok())
            .unwrap_or(sim::creation::LOWEST)
    });
    // A level without classes (the course) takes the persona only.
    let unmade = sim::creation::unmade(abilities, 0);
    let skills = classes(world)
        .and_then(|classes| classes.auto(&unmade, &Vec::new(), class, 0))
        .map_or([0; 8], |picks| picks.skills);
    Ok(CharacterDesc {
        class,
        female: false,
        portrait: portrait.to_string(),
        abilities,
        skills,
        name: name.to_string(),
        feats: Vec::new(),
    })
}

/// Every seat of `world` given its persona (the defaults), and seat 0 the
/// character `made`, when one was: what was done, a line each.
pub fn start(
    world: &mut SimWorld,
    resolver: &Resolver,
    seats: u8,
    made: Option<Made>,
) -> Result<Vec<String>, String> {
    let portraits = Portraits::read(resolver).ok_or("portraits.2da: not read")?;
    let mut notes = Vec::new();
    for seat in (0..seats.max(1)).map(Seat) {
        if let Some(persona) = portraits.default(seat) {
            notes.push(format!(
                "seat {}: appearance {}",
                seat.0, persona.appearance
            ));
            world.set_persona(persona);
        }
    }
    let quickly;
    let made = match made {
        Some(Made::Desc(made)) => Some(made),
        Some(Made::Quick {
            class,
            portrait,
            name,
        }) => {
            quickly = quick(world, resolver, (class, portrait, name))?;
            Some(&quickly)
        }
        None => None,
    };
    if let Some(made) = made {
        let creation = creation(&portraits, Seat(0), made)
            .ok_or(format!("portrait {}: not a player's", made.portrait))?;
        notes.push(format!(
            "seat 0 made: {} (class {}, appearance {})",
            made.name, made.class, creation.persona.appearance
        ));
        if classes(world).is_some() {
            world.create(&creation)?;
        } else {
            notes.push("no classes here: the made character's sheet is not applied".into());
            world.set_persona(creation.persona);
        }
    }
    Ok(notes)
}

/// The classes `world`'s level has, if any.
fn classes(world: &SimWorld) -> Option<&sim::levels::Classes> {
    let classes = world.level().scripts()?.classes();
    (!classes.classes.is_empty()).then_some(classes)
}

/// The body `appearance` wearing `armour` is drawn with (`appearance.2da`,
/// as a creature's: the armour's body variation and texture).
pub fn wear(
    resolver: &Resolver,
    tables: &mut asset_odyssey::tables::Tables,
    (appearance, armour): (u16, Option<String>),
) -> Option<crate::play::Wear> {
    let source = crate::viewer::ResolverSource(resolver);
    let look =
        asset_odyssey::creature::resolve_appearance(&source, u32::from(appearance), armour, tables)
            .ok()?;
    let strides = asset_odyssey::creature::strides(&source, u32::from(appearance), tables);
    Some(crate::play::Wear {
        body: look.body,
        head: look.head?,
        texture: look.body_texture,
        strides: strides.unwrap_or(crate::play::STRIDES),
        chest: look.chest,
    })
}

/// The persona of the player whose own character is `id` in `snapshot`.
pub fn of_character(snapshot: &sim::Snapshot, id: u32) -> Option<&Persona> {
    let entity = snapshot.entities.iter().find(|entity| entity.id.0 == id)?;
    match entity.kind {
        sim::Kind::Avatar(seat) => sim::persona::of(&snapshot.personas, seat),
        _ => None,
    }
}
