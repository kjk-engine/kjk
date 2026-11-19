//! The player portraits (`portraits.2da` rows with `forpc` set) and the
//! persona each gives a class. A player portrait names one body per build:
//! a soldier's and a Jedi Guardian's the large (`appearance_l`), a
//! scoundrel's and a Jedi Consular's the small (`appearance_s`), any other
//! class's (a scout's, a Jedi Sentinel's) the middle (`appearancenumber`),
//! as the games' class selection and portrait picker take them (TSL's
//! picker by its class, reone's for K1's). The body is chosen by the
//! character's first class.

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;
use sim::Seat;
use sim::persona::{FEMALE, MALE, Persona};

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

/// One player portrait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    /// Its `portraits.2da` row.
    pub row: u16,
    /// Its texture (`baseresref`).
    pub texture: String,
    pub female: bool,
    /// Its `appearance.2da` rows by build (`BUILDS`).
    bodies: [Option<u16>; 3],
}

/// The player portraits, by row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Portraits(Vec<Portrait>);

impl Portraits {
    /// The player portraits of `source`'s `portraits.2da`, if it reads.
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

    /// Every player portrait, in row order.
    pub fn all(&self) -> &[Portrait] {
        &self.0
    }

    /// The player portrait of `portraits.2da` row `row`, if it is one.
    pub fn at(&self, row: u16) -> Option<&Portrait> {
        self.0.iter().find(|portrait| portrait.row == row)
    }

    /// The persona of portrait `texture` for `class`, if it is a player's.
    pub fn persona(&self, seat: Seat, texture: &str, class: u8, name: &str) -> Option<Persona> {
        let portrait = self
            .0
            .iter()
            .find(|portrait| portrait.texture.eq_ignore_ascii_case(texture))?;
        Some(Persona {
            seat,
            appearance: portrait.bodies[build(class)]?,
            gender: if portrait.female { FEMALE } else { MALE },
            portrait: portrait.row,
            name: name.to_string(),
        })
    }

    /// `persona` with player portrait `row`'s head and the body it gives
    /// `class`, its gender and name kept. Without a class (a save that does
    /// not record it), the body is of the build `persona` wears now. None
    /// when `row` is not a player portrait, is of the other gender, or has
    /// no body for that build.
    pub fn worn(&self, persona: &Persona, row: u16, class: Option<u8>) -> Option<Persona> {
        let build = match class {
            Some(class) => build(class),
            None => self
                .at(persona.portrait)?
                .bodies
                .iter()
                .position(|body| *body == Some(persona.appearance))?,
        };
        let portrait = self
            .at(row)
            .filter(|portrait| portrait.female == persona.female())?;
        Some(Persona {
            appearance: portrait.bodies[build]?,
            portrait: row,
            ..persona.clone()
        })
    }

    /// A seat's default: of the sex given, else male and female in turn,
    /// each sex's portraits in order, a soldier's body.
    pub fn default(&self, seat: Seat, female: Option<bool>) -> Option<Persona> {
        let female = female.unwrap_or(seat.0 % 2 == 1);
        let of_sex: Vec<&String> = self
            .0
            .iter()
            .filter(|portrait| portrait.female == female)
            .map(|portrait| &portrait.texture)
            .collect();
        let texture = of_sex.get(usize::from(seat.0 / 2) % of_sex.len().max(1))?;
        self.persona(seat, texture, DEFAULT_CLASS, "")
    }

    /// Whether body model `body` is a woman's: the sex of every player
    /// portrait whose `appearance.2da` rows name it (`names_body`),
    /// when they agree; none for a body no player portrait wears.
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
