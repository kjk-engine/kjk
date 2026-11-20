//! Who each seat's player is at a session's start (`sim::persona`): the
//! character made in character creation for the first seat, else the
//! default the host gives each seat. The player portraits and the body each
//! gives a class are `session::portraits`.

use session::portraits::Portraits;
use sim::creation::Creation;
use sim::levels::Choices;
use sim::{Seat, SimWorld};
use ui::intent::CharacterDesc;
use vfs::resolver::Resolver;

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
    let abilities = session::classes::recommended(&table, row);
    // A level without classes (the course) takes the persona only.
    let skills = classes(world)
        .and_then(|classes| {
            let unmade = sim::creation::unmade_of(classes, class, abilities, 0);
            classes.auto(&unmade, &Vec::new(), class, 0)
        })
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

/// Every seat of `world` given its persona (the defaults; a seat drawn as a
/// body of `bodies`, the seats' in turn, a portrait of that body's sex, so
/// its portrait and voice match), and seat 0 the character `made`, when one
/// was: what was done, a line each.
pub fn start(
    world: &mut SimWorld,
    resolver: &Resolver,
    seats: u8,
    bodies: &[(String, String)],
    made: Option<Made>,
) -> Result<Vec<String>, String> {
    let portraits = Portraits::read(&crate::modules::ResolverSource(resolver))
        .ok_or("portraits.2da: not read")?;
    let appearances = resolver.two_da("appearance");
    let mut notes = Vec::new();
    for seat in (0..seats.max(1)).map(Seat) {
        let female = bodies
            .get(usize::from(seat.0))
            .zip(appearances.as_ref())
            .and_then(|((body, _), table)| portraits.female_body(table, body));
        if let Some(persona) = portraits.default(seat, female) {
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

/// Seat 0's character of a loaded `world` made again as `made`, wearing
/// `gear` (`SimWorld::remake`).
pub fn remake(
    world: &mut SimWorld,
    resolver: &Resolver,
    made: &CharacterDesc,
    gear: &[(&str, u8)],
) -> Result<(), String> {
    let portraits = Portraits::read(&crate::modules::ResolverSource(resolver))
        .ok_or("portraits.2da: not read")?;
    let creation = creation(&portraits, Seat(0), made)
        .ok_or(format!("portrait {}: not a player's", made.portrait))?;
    world.remake(&creation, gear)
}

/// The classes `world`'s level has, if any.
fn classes(world: &SimWorld) -> Option<&sim::levels::Classes> {
    let classes = world.level().scripts()?.classes();
    (!classes.classes.is_empty()).then_some(classes)
}

/// The body `appearance` wearing `armour` is drawn with (`appearance.2da`,
/// as a creature's: the armour's body variation and texture), and the
/// model of `headgear` (an item, a module's own read from `owner`:
/// `viewer::worn_model`) on its head, dressed for `aligned`
/// (`creature::align`).
pub fn wear(
    (resolver, owner): (&Resolver, Option<&Resolver>),
    tables: &mut asset_odyssey::tables::Tables,
    (appearance, armour, headgear, aligned): crate::headgear::SeatLook,
) -> Option<crate::play::Wear> {
    use asset_odyssey::creature;
    let source = crate::viewer::ResolverSource(resolver);
    let mut look =
        creature::resolve_appearance(&source, u32::from(appearance), armour, tables).ok()?;
    creature::align(&source, &mut look, aligned, tables);
    let strides = creature::strides(&source, u32::from(appearance), tables);
    Some(crate::play::Wear {
        body: look.body,
        head: look.head?,
        head_texture: look.head_texture,
        texture: look.body_texture,
        headgear: headgear
            .and_then(|item| crate::viewer::worn_model(owner, &source, tables, &item)),
        strides: strides.unwrap_or(crate::play::STRIDES),
        envmap: look.envmap,
    })
}
