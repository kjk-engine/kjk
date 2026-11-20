//! The player portraits are the `portraits.2da` rows with `forpc` set.
//! Each player portrait names one body per build. A soldier and a Jedi
//! Guardian wear the large body (`appearance_l`). A scoundrel and a Jedi
//! Consular wear the small body (`appearance_s`). Any other class, such as
//! a scout or a Jedi Sentinel, wears the middle body (`appearancenumber`).
//! The games' class selection and portrait picker choose the body this
//! way, by the character's first class.

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;
use sim::Seat;
use sim::persona::{FEMALE, MALE, Persona};

/// The `portraits.2da` columns of the bodies, in build order. The large
/// build comes first, then the middle, then the small.
const BUILDS: [&str; 3] = ["appearance_l", "appearancenumber", "appearance_s"];

/// The build (an index into `BUILDS`) of the body that a class wears, by
/// its `classes.2da` row.
fn build(class: u8) -> usize {
    match class {
        0 | 3 => 0,
        2 | 4 => 2,
        _ => 1,
    }
}

/// The class that a seat without a made character is drawn as. It is a
/// soldier, as the dev sheet it fights with.
const DEFAULT_CLASS: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    pub row: u16,
    /// The `baseresref` column.
    pub texture: String,
    pub female: bool,
    /// The `appearance.2da` rows of the bodies, by build (`BUILDS`).
    bodies: [Option<u16>; 3],
}

/// Why a persona cannot wear a portrait (`Portraits::worn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotAPlayerPortrait,
    OtherGender,
    NoBody,
    /// No class was given, and the body worn now is not one of the
    /// persona's own portrait's bodies, so its build is unknown.
    UnknownBuild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portraits(Vec<Portrait>);

impl Portraits {
    pub fn read(source: &dyn Source) -> Option<Portraits> {
        let bytes = source.read("portraits", ResType::TWO_DA)?;
        Some(Portraits::of(&TwoDa::parse(&bytes).ok()?))
    }

    pub fn of(table: &TwoDa) -> Portraits {
        let cell = |row: usize, name: &str| table.named(row, table.column(name)?);
        let number = |row: usize, name: &str| cell(row, name)?.parse::<u16>().ok();
        Portraits(
            (0..table.cells.len())
                .filter(|row| number(*row, "forpc") == Some(1))
                .filter_map(|row| {
                    Some(Portrait {
                        row: u16::try_from(row).ok()?,
                        texture: cell(row, "baseresref")?,
                        female: number(row, "sex") == Some(u16::from(FEMALE)),
                        bodies: BUILDS.map(|column| number(row, column)),
                    })
                })
                .collect(),
        )
    }

    pub fn all(&self) -> &[Portrait] {
        &self.0
    }

    pub fn at(&self, row: u16) -> Option<&Portrait> {
        self.0.iter().find(|portrait| portrait.row == row)
    }

    pub fn of_gender(&self, female: bool) -> impl Iterator<Item = &Portrait> {
        self.0
            .iter()
            .filter(move |portrait| portrait.female == female)
    }

    /// The persona of the player portrait whose texture is `texture`,
    /// wearing the body that `class` takes.
    pub fn persona(&self, seat: Seat, texture: &str, class: u8, name: &str) -> Option<Persona> {
        let portrait = self
            .0
            .iter()
            .find(|portrait| portrait.texture.eq_ignore_ascii_case(texture))?;
        dressed(portrait, build(class), seat, name).ok()
    }

    /// `persona` with player portrait `row`'s head and the body it gives
    /// `class`, its gender and name kept. Without a class (a save that does
    /// not record it), the body is of the build `persona` wears now.
    pub fn worn(&self, persona: &Persona, row: u16, class: Option<u8>) -> Result<Persona, Refusal> {
        let portrait = self.at(row).ok_or(Refusal::NotAPlayerPortrait)?;
        if portrait.female != persona.female() {
            return Err(Refusal::OtherGender);
        }
        let build = match class {
            Some(class) => build(class),
            None => self
                .at(persona.portrait)
                .and_then(|own| {
                    own.bodies
                        .iter()
                        .position(|body| *body == Some(persona.appearance))
                })
                .ok_or(Refusal::UnknownBuild)?,
        };
        let worn = dressed(portrait, build, persona.seat, &persona.name)?;
        Ok(Persona {
            gender: persona.gender,
            ..worn
        })
    }

    /// A seat's default persona. It is of the sex given, or else male and
    /// female in turn by seat. Each sex's portraits are taken in order, and
    /// the body is a soldier's.
    pub fn default(&self, seat: Seat, female: Option<bool>) -> Option<Persona> {
        let female = female.unwrap_or(seat.0 % 2 == 1);
        let of_sex: Vec<&Portrait> = self.of_gender(female).collect();
        let portrait = of_sex.get(usize::from(seat.0 / 2) % of_sex.len().max(1))?;
        dressed(portrait, build(DEFAULT_CLASS), seat, "").ok()
    }

    /// Whether body model `body` is a woman's. It is the sex of every
    /// player portrait whose `appearance.2da` rows name that body
    /// (`names_body`), when they all agree. A body that no player portrait
    /// wears gives none.
    pub fn female_body(&self, appearances: &TwoDa, body: &str) -> Option<bool> {
        let wears =
            |row: u16| asset_odyssey::creature::names_body(appearances, usize::from(row), body);
        let mut sexes = self
            .0
            .iter()
            .filter(|portrait| portrait.bodies.iter().flatten().any(|&row| wears(row)))
            .map(|portrait| portrait.female);
        let first = sexes.next()?;
        sexes.all(|female| female == first).then_some(first)
    }
}

fn dressed(portrait: &Portrait, build: usize, seat: Seat, name: &str) -> Result<Persona, Refusal> {
    Ok(Persona {
        seat,
        appearance: portrait.bodies[build].ok_or(Refusal::NoBody)?,
        gender: if portrait.female { FEMALE } else { MALE },
        portrait: portrait.row,
        name: name.to_string(),
    })
}
